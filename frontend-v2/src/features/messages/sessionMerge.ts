// @ts-nocheck
import { splitSessionSections } from './sessionLayout'
import {
  normalizeSessionTimestamp,
  pickSessionField,
  resolveSessionActivityTs,
} from './sessionState'

function mergeOneSession(apiSession = {}, existingSession = {}) {
  const apiTs = normalizeSessionTimestamp(apiSession.ts)
  const existingTs = normalizeSessionTimestamp(existingSession.ts)
  const apiActivityTs = resolveSessionActivityTs(apiSession)
  const existingActivityTs = resolveSessionActivityTs(existingSession)
  const keepExistingPreview = existingActivityTs >= apiActivityTs

  return {
    ...apiSession,
    sortIndex: Math.max(apiActivityTs, existingActivityTs),
    ts: Math.max(apiTs, existingTs, apiActivityTs, existingActivityTs),
    // Trust the API when its activity is at least as fresh as what we have
    // locally — it's authoritative for read/unread state from the server.
    // The old logic `apiActivityTs > existingActivityTs` (strict) fell into a
    // `Math.max(api.unread, existing.unread)` branch whenever timestamps tied,
    // which preserved phantom unreads forever (WS replay bumped existing to
    // 1, API refresh returned 0 with same ts, max(0,1)=1 stuck). Only keep
    // existing's higher unread when existing has STRICTLY NEWER activity —
    // that means a WS push arrived after the API call and the API just
    // doesn't know yet.
    unread: existingActivityTs > apiActivityTs
      ? Math.max(Number(apiSession.unread || 0), Number(existingSession.unread || 0))
      : Number(apiSession.unread || 0),
    summary: keepExistingPreview
      ? pickSessionField(existingSession.summary, apiSession.summary, 'summary')
      : pickSessionField(apiSession.summary, existingSession.summary, 'summary'),
    peerUid: pickSessionField(apiSession.peerUid, existingSession.peerUid, 'peerUid'),
    peerNick: pickSessionField(apiSession.peerNick, existingSession.peerNick, 'peerNick'),
    peerLogo: pickSessionField(apiSession.peerLogo, existingSession.peerLogo, 'peerLogo'),
    itemId: pickSessionField(apiSession.itemId, existingSession.itemId, 'itemId'),
    itemTitle: pickSessionField(apiSession.itemTitle, existingSession.itemTitle, 'itemTitle'),
    itemPic: pickSessionField(apiSession.itemPic, existingSession.itemPic, 'itemPic'),
  }
}

export function mergeSessionLists(apiSessions = [], existingSessions = []) {
  const existingBySessionId = new Map(
    (Array.isArray(existingSessions) ? existingSessions : []).map((session) => [session.sessionId, session]),
  )
  const apiSessionIds = new Set((Array.isArray(apiSessions) ? apiSessions : []).map((session) => session.sessionId))

  const mergedApiSessions = (Array.isArray(apiSessions) ? apiSessions : []).map((apiSession) => {
    const existingSession = existingBySessionId.get(apiSession.sessionId)
    return existingSession ? mergeOneSession(apiSession, existingSession) : apiSession
  })

  const liveOnlySessions = (Array.isArray(existingSessions) ? existingSessions : []).filter(
    (session) => !apiSessionIds.has(session.sessionId),
  )

  return splitSessionSections([...mergedApiSessions, ...liveOnlySessions]).orderedSessions
}
