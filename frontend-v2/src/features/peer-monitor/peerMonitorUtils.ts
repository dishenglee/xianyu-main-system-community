export type PeerRecord = Record<string, unknown>
export type PeerValue = string | number | null

export function valuesFromResponse(value: unknown, keys: string[]): unknown[] {
  if (Array.isArray(value)) return value
  if (!value || typeof value !== 'object') return []
  const record = value as PeerRecord
  for (const key of keys) if (Array.isArray(record[key])) return record[key] as unknown[]
  return []
}

export function listFromResponse(value: unknown, keys = ['targets', 'items', 'data', 'list']): PeerRecord[] {
  if (Array.isArray(value)) return value.filter((entry): entry is PeerRecord => Boolean(entry) && typeof entry === 'object')
  if (!value || typeof value !== 'object') return []
  const record = value as PeerRecord
  for (const key of keys) if (Array.isArray(record[key])) return record[key].filter((entry): entry is PeerRecord => Boolean(entry) && typeof entry === 'object')
  return []
}

export function valueOf(value: unknown, ...keys: string[]): PeerValue {
  const record = value && typeof value === 'object' ? value as PeerRecord : null
  for (const key of keys) {
    const current = record?.[key]
    if (typeof current === 'string' || typeof current === 'number') {
      if (current !== '') return current
    }
  }
  if (keys.includes('capturedAt')) {
    const observedAt = record?.observedAt
    return typeof observedAt === 'string' || typeof observedAt === 'number' ? observedAt : null
  }
  return null
}

export function formatTime(value: unknown) {
  if (!value) return '尚未记录'
  const numericValue = Number(value)
  const date = new Date(Number.isFinite(numericValue) && numericValue > 0
    ? (numericValue < 1e12 ? numericValue * 1000 : numericValue)
    : String(value))
  if (Number.isNaN(date.getTime())) return String(value)
  return date.toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

export function statusMeta(status: unknown) {
  if (status === 'active' || status === 'running' || status === 'success') return { label: '已建立基线', tone: 'success' }
  if (status === 'partial') return { label: '部分采集完成', tone: 'warning' }
  if (status === 'paused') return { label: '已暂停', tone: 'neutral' }
  if (status === 'error' || status === 'failed') return { label: '需处理', tone: 'danger' }
  return { label: '等待首次采集', tone: 'neutral' }
}

export function eventMeta(type: unknown) {
  const map: Record<string, { label: string; tone: string }> = {
    baseline: { label: '已建立基线', tone: 'info' },
    new_item: { label: '新上架', tone: 'success' },
    item_listed: { label: '新上架', tone: 'success' },
    removed_item: { label: '已下架', tone: 'neutral' },
    item_off_shelf: { label: '已下架', tone: 'neutral' },
    item_relisted: { label: '重新上架', tone: 'success' },
    price_changed: { label: '价格变化', tone: 'warning' },
    profile_changed: { label: '主页变化', tone: 'info' },
    refresh_failed: { label: '采集异常', tone: 'danger' },
    timeline_listed: { label: '动态上架', tone: 'success' },
    timeline_sold: { label: '动态售出', tone: 'warning' },
    timeline_off_shelf: { label: '动态下架', tone: 'neutral' },
    new_review: { label: '新增评价', tone: 'info' },
  }
  return map[String(type)] ?? { label: '状态变化', tone: 'neutral' }
}
