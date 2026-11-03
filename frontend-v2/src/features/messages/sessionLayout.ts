// @ts-nocheck
import { resolveSessionActivityTs } from './sessionState'

const SESSION_TYPE_META = {
  // sessionType=53 ("工作台通知") is the seller workbench notification channel.
  // 闲鱼 fans the same workbench notification out across 3 platform sender
  // accounts (2222215948242 / 2222216136891 / 2222229870221), so each uid
  // ends up with 3 distinct sessionIds for what the phone UI collapses into
  // a single "工作台通知" entry. We mirror that: classify it as notification
  // and dedupe across the sub-sessions in `splitSessionSections` below.
  53: { label: '工作台通知', section: 'notification', priority: 0, showUnreadBadge: false },
  6: { label: '互动消息', section: 'notification', priority: 0, showUnreadBadge: true },
  3: { label: '通知消息', section: 'notification', priority: 1, showUnreadBadge: false },
  1: { label: '商品会话', section: 'chat', priority: 2, showUnreadBadge: true },
}

export function getSessionMeta(sessionType) {
  return SESSION_TYPE_META[Number(sessionType)] || {
    label: '消息',
    section: 'chat',
    priority: 9,
  }
}

function sortWithinSection(left, right) {
  return resolveSessionActivityTs(right) - resolveSessionActivityTs(left)
}

// 闲鱼's seller-workbench notifications (sessionType=53, peerNick="工作台通知")
// arrive from 3 different platform sender sub-accounts but represent the
// SAME notification stream. The phone app shows one entry per uid; we have
// 3 sessionIds and would otherwise show 3 rows labeled "工作台通知" each.
// Collapse them by `_uid` (or sessionId when uid is missing) and keep the
// most recently active sub-session — matches phone behavior, and a uid
// whose 3 sub-sessions carry truly distinct latest messages still surfaces
// the newest one via the kept entry's summary.
export function dedupeWorkbenchSessions(sessions) {
  const seen = new Set()
  const out = []
  for (const session of sessions) {
    if (Number(session?.sessionType) !== 53) {
      out.push(session)
      continue
    }
    const key = `${session?._uid || ''}:53`
    if (seen.has(key)) continue
    seen.add(key)
    out.push(session)
  }
  return out
}

export function splitSessionSections(sessions = []) {
  const notificationSessions = []
  const chatSessions = []

  for (const session of sessions) {
    const meta = getSessionMeta(session?.sessionType)
    const target = meta.section === 'notification' ? notificationSessions : chatSessions
    target.push(session)
  }

  notificationSessions.sort((left, right) => {
    const priorityDiff = getSessionMeta(left?.sessionType).priority - getSessionMeta(right?.sessionType).priority
    if (priorityDiff !== 0) return priorityDiff
    return sortWithinSection(left, right)
  })

  chatSessions.sort(sortWithinSection)

  // After sorting by recency, the FIRST sessionType=53 entry per uid is the
  // most recent — that's the one we want to keep. Dedup pass drops the rest.
  const dedupedNotifications = dedupeWorkbenchSessions(notificationSessions)

  return {
    notificationSessions: dedupedNotifications,
    chatSessions,
    orderedSessions: [...dedupedNotifications, ...chatSessions],
  }
}

export function pickDefaultSession(sessions = [], currentSessionId = '') {
  const { orderedSessions } = splitSessionSections(sessions)
  if (!orderedSessions.length) return null

  if (currentSessionId) {
    const current = orderedSessions.find((session) => session.sessionId === currentSessionId)
    if (current) return current
  }

  return orderedSessions[0]
}

export function canReplyToSession(session) {
  return Number(session?.sessionType || 0) === 1
}

export function shouldShowUnreadBadge(session) {
  return getSessionMeta(session?.sessionType).showUnreadBadge !== false
}
