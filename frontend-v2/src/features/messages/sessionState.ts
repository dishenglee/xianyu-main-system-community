// @ts-nocheck
const PLACEHOLDER_TEXT = new Set([
  '你有一条新消息',
  '您有一条新消息',
  '未知',
  '未知会话',
  '未知账号',
  '未知客户',
  '未知商品',
  '闲鱼商品会话',
  '暂无消息',
])

function toFiniteNumber(value) {
  const number = Number(value || 0)
  return Number.isFinite(number) && number > 0 ? number : 0
}

function parseCompactDatetime(value) {
  const text = String(value || '').trim()
  if (!/^\d{14}$/.test(text)) return 0

  const year = Number(text.slice(0, 4))
  const month = Number(text.slice(4, 6))
  const day = Number(text.slice(6, 8))
  const hour = Number(text.slice(8, 10))
  const minute = Number(text.slice(10, 12))
  const second = Number(text.slice(12, 14))
  const parsed = new Date(year, month - 1, day, hour, minute, second).getTime()
  return Number.isFinite(parsed) ? parsed : 0
}

export function normalizeSessionTimestamp(value) {
  const text = String(value || '').trim()
  if (!text) return 0

  const compactDatetime = parseCompactDatetime(text)
  if (compactDatetime) return compactDatetime

  const number = toFiniteNumber(text)
  if (!number) return 0
  if (number >= 1e9 && number < 1e12) return number * 1000
  return number
}

export function resolveSessionActivityTs(session = {}) {
  return Math.max(
    normalizeSessionTimestamp(session?.ts),
    normalizeSessionTimestamp(session?.sortIndex),
  )
}

export function normalizeSessionField(value, field = 'text') {
  const text = String(value || '').trim()
  if (!text) return ''
  if (PLACEHOLDER_TEXT.has(text)) return ''
  if (field === 'peerNick') {
    if (/^(用户_|tbNick_)/i.test(text)) return ''
    if (/^\d+$/.test(text)) return ''
  }
  if (field === 'itemId' && /^auto_\d+$/i.test(text)) return ''
  if (field === 'peerUid' && text.includes('*')) return ''
  return text
}

export function hasSessionField(value, field = 'text') {
  return Boolean(normalizeSessionField(value, field))
}

export function pickSessionField(preferred, fallback, field = 'text') {
  const preferredValue = normalizeSessionField(preferred, field)
  if (preferredValue) return preferredValue
  return normalizeSessionField(fallback, field)
}
