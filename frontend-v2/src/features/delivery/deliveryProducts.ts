export type DeliveryProductOption = {
  id: string
  title: string
  price: string
  status: string
  quantity: number | null
  online: boolean
  picUrl: string
}

function readString(source: Record<string, unknown> | null | undefined, keys: string[], fallback = '') {
  for (const key of keys) {
    const value = source?.[key]
    if (value !== undefined && value !== null && String(value).trim()) return String(value).trim()
  }
  return fallback
}

function readNumber(source: Record<string, unknown> | null | undefined, keys: string[]) {
  for (const key of keys) {
    const value = source?.[key]
    if (value === undefined || value === null || value === '') continue
    const number = Number(value)
    if (Number.isFinite(number)) return number
  }
  return null
}

function readOnline(value: unknown, status: string) {
  if (/下架|售罄|已删除|sold[_ -]?out|off[_ -]?shelf|deleted/i.test(status)) return false
  if (value === undefined || value === null || value === '') return true
  if (typeof value === 'boolean') return value
  if (typeof value === 'number') return value !== 0
  const normalized = String(value).trim().toLowerCase()
  return !['0', 'false', 'no', 'off'].includes(normalized)
}

function extractList(payload: unknown): Record<string, unknown>[] {
  const root = payload as Record<string, unknown> | null | undefined
  const data = root?.data as Record<string, unknown> | unknown[] | null | undefined
  const candidates = [
    payload,
    root?.items,
    root?.itemList,
    data,
    !Array.isArray(data) ? data?.items : undefined,
    !Array.isArray(data) ? data?.itemList : undefined,
    !Array.isArray(data) ? data?.list : undefined,
  ]
  return (candidates.find(Array.isArray) as Record<string, unknown>[] | undefined) ?? []
}

export function normalizeDeliveryProducts(payload: unknown): DeliveryProductOption[] {
  const seen = new Set<string>()
  const products: DeliveryProductOption[] = []
  for (const item of extractList(payload)) {
    const id = readString(item, ['itemId', 'item_id', 'id'])
    if (!id || seen.has(id)) continue
    seen.add(id)
    const status = readString(item, ['statusText', 'status_text', 'status', 'itemStatus'])
    const online = readOnline(item.online, status)
    if (!online) continue
    products.push({
      id,
      title: readString(item, ['title', 'itemTitle', 'name', 'desc'], id),
      price: readString(item, ['priceText', 'price', 'soldPrice', 'orig_price']),
      status,
      quantity: readNumber(item, ['quantity', 'stock', 'stockNum', 'inventory']),
      online,
      picUrl: readString(item, ['picUrl', 'pic_url', 'cover_url', 'coverUrl', 'imageUrl', 'mainPic', 'cover']),
    })
  }
  return products
}

export function getStockRefreshCandidates(products: DeliveryProductOption[]) {
  return products.filter((item) => item.online && item.quantity === null)
}

export function getSelectableDeliveryProducts(products: DeliveryProductOption[]) {
  return products.filter((item) => item.online && item.quantity !== null && item.quantity > 0)
}
