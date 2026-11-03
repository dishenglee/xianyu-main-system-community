export type DeliveryPendingOrderMismatchLike = {
  orderId?: string | null
  itemId?: string | null
  title?: string | null
  status?: string | null
  statusText?: string | null
}

export type DeliveryConfigurationLike = {
  id: number
  uid: string
  itemId: string
  poolTag?: string | null
  poolTags?: string[] | null
  enabled?: boolean
  needsRepair?: boolean
  product?: { quantity?: number | null } | null
  pendingOrderMismatches?: DeliveryPendingOrderMismatchLike[] | null
  [key: string]: unknown
}

export type DeliveryAccountGroup<T extends DeliveryConfigurationLike> = {
  uid: string
  configs: T[]
  configuredCount: number
  enabledCount: number
  poolTagCount: number
  repairCount: number
  stockIssueCount: number
  mismatches: DeliveryPendingOrderMismatchLike[]
  mismatchCount: number
}

export function buildDeliveryAccountGroups<T extends DeliveryConfigurationLike>(configs: T[]): DeliveryAccountGroup<T>[] {
  const byUid = new Map<string, T[]>()
  configs.forEach((config) => {
    const accountConfigs = byUid.get(config.uid)
    if (accountConfigs) accountConfigs.push(config)
    else byUid.set(config.uid, [config])
  })

  return Array.from(byUid, ([uid, accountConfigs]) => {
    const mismatchesByOrderAndItem = new Map<string, DeliveryPendingOrderMismatchLike>()
    accountConfigs.forEach((config) => {
      config.pendingOrderMismatches?.forEach((mismatch) => {
        const orderId = String(mismatch.orderId ?? '')
        const itemId = String(mismatch.itemId ?? '')
        const key = `${orderId}\u0000${itemId}`
        if (!mismatchesByOrderAndItem.has(key)) mismatchesByOrderAndItem.set(key, mismatch)
      })
    })
    const mismatches = Array.from(mismatchesByOrderAndItem.values())
    return {
      uid,
      configs: accountConfigs,
      configuredCount: accountConfigs.length,
      enabledCount: accountConfigs.filter((config) => config.enabled).length,
      poolTagCount: new Set(accountConfigs.flatMap((config) => config.poolTags?.length ? config.poolTags : (config.poolTag ? [config.poolTag] : []))).size,
      repairCount: accountConfigs.filter((config) => config.needsRepair).length,
      stockIssueCount: accountConfigs.filter((config) => {
        const quantity = config.product?.quantity
        return quantity !== undefined && quantity !== null && Number(quantity) <= 0
      }).length,
      mismatches,
      mismatchCount: mismatches.length,
    }
  }).sort((left, right) => right.mismatchCount - left.mismatchCount)
}
