// @ts-nocheck
import { canReplyToSession, getSessionMeta } from './sessionLayout.js'
import { normalizeSessionField, normalizeSessionTimestamp } from './sessionState.js'

function meaningful(value, field = 'text') {
  return normalizeSessionField(value, field)
}

function resolveAnonymousPeerLabel(session = {}) {
  const peerUid = meaningful(session.peerUid, 'peerUid')
  if (!peerUid) return '新会话'

  const suffix = peerUid.slice(-4)
  const peerRole = meaningful(session.peerRole)
  if (peerRole === 'seller') return `卖家 ${suffix}`
  if (peerRole === 'buyer') return `买家 ${suffix}`
  return `用户 ${suffix}`
}

export function resolveSessionListRow(session = {}) {
  const meta = getSessionMeta(session?.sessionType)
  const notificationTitle = Number(session?.sessionType) === 6 ? '互动消息' : meta.label
  const peerNick = meaningful(session.peerNick, 'peerNick')
  const title = meta.section === 'notification'
    ? notificationTitle
    : peerNick || resolveAnonymousPeerLabel(session)
  const preview = meaningful(session.summary) || '暂无历史消息'
  const itemTitle = meaningful(session.itemTitle)
  const itemPic = meaningful(session.itemPic)

  return {
    title,
    preview,
    unread: Number(session?.unread || 0),
    ts: normalizeSessionTimestamp(session?.ts),
    item: (itemTitle || itemPic)
      ? {
          title: itemTitle || '关联商品',
          pic: itemPic,
        }
      : null,
  }
}

export function resolveSessionDetailHeader(session = {}) {
  const meta = getSessionMeta(session?.sessionType)
  const isNotification = meta.section === 'notification'
  const notificationTitle = Number(session?.sessionType) === 6 ? '互动消息' : meta.label
  const title = isNotification
    ? notificationTitle
    : meaningful(session.peerNick, 'peerNick') || resolveAnonymousPeerLabel(session)
  const subtitle = isNotification
    ? ''
    : meaningful(session.sessionId) || ''
  const itemTitle = meaningful(session.itemTitle)
  const itemPic = meaningful(session.itemPic)
  const itemId = meaningful(session.itemId, 'itemId')

  return {
    title,
    subtitle,
    canReply: canReplyToSession(session),
    item: (itemTitle || itemPic || itemId)
      ? {
          title: itemTitle || '',
          pic: itemPic || '',
          itemId: itemId || '',
        }
      : null,
  }
}

export function resolveCardStatus(connection = {}) {
  const status = String(connection?.status || '').trim()
  if (status === 'connected') {
    return { tone: 'connected', label: '同步中' }
  }
  if (status === 'verification_required' || status === 'auth_failed') {
    return { tone: 'warning', label: '待验证' }
  }
  if (status === 'reconnecting' || status === 'failed' || status === 'disconnected') {
    return { tone: 'degraded', label: '正在重新连接' }
  }
  return { tone: 'degraded', label: '正在重新连接' }
}

export function resolveCardNotice(card = {}) {
  const unread = Math.max(0, Number(card?.unread || 0))
  if (unread <= 0) return null
  return {
    tone: 'attention',
    text: '新消息',
    count: unread,
  }
}
