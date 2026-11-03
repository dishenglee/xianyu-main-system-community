type ConfigurationLike = {
  uid?: unknown
  poolTag?: unknown
  pool_tag?: unknown
  poolTags?: unknown
  pool_tags?: unknown
  oneTimeEnabled?: unknown
  one_time_enabled?: unknown
  enabled?: unknown
}

type PoolOptionLike = {
  tag: string
  oneTimeEnabled: boolean
  availableCount: number
}

export type DeliverySharedPoolWarning = {
  poolTag: string
  availableCount: number
  configuredProductCount: number
}

function readBoolean(value: unknown) {
  if (typeof value === 'boolean') return value
  if (typeof value === 'number') return value !== 0
  return ['1', 'true', 'yes', 'on'].includes(String(value ?? '').trim().toLowerCase())
}

export function buildDeliverySharedPoolWarnings(configurations: ConfigurationLike[], poolOptions: PoolOptionLike[]) {
  const configuredCountByTag = new Map<string, number>()
  configurations.forEach((configuration) => {
    const rawTags = configuration.poolTags ?? configuration.pool_tags
    const tags = Array.isArray(rawTags)
      ? Array.from(new Set(rawTags.map((tag) => String(tag).trim()).filter(Boolean)))
      : [String(configuration.poolTag ?? configuration.pool_tag ?? '').trim()].filter(Boolean)
    const oneTimeEnabled = readBoolean(configuration.oneTimeEnabled ?? configuration.one_time_enabled)
    if (!tags.length || !oneTimeEnabled || !readBoolean(configuration.enabled)) return
    tags.forEach((tag) => configuredCountByTag.set(tag, (configuredCountByTag.get(tag) ?? 0) + 1))
  })

  const warnings = new Map<string, DeliverySharedPoolWarning>()
  configuredCountByTag.forEach((configuredProductCount, poolTag) => {
    const option = poolOptions.find((item) => item.tag === poolTag && item.oneTimeEnabled)
    const availableCount = option?.availableCount ?? 0
    if (configuredProductCount > availableCount) warnings.set(poolTag, { poolTag, availableCount, configuredProductCount })
  })
  return warnings
}
