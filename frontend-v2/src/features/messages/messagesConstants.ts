// @ts-nocheck
import { normalizeSessionField, normalizeSessionTimestamp } from './sessionState'
import { sanitizeMessageHtml } from './sanitizeHtml'

export const TYPE_FILTERS = [
  { key: 'chat', label: '商品会话', icon: null as any },
  { key: 'system', label: '通知消息', icon: null as any },
  { key: 'favorite', label: '互动消息', icon: null as any },
]

// Apple-minimal palette — single accent (system blue), no brand yellow/orange.
export const BRAND = {
  primary: 'var(--primary)',
  accent: 'var(--danger)',
  gradient: 'none',
  page: 'var(--bg-page)',
  card: 'var(--surface-card)',
  textMain: 'var(--text-main)',
  textSecondary: 'var(--text-secondary)',
  textTertiary: 'var(--text-muted)',
  border: 'var(--control-border)',
  selected: 'var(--control-active)',
  hover: 'rgba(255,255,255,0.54)',
}

export const PANEL_STYLE = {
  backgroundColor: BRAND.card,
  borderColor: BRAND.border,
  boxShadow: 'var(--glass-shadow-soft)',
}

export const STICKY_GROUP_STYLE = {
  backgroundColor: 'rgba(246,250,254,0.82)',
  borderColor: BRAND.border,
  color: BRAND.textTertiary,
  backdropFilter: 'saturate(180%) blur(20px)',
  WebkitBackdropFilter: 'saturate(180%) blur(20px)',
}

export function withAlpha(color, alpha = '18') {
  return typeof color === 'string' && color.startsWith('#') && color.length === 7
    ? `${color}${alpha}`
    : color
}

export function sessionType(session) {
  return Number(session?.sessionType ?? session?.type ?? 1)
}

export function sessionKey(session) {
  return `${session?._uid || ''}:${session?.sessionId || ''}:${sessionType(session)}`
}

export function avatarFallback(label = '') {
  return String(label || '?').trim().charAt(0).toUpperCase() || '?'
}

export function peerLabel(session) {
  // Notification / interaction sessions come from the platform's fixed
  // virtual users (userId=100 type=10 / userId=200 type=10). The raw nick
  // varies by sub-channel — sessionType=3 carries either "系统消息" or
  // "通知消息", sessionType=6 carries either "互动消息" or "留言". The
  // 闲鱼 client UI collapses both sub-channels into a single label, so we
  // match that and ignore the per-row peerNick.
  const st = sessionType(session)
  if (st === 3) return '通知消息'
  if (st === 6) return '互动消息'
  const peerNick = normalizeSessionField(session?.peerNick, 'peerNick')
  if (peerNick) return peerNick
  return session?.peerUid || '未知'
}

export function accountLabel(session) {
  return session?._accountNick || session?._uid || '未知账号'
}

export function summaryText(session) {
  const raw = sanitizeMessageHtml(session?.summary || '').text.trim()
  if (raw) return raw
  if (sessionType(session) === 3) return '通知消息'
  if (sessionType(session) === 6) return '互动消息'
  return '暂无消息'
}

// Session list — today: HH:MM · this year: M/D · older: YYYY/M/D.
export function formatSessionTime(ts) {
  const timestamp = normalizeSessionTimestamp(ts)
  if (!timestamp) return ''
  const target = new Date(timestamp)
  const now = new Date()
  if (target.toDateString() === now.toDateString()) {
    return target.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  }
  if (target.getFullYear() !== now.getFullYear()) {
    return target.toLocaleDateString('zh-CN', { year: 'numeric', month: 'numeric', day: 'numeric' })
  }
  return target.toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' })
}

// Chat bubble timestamps — today: HH:MM · yesterday: 昨天 HH:MM · this year:
// M/D HH:MM · older: YYYY/M/D HH:MM. Python-era UI collapsed everything to
// HH:MM which made it impossible to tell today from a week ago.
export function formatMessageTime(ts) {
  const timestamp = normalizeSessionTimestamp(ts)
  if (!timestamp) return ''
  const target = new Date(timestamp)
  const now = new Date()
  const hm = target.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  if (target.toDateString() === now.toDateString()) return hm
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  if (target.toDateString() === yesterday.toDateString()) return `昨天 ${hm}`
  if (target.getFullYear() !== now.getFullYear()) {
    const d = target.toLocaleDateString('zh-CN', { year: 'numeric', month: 'numeric', day: 'numeric' })
    return `${d} ${hm}`
  }
  const d = target.toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' })
  return `${d} ${hm}`
}

export function resolveSessionItem(session) {
  if (sessionType(session) === 3 || sessionType(session) === 6) return null

  const itemId = String(session?.itemId ?? session?.item?.id ?? '').trim()
  const pic = session?.itemPicUrl ?? session?.itemPic ?? session?.item?.pic ?? ''
  const title = session?.itemTitle ?? session?.item?.title ?? ''
  // price isn't on the session record — resolved asynchronously via itemDetail
  const price = session?.itemPrice ?? session?.item?.price ?? ''

  if (!itemId && !pic && !title && !price) return null

  return {
    itemId,
    pic,
    title,
    price,
  }
}

export function resolveMessageImage(message) {
  return String(message?.imageUrl ?? message?.pic ?? '').trim()
}

// All messages go through sanitizeMessageHtml — the platform embeds
// `<font color='...'>查看详情</font>`, `<strong>...</strong>`, etc. in
// `tip` / `card` kinds too. The sanitizer short-circuits to plain text when
// no `<tag>` is present, so this is zero cost for normal chat.

export function resolveMessageIsMine(message, uid) {
  const senderId = String(
    message?.senderId
      ?? message?.sender_id
      ?? message?.senderUid
      ?? message?.sender_uid
      ?? '',
  ).trim()

  return Boolean(
    message?.isSelf
      || message?.isMe
      || message?.isMine
      || message?.mine
      || (senderId && senderId === uid),
  )
}
