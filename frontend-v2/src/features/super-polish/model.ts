export type JsonRecord = Record<string, unknown>

export function asRecord(value: unknown): JsonRecord {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as JsonRecord
  }
  return {}
}

export function asArray<T = unknown>(value: unknown): T[] {
  return Array.isArray(value) ? value as T[] : []
}

export function readPath(value: unknown, path: string[]): unknown {
  let current: unknown = value
  for (const key of path) {
    if (Array.isArray(current)) {
      const index = Number(key)
      if (!Number.isInteger(index)) return undefined
      current = current[index]
      continue
    }
    if (!current || typeof current !== 'object') return undefined
    current = (current as JsonRecord)[key]
  }
  return current
}

export function extractRecords(value: unknown, paths: string[][]): JsonRecord[] {
  for (const path of paths) {
    const found = readPath(value, path)
    if (Array.isArray(found)) {
      return found.map((item) => asRecord(item))
    }
  }
  return []
}

export function pickString(source: unknown, paths: string[][], fallback = '—') {
  for (const path of paths) {
    const value = readPath(source, path)
    if (value === undefined || value === null || value === '') continue
    return String(value)
  }
  return fallback
}

export function pickNumberString(source: unknown, paths: string[][], fallback = '—') {
  for (const path of paths) {
    const value = readPath(source, path)
    if (value === undefined || value === null || value === '') continue
    const numeric = Number(value)
    if (Number.isFinite(numeric)) {
      return Number.isInteger(numeric)
        ? String(numeric)
        : numeric.toLocaleString('zh-CN', { maximumFractionDigits: 2 })
    }
    return String(value)
  }
  return fallback
}

function formatYuan(value: number) {
  return value.toLocaleString('zh-CN', {
    minimumFractionDigits: Number.isInteger(value) ? 0 : 1,
    maximumFractionDigits: 2,
  })
}

export function formatMoney(value: unknown) {
  const numeric = Number(value)
  if (!Number.isFinite(numeric)) return '¥0'
  return `¥${formatYuan(numeric / 100)}`
}

export function formatJson(value: unknown) {
  if (value === undefined) return ''
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

export function parseJsonDraft(input: string): unknown {
  const trimmed = input.trim()
  if (!trimmed) return {}
  return JSON.parse(trimmed)
}

export function todayInput() {
  return new Date().toISOString().slice(0, 10)
}

export function daysAgoInput(days: number) {
  const date = new Date()
  date.setDate(date.getDate() - days)
  return date.toISOString().slice(0, 10)
}

export function compactDate(value: string) {
  return value.replaceAll('-', '')
}

export function toEntityList(items: JsonRecord[]) {
  return items.map((item) => ({
    entityId: Number(pickString(item, [['itemId']], '0')),
    catId: Number(pickString(item, [['catId']], '0')),
    price: Number(pickString(item, [['price']], '0')),
    areaTarget: false,
    startTime: Date.now(),
  }))
}

export function campaignListFromResponse(value: unknown) {
  return extractRecords(value, [['result']])
}

export function groupListFromResponse(value: unknown) {
  return extractRecords(value, [['result']])
}

export function itemListFromSearch(value: unknown) {
  return extractRecords(value, [['result', 'itemList'], ['result']])
}

export function reportRowsFromResponse(value: unknown) {
  return extractRecords(value, [['result'], ['dataDetailVOList'], ['data', 'dataDetailVOList']])
}
