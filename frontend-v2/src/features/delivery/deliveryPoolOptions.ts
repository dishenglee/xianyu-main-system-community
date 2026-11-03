export type DeliveryPoolOption = {
  tag: string
  oneTimeEnabled: boolean
  totalCount: number
  availableCount: number
}

type CardLike = {
  cardPoolTag?: unknown
  card_pool_tag?: unknown
  reusable?: unknown
  used?: unknown
}

function readBoolean(value: unknown) {
  if (typeof value === 'boolean') return value
  if (typeof value === 'number') return value !== 0
  return ['1', 'true', 'yes', 'on'].includes(String(value ?? '').trim().toLowerCase())
}

export function buildDeliveryPoolOptions(cards: CardLike[]) {
  const groups = new Map<string, DeliveryPoolOption>()
  for (const card of cards) {
    const tag = String(card.cardPoolTag ?? card.card_pool_tag ?? '').trim()
    if (!tag) continue
    const oneTimeEnabled = !readBoolean(card.reusable)
    const key = `${tag}\u0000${oneTimeEnabled ? 'one-time' : 'fixed'}`
    const option = groups.get(key) ?? { tag, oneTimeEnabled, totalCount: 0, availableCount: 0 }
    option.totalCount += 1
    if (!oneTimeEnabled || !readBoolean(card.used)) option.availableCount += 1
    groups.set(key, option)
  }
  return Array.from(groups.values())
}

export function findDeliveryPoolOption(options: DeliveryPoolOption[], tag: string, oneTimeEnabled: boolean) {
  return options.find((option) => option.tag === tag.trim() && option.oneTimeEnabled === oneTimeEnabled)
}

export function selectAvailableDeliveryPoolOptions(options: DeliveryPoolOption[], oneTimeEnabled: boolean) {
  return options.filter((option) => option.oneTimeEnabled === oneTimeEnabled && option.availableCount > 0)
}

export function sanitizeSelectedOneTimePoolTags(tags: string[], options: DeliveryPoolOption[]) {
  const availableTags = new Set(selectAvailableDeliveryPoolOptions(options, true).map((option) => option.tag))
  return Array.from(new Set(tags.map((tag) => String(tag).trim()).filter((tag) => tag && availableTags.has(tag))))
}

export function sanitizeSelectedFixedPoolTags(tags: string[], options: DeliveryPoolOption[]) {
  const fixedTags = new Set(options.filter((option) => !option.oneTimeEnabled && option.totalCount > 0).map((option) => option.tag))
  return Array.from(new Set(tags.map((tag) => String(tag).trim()).filter((tag) => tag && fixedTags.has(tag))))
}

export function normalizeExclusiveDeliveryPoolTags(
  poolTags: string[],
  fixedPoolTags: string[],
  options: DeliveryPoolOption[],
  oneTimeEnabled?: boolean,
) {
  const validOneTime = sanitizeSelectedOneTimePoolTags(poolTags, options)
  const validFixed = sanitizeSelectedFixedPoolTags(fixedPoolTags, options)
  if (oneTimeEnabled === true && validOneTime.length) return { poolTags: validOneTime, fixedPoolTags: [] }
  if (oneTimeEnabled === false && validFixed.length) return { poolTags: [], fixedPoolTags: validFixed }
  if (validOneTime.length) return { poolTags: validOneTime, fixedPoolTags: [] }
  return { poolTags: [], fixedPoolTags: validFixed }
}
