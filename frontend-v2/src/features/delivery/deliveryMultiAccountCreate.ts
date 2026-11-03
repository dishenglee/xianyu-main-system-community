import { limitConcurrency } from '../../hooks/limitConcurrency'
import { planDeliveryBatchCreate } from './deliveryBatchCreate'
import type { DeliveryProductOption } from './deliveryProducts'

export type DeliveryItemsByUid = Record<string, DeliveryProductOption[]>
export type DeliveryItemSelectionByUid = Record<string, string[]>

export type DeliveryCreateFailure = {
  uid: string
  itemId: string
  title: string
  stage: 'plan' | 'stock' | 'create'
  reason: string
}

export type DeliveryCreatedItem = {
  uid: string
  itemId: string
  title: string
}

export function toggleDeliveryAccountSelection(selectedUids: string[], uid: string) {
  return selectedUids.includes(uid)
    ? selectedUids.filter((selectedUid) => selectedUid !== uid)
    : [...selectedUids, uid]
}

export function removeDeliveryAccountSelection(
  selection: DeliveryItemSelectionByUid,
  uid: string,
) {
  const next = { ...selection }
  delete next[uid]
  return next
}

export function toggleDeliveryItemSelection(
  selection: DeliveryItemSelectionByUid,
  uid: string,
  itemId: string,
) {
  const current = selection[uid] ?? []
  return {
    ...selection,
    [uid]: current.includes(itemId)
      ? current.filter((selectedItemId) => selectedItemId !== itemId)
      : [...current, itemId],
  }
}

export function countDeliveryItemSelection(selection: DeliveryItemSelectionByUid) {
  return Object.values(selection).reduce((total, itemIds) => total + itemIds.length, 0)
}

export function planDeliveryMultiAccountCreate({
  selectedUids,
  selectedItemIdsByUid,
  productsByUid,
  configuredItemIdsByUid,
}: {
  selectedUids: string[]
  selectedItemIdsByUid: DeliveryItemSelectionByUid
  productsByUid: DeliveryItemsByUid
  configuredItemIdsByUid: Record<string, string[]>
}) {
  return selectedUids.map((uid) => {
    const products = productsByUid[uid] ?? []
    const plan = planDeliveryBatchCreate({
      selectedItemIds: selectedItemIdsByUid[uid] ?? [],
      products,
      configuredItemIds: configuredItemIdsByUid[uid] ?? [],
    })
    const productMap = new Map(products.map((product) => [product.id, product]))
    return {
      uid,
      targets: plan.targets,
      blocked: plan.blocked.map((blocked) => ({
        uid,
        itemId: blocked.itemId,
        title: productMap.get(blocked.itemId)?.title ?? blocked.itemId,
        stage: 'plan' as const,
        reason: blocked.reason,
      })),
    }
  })
}

export async function runDeliveryMultiAccountCreate({
  selectedUids,
  selectedItemIdsByUid,
  productsByUid,
  configuredItemIdsByUid,
  refreshStock,
  createConfiguration,
  payload,
}: {
  selectedUids: string[]
  selectedItemIdsByUid: DeliveryItemSelectionByUid
  productsByUid: DeliveryItemsByUid
  configuredItemIdsByUid: Record<string, string[]>
  refreshStock: (uid: string, itemId: string) => Promise<{ quantity?: number | null }>
  createConfiguration: (payload: Record<string, unknown>) => Promise<unknown>
  payload: Record<string, unknown>
}) {
  const plans = planDeliveryMultiAccountCreate({
    selectedUids,
    selectedItemIdsByUid,
    productsByUid,
    configuredItemIdsByUid,
  })
  const failures: DeliveryCreateFailure[] = plans.flatMap((plan) => plan.blocked)
  const targets = plans.flatMap((plan) => plan.targets.map((product) => ({ uid: plan.uid, product })))

  const stockResults = await limitConcurrency(targets, 3, async ({ uid, product }) => {
    try {
      const result = await refreshStock(uid, product.id)
      const quantity = result.quantity ?? null
      return Number(quantity) > 0
        ? { uid, product, quantity, reason: '' }
        : { uid, product, quantity, reason: '商品库存不足' }
    } catch (cause) {
      return {
        uid,
        product,
        quantity: null,
        reason: cause instanceof Error ? cause.message : '库存刷新失败',
      }
    }
  })

  for (const result of stockResults) {
    if (!result.reason) continue
    failures.push({
      uid: result.uid,
      itemId: result.product.id,
      title: result.product.title,
      stage: 'stock',
      reason: result.reason,
    })
  }

  const createResults = await limitConcurrency(
    stockResults.filter((result) => !result.reason),
    3,
    async ({ uid, product }) => {
      try {
        await createConfiguration({ ...payload, uid, itemId: product.id })
        return { uid, product, success: true, reason: '' }
      } catch (cause) {
        return {
          uid,
          product,
          success: false,
          reason: cause instanceof Error ? cause.message : '创建失败',
        }
      }
    },
  )

  const created: DeliveryCreatedItem[] = []
  for (const result of createResults) {
    if (result.success) {
      created.push({ uid: result.uid, itemId: result.product.id, title: result.product.title })
    } else {
      failures.push({
        uid: result.uid,
        itemId: result.product.id,
        title: result.product.title,
        stage: 'create',
        reason: result.reason,
      })
    }
  }

  return {
    plans,
    created,
    failures,
    stock: stockResults.map((result) => ({
      uid: result.uid,
      itemId: result.product.id,
      quantity: result.quantity,
      success: !result.reason,
    })),
  }
}

export function retainFailedDeliverySelections(
  failures: DeliveryCreateFailure[],
): DeliveryItemSelectionByUid {
  return failures.reduce<DeliveryItemSelectionByUid>((selection, failure) => {
    if (!failure.itemId) return selection
    const current = selection[failure.uid] ?? []
    if (!current.includes(failure.itemId)) selection[failure.uid] = [...current, failure.itemId]
    return selection
  }, {})
}

export function formatDeliveryCreateFailures(
  failures: DeliveryCreateFailure[],
  accountLabels: Map<string, string>,
) {
  return failures.map((failure) => (
    `${accountLabels.get(failure.uid) ?? failure.uid} / ${failure.title || failure.itemId}：${failure.reason}`
  )).join('；')
}
