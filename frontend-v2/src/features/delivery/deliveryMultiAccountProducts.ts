import { limitConcurrency } from '../../hooks/limitConcurrency'
import {
  getStockRefreshCandidates,
  normalizeDeliveryProducts,
  type DeliveryProductOption,
} from './deliveryProducts'

export type DeliveryAccountProductState = {
  items: DeliveryProductOption[]
  loading: boolean
  error: string
  stockSyncing: number
  stockFailures: number
}

export const emptyDeliveryAccountProductState = (): DeliveryAccountProductState => ({
  items: [],
  loading: false,
  error: '',
  stockSyncing: 0,
  stockFailures: 0,
})

export async function loadDeliveryAccountProducts({
  uid,
  listProducts,
  refreshStock,
  isCurrent,
  onProductsLoaded,
}: {
  uid: string
  listProducts: (uid: string, page: number, pageSize: number) => Promise<unknown>
  refreshStock: (uid: string, itemId: string) => Promise<{ quantity?: number | null }>
  isCurrent: () => boolean
  onProductsLoaded?: (state: DeliveryAccountProductState) => void
}): Promise<DeliveryAccountProductState | null> {
  try {
    const response = await listProducts(uid, 1, 200)
    if (!isCurrent()) return null
    const normalized = normalizeDeliveryProducts(response)
    const pending = getStockRefreshCandidates(normalized)
    onProductsLoaded?.({
      items: normalized,
      loading: pending.length > 0,
      error: '',
      stockSyncing: pending.length,
      stockFailures: 0,
    })
    if (!pending.length) return { items: normalized, loading: false, error: '', stockSyncing: 0, stockFailures: 0 }

    const results = await limitConcurrency(pending, 3, async (item) => {
      try {
        const result = await refreshStock(uid, item.id)
        return { id: item.id, quantity: result.quantity ?? null, failed: false }
      } catch {
        return { id: item.id, quantity: null, failed: true }
      }
    })
    if (!isCurrent()) return null
    const resultMap = new Map(results.map((result) => [result.id, result]))
    return {
      items: normalized.map((item) => {
        const result = resultMap.get(item.id)
        return result && !result.failed ? { ...item, quantity: result.quantity } : item
      }),
      loading: false,
      error: '',
      stockSyncing: 0,
      stockFailures: results.filter((result) => result.failed || result.quantity === null).length,
    }
  } catch (cause) {
    if (!isCurrent()) return null
    return {
      items: [],
      loading: false,
      error: cause instanceof Error ? cause.message : '商品加载失败',
      stockSyncing: 0,
      stockFailures: 0,
    }
  }
}
