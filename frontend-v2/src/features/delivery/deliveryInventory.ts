type CardLike = { cardPoolTag?: string | null; card_pool_tag?: string | null }
type ConfigurationLike = { poolTag?: string | null; pool_tag?: string | null; poolTags?: string[] | null; pool_tags?: string[] | null }

export function cardsForDeliveryConfiguration<T extends CardLike>(
  cards: T[],
  configuration: ConfigurationLike,
) {
  const rawTags = configuration.poolTags ?? configuration.pool_tags
  const poolTags = new Set(Array.isArray(rawTags) ? rawTags : [configuration.poolTag ?? configuration.pool_tag ?? ''])
  poolTags.delete('')
  if (!poolTags.size) return []
  return cards.filter((card) => poolTags.has(String(card.cardPoolTag ?? card.card_pool_tag ?? '').trim()))
}
