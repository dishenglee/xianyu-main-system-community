type AccountLike = { uid?: unknown }

export function buildDeliveryAccountScopeTargets(accounts: AccountLike[], selectedUid: string) {
  const activeUids = accounts.map((account) => String(account.uid ?? '')).filter(Boolean)
  if (!selectedUid) return activeUids
  return activeUids.includes(selectedUid) ? [selectedUid] : []
}

export function mergeDeliveryAccountResults<T extends { uid?: unknown }>(
  results: Array<{ uid: string; records: T[] }>,
) {
  const seen = new Set<string>()
  return results.flatMap(({ uid, records }) => records.flatMap((record) => {
    const next = { ...record, uid: String(record.uid ?? uid) }
    const id = 'id' in next ? String(next.id) : JSON.stringify(next)
    const key = `${next.uid}:${id}`
    if (seen.has(key)) return []
    seen.add(key)
    return [next]
  }))
}
