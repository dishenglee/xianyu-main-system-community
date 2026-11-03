export type CardPoolCard = {
  id?: unknown
  cardPoolTag?: unknown
  reusable?: unknown
  used?: unknown
  cardContent?: unknown
  content?: unknown
  createdAt?: unknown
  created_at?: unknown
}

export type CardPoolGroup<T extends CardPoolCard> = {
  key: string
  tag: string
  cards: T[]
  type: 'one-time' | 'reusable' | 'mixed'
}

export function groupCardPools<T extends CardPoolCard>(cards: T[]): CardPoolGroup<T>[] {
  const groups = new Map<string, { tag: string; cards: T[] }>()
  for (const card of cards) {
    const tag = String(card.cardPoolTag ?? '').trim() || '未分组'
    const group = groups.get(tag) ?? { tag, cards: [] }
    group.cards.push(card)
    groups.set(tag, group)
  }
  return Array.from(groups, ([key, group]) => {
    const reusableCount = group.cards.filter((card) => card.reusable === true).length
    const type: CardPoolGroup<T>['type'] = reusableCount === 0
      ? 'one-time'
      : reusableCount === group.cards.length
        ? 'reusable'
        : 'mixed'
    return { key: `pool:${key}`, tag: group.tag, cards: group.cards, type }
  })
}

export function buildCardGroupUpdatePlan<T extends CardPoolCard>(
  pool: CardPoolGroup<T>,
  draft: { tag: string; contentsById: Record<number, string> },
) {
  const tag = draft.tag.trim()
  if (!tag) throw new Error('请输入标签名称')
  return pool.cards.map((card) => {
    if (typeof card.id !== 'number') throw new Error('卡密缺少可更新标识')
    const cardContent = String(draft.contentsById[card.id] ?? '').trim()
    if (!cardContent) throw new Error('卡密内容不能为空')
    return { id: card.id, payload: { cardPoolTag: tag, cardContent } }
  })
}

export function buildCardGroupDeletePlan<T extends CardPoolCard>(pool: CardPoolGroup<T>) {
  return Array.from(new Set(pool.cards
    .map((card) => card.id)
    .filter((id): id is number => typeof id === 'number')))
}

export function getCardPoolEarliestCreatedAt(cards: CardPoolCard[]) {
  const timestamps = cards.flatMap((card) => {
    const value = Number(card.createdAt ?? card.created_at)
    if (!Number.isFinite(value) || value <= 0) return []
    return [value > 10_000_000_000 ? value : value * 1000]
  })
  return timestamps.length ? Math.min(...timestamps) : null
}
