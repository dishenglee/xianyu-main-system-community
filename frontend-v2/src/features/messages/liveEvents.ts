// @ts-nocheck
import { mergeHistoryState } from './historyState'
import { splitSessionSections } from './sessionLayout'
import {
  normalizeSessionField,
  normalizeSessionTimestamp,
  pickSessionField,
  resolveSessionActivityTs,
} from './sessionState'

const MESSAGE_EVENT_TYPES = new Set(['message.received', 'message.sent'])
const CONNECTION_EVENT_TYPES = new Set([
  'connection.connected',
  'connection.disconnected',
  'connection.reconnecting',
  'connection.failed',
  'connection.auth_failed',
  'connection.verification_required',
])

function toPositiveNumber(value) {
  const number = Number(value || 0)
  return Number.isFinite(number) && number > 0 ? number : 0
}

function resolveSummary(event = {}) {
  return String(
    event.rawPreview
      || event.text
      || event.title
      || (event.imageUrl ? '[图片]' : '')
      || '暂无消息',
  ).trim()
}

function pickFirstMeaningful(field, ...values) {
  for (const value of values) {
    const text = normalizeSessionField(value, field)
    if (text) return text
  }
  return ''
}

function resolvePeerIdentity(event = {}, session = {}) {
  const incomingFallbackUid = pickFirstMeaningful('peerUid', event.peerUid, !event.isSelf ? event.senderUid : '')
  const incomingFallbackNick = pickFirstMeaningful('peerNick', event.peerNick, !event.isSelf ? event.senderNick : '')
  const incomingFallbackLogo = pickFirstMeaningful('peerLogo', event.peerLogo, !event.isSelf ? event.senderLogo : '')

  return {
    peerUid: pickSessionField(session.peerUid, incomingFallbackUid, 'peerUid'),
    peerNick: pickSessionField(session.peerNick, incomingFallbackNick, 'peerNick'),
    peerLogo: pickSessionField(session.peerLogo, incomingFallbackLogo, 'peerLogo'),
  }
}

function normalizeIncomingMessage(event = {}) {
  const ts = normalizeSessionTimestamp(event.ts) || Date.now()
  const version = toPositiveNumber(event.version) || ts
  // contentType + cardMeta: forwarded so item-card (7/8) / location-card (30)
  // pushes render as rich cards without waiting for a history refetch. The
  // backend emits them on live_message_event_to_value; null is acceptable
  // (the renderer guards typeof === 'object').
  const contentType = Number(event.contentType ?? event.content_type ?? 0)
  const cardMeta = event.cardMeta && typeof event.cardMeta === 'object' ? event.cardMeta : null
  return {
    messageId: String(event.messageId || '').trim(),
    version,
    ts,
    senderNick: String(event.senderNick || '').trim(),
    senderLogo: String(event.senderLogo || '').trim(),
    isSelf: Boolean(event.isSelf || event.type === 'message.sent'),
    kind: String(event.kind || (event.imageUrl ? 'image' : 'text')).trim(),
    title: String(event.title || '').trim(),
    text: String(event.text || '').trim(),
    imageUrl: String(event.imageUrl || '').trim(),
    preview: resolveSummary(event),
    contentType,
    cardMeta,
  }
}

export function applyLiveEventToSessions(sessions = [], event = {}, options = {}) {
  if (!MESSAGE_EVENT_TYPES.has(event?.type)) {
    return Array.isArray(sessions) ? sessions : []
  }

  const sessionId = String(event.sessionId || '').trim()
  if (!sessionId) {
    return Array.isArray(sessions) ? sessions : []
  }

  const messageTs = normalizeSessionTimestamp(event.ts) || Date.now()
  const eventVersion = toPositiveNumber(event.version) || messageTs
  const baseShouldIncrementUnread = (
    event.type === 'message.received'
    && !event.isSelf
    && !options.markSeen
  )
  const summary = resolveSummary(event)

  let found = false
  const nextSessions = (Array.isArray(sessions) ? sessions : []).map((session) => {
    if (session?.sessionId !== sessionId) return session
    found = true
    const peer = resolvePeerIdentity(event, session)
    // Gate the unread bump on event version vs the session's known
    // latest_version. The backend persists each session's max-seen version
    // in `message_live_state` and surfaces it as `session.version`. WS pushes
    // arriving on reconnect (or upstream re-sync) often carry historical
    // messages whose `event.version <= session.version` — those have already
    // been counted (or read upstream) and must not inflate the unread badge.
    // For sessions without a known version yet (older cache shape) we
    // fall back to the previous behaviour so we don't regress fresh chats.
    const sessionVersion = toPositiveNumber(session?.version)
    const versionAdvances = sessionVersion === 0 || eventVersion > sessionVersion
    const shouldIncrementUnread = baseShouldIncrementUnread && versionAdvances
    const nextVersion = Math.max(sessionVersion, eventVersion)
    return {
      ...session,
      sessionType: Number(event.sessionType || session.sessionType || 1),
      ts: Math.max(resolveSessionActivityTs(session), messageTs),
      sortIndex: Math.max(resolveSessionActivityTs(session), messageTs),
      version: nextVersion,
      unread: shouldIncrementUnread ? Number(session.unread || 0) + 1 : Number(session.unread || 0),
      summary,
      peerUid: peer.peerUid,
      peerNick: peer.peerNick,
      peerLogo: peer.peerLogo,
      itemId: pickSessionField(session.itemId, event.itemId, 'itemId'),
      itemTitle: pickSessionField(session.itemTitle, event.itemTitle, 'itemTitle'),
      itemPic: pickSessionField(session.itemPic, event.itemPic, 'itemPic'),
    }
  })

  if (!found) {
    const peer = resolvePeerIdentity(event)
    // Outgoing-echo events for first-time conversations sometimes lack peer
    // info (backend can't always derive it from the WS frame for is_self
    // pushes without a cached session). Auto-creating a session entry with
    // an empty peerUid produces an unlabeled "未知" row that the next session
    // list refetch will replace. Skip the auto-create — the refetch happens
    // ~15s later via the existing fallback polling, and meanwhile the
    // outgoing message renders correctly inside the optimistic history.
    if (!peer.peerUid && !peer.peerNick) {
      return Array.isArray(sessions) ? sessions : []
    }
    // Brand-new session — there's no prior version to gate against, so the
    // base flag is enough (a true historical re-sync won't manufacture a new
    // session row on the frontend; it would always hit the existing-session
    // branch above).
    nextSessions.push({
      sessionId,
      sessionType: Number(event.sessionType || 1),
      ts: messageTs,
      sortIndex: messageTs,
      version: eventVersion,
      unread: baseShouldIncrementUnread ? 1 : 0,
      summary,
      peerUid: peer.peerUid,
      peerNick: peer.peerNick,
      peerLogo: peer.peerLogo,
      itemId: pickFirstMeaningful('itemId', event.itemId),
      itemTitle: pickFirstMeaningful('itemTitle', event.itemTitle),
      itemPic: pickFirstMeaningful('itemPic', event.itemPic),
    })
  }

  return splitSessionSections(nextSessions).orderedSessions
}

