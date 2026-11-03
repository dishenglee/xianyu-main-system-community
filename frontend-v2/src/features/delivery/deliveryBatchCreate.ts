type DeliveryProductLike = {
  id: string
  online?: boolean
  quantity?: number | null
  [key: string]: unknown
}

export function planDeliveryBatchCreate<T extends DeliveryProductLike>({
  selectedItemIds,
  products,
  configuredItemIds,
}: {
  selectedItemIds: string[]
  products: T[]
  configuredItemIds: string[]
}) {
  const productMap = new Map(products.map((product) => [product.id, product]))
  const configured = new Set(configuredItemIds)
  const targets: T[] = []
  const blocked: Array<{ itemId: string; reason: string }> = []

  for (const itemId of Array.from(new Set(selectedItemIds.filter(Boolean)))) {
    const product = productMap.get(itemId)
    if (!product) blocked.push({ itemId, reason: '商品不存在或已移除' })
    else if (configured.has(itemId)) blocked.push({ itemId, reason: '该商品已存在自动发货配置' })
    else if (!product.online) blocked.push({ itemId, reason: '商品已下架' })
    else if (product.quantity === undefined || product.quantity === null || Number(product.quantity) <= 0) blocked.push({ itemId, reason: '商品库存不足' })
    else targets.push(product)
  }
  return { targets, blocked }
}
