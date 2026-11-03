// @ts-nocheck
export function collectFocusedSessions({
  activeUids = [],
  viewModeByUid = {},
  selectedSessionIdByUid = {},
  sessionsByUid = {},
} = {}) {
  const activeUidSet = new Set(Array.isArray(activeUids) ? activeUids : [])
  const focused = []

  for (const uid of activeUidSet) {
    if (viewModeByUid[uid] !== 'detail') continue
    const sessionId = String(selectedSessionIdByUid[uid] || '').trim()
    if (!sessionId) continue
    const session = (sessionsByUid[uid] || []).find((item) => item?.sessionId === sessionId)
    focused.push({
      uid,
      sessionId,
      sessionType: Number(session?.sessionType || 1),
    })
  }

  return focused
}

export function syncLiveTransportBindings(transport, {
  activeUids = [],
  focusedSessions = [],
} = {}) {
  if (!transport) return false
  const normalizedUids = (Array.isArray(activeUids) ? activeUids : [])
    .map((uid) => String(uid || '').trim())
    .filter(Boolean)
  const subscribed = transport.subscribe(normalizedUids)
  if (!subscribed) return false

  for (const session of Array.isArray(focusedSessions) ? focusedSessions : []) {
    transport.focus(session.uid, session.sessionId, Number(session.sessionType || 1))
  }
  return true
}

export function blurFocusedSession(transport, { uid = '', sessionId = '' } = {}) {
  if (!transport) return false
  return transport.blur(String(uid || '').trim(), String(sessionId || '').trim())
}
