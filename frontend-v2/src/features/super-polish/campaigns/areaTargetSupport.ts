export interface AreaTargetSupportCandidate {
  title: string
  supportAreaTarget: boolean
}

export interface AreaTargetSupportSummary {
  totalCount: number
  supportedCount: number
  unsupportedCount: number
  available: boolean
  partiallySupported: boolean
  fullySupported: boolean
  supportedTitles: string[]
  unsupportedTitles: string[]
}

function normalizeTitles(items: AreaTargetSupportCandidate[]) {
  return items
    .map((item) => item.title.trim())
    .filter(Boolean)
}

export function summarizeAreaTargetSupport(
  items: AreaTargetSupportCandidate[],
): AreaTargetSupportSummary {
  const supportedItems = items.filter((item) => item.supportAreaTarget)
  const unsupportedItems = items.filter((item) => !item.supportAreaTarget)
  const totalCount = items.length
  const supportedCount = supportedItems.length
  const unsupportedCount = unsupportedItems.length

  return {
    totalCount,
    supportedCount,
    unsupportedCount,
    available: supportedCount > 0,
    partiallySupported: supportedCount > 0 && unsupportedCount > 0,
    fullySupported: totalCount > 0 && supportedCount === totalCount,
    supportedTitles: normalizeTitles(supportedItems),
    unsupportedTitles: normalizeTitles(unsupportedItems),
  }
}