export function applyLiveEventToHistory(history = {}, session = {}, event = {}) {
  if (!MESSAGE_EVENT_TYPES.has(event?.type)) {
    return history || {}
  }

  const sessionId = String(event.sessionId || '').trim()
  if (!sessionId || sessionId !== String(session?.sessionId || history?.sessionId || '').trim()) {
    return history || {}
  }

  const normalizedMessage = normalizeIncomingMessage(event)
  const peer = resolvePeerIdentity(event, session)
  return mergeHistoryState(history || {}, {
    sessionId,
    sessionType: Number(event.sessionType || session?.sessionType || history?.sessionType || 1),
    hasMore: Boolean(history?.hasMore),
    nextCursor: toPositiveNumber(history?.nextCursor),
    latestCursor: Math.max(
      toPositiveNumber(history?.latestCursor),
      normalizedMessage.version,
    ),
    itemId: pickFirstMeaningful('itemId', event.itemId, session?.itemId, history?.itemId),
    itemTitle: pickFirstMeaningful('itemTitle', event.itemTitle, session?.itemTitle, history?.itemTitle),
    itemPic: pickFirstMeaningful('itemPic', event.itemPic, session?.itemPic, history?.itemPic),
    peerUid: pickFirstMeaningful('peerUid', peer.peerUid, history?.peerUid),
    peerNick: pickFirstMeaningful('peerNick', peer.peerNick, history?.peerNick),
    peerLogo: pickFirstMeaningful('peerLogo', peer.peerLogo, history?.peerLogo),
    messages: [normalizedMessage],
  }, 'refresh')
}

export function buildOutgoingMessageEvent({
  uid = '',
  session = {},
  history = {},
  content = '',
  response = {},
} = {}) {
  const payload = response && typeof response === 'object' ? response : {}
  const data = payload.data && typeof payload.data === 'object' ? payload.data : {}
  const ts = normalizeSessionTimestamp(data.ts || payload.ts) || Date.now()
  const sessionId = String(data.sessionId || session.sessionId || '').trim()
  const messageId = String(data.messageId || payload.messageId || '').trim() || `local:${uid}:${sessionId}:${ts}`
  const text = String(content || '').trim()

  return {
    type: 'message.sent',
    uid: String(uid || '').trim(),
    sessionId,
    sessionType: Number(data.sessionType || session.sessionType || 1),
    messageId,
    version: toPositiveNumber(data.version) || ts,
    ts,
    kind: 'text',
    title: '',
    text,
    imageUrl: '',
    rawPreview: text,
    itemId: pickFirstMeaningful('itemId', data.itemId, session.itemId, history.itemId),
    itemTitle: pickFirstMeaningful('itemTitle', data.itemTitle, session.itemTitle, history.itemTitle),
    itemPic: pickFirstMeaningful('itemPic', data.itemPic, session.itemPic, history.itemPic),
    senderNick: '',
    senderLogo: '',
    isSelf: true,
  }
}

export function applyConnectionEvent(connectionStates = {}, event = {}) {
  if (!CONNECTION_EVENT_TYPES.has(event?.type)) {
    return { ...(connectionStates || {}) }
  }

  const uid = String(event.uid || '').trim()
  if (!uid) {
    return { ...(connectionStates || {}) }
  }

  return {
    ...(connectionStates || {}),
    [uid]: {
      status: event.type.replace('connection.', ''),
      reason: String(event.reason || '').trim(),
      snapshot: Boolean(event.snapshot),
      updatedAt: Number(event.updatedAt || 0) || Date.now(),
    },
  }
}
