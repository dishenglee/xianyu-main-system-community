// @ts-nocheck
export function summarizeCardPools(cards = []) {
  const pools = new Map()

  for (const card of cards) {
    const tag = String(card?.cardPoolTag || '').trim()
    if (!tag) continue

    const current = pools.get(tag) || { tag, total: 0, unused: 0 }
    current.total += 1
    if (!card?.used) {
      current.unused += 1
    }
    pools.set(tag, current)
  }

  return [...pools.values()]
    .sort((left, right) => left.tag.localeCompare(right.tag, 'zh-Hans-CN'))
    .map((summary) => ({
      ...summary,
      label: `${summary.tag}（可用 ${summary.unused} / 总数 ${summary.total}）`,
    }))
}
