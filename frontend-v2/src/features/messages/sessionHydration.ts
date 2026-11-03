// @ts-nocheck
import { normalizeSessionField, pickSessionField } from './sessionState'

export function needsSessionHydration(session = {}) {
  if (!session?.sessionId) return false
  const sessionType = Number(session?.sessionType || 1)
  if (sessionType !== 1) {
    return !normalizeSessionField(session.peerNick, 'peerNick')
  }
  return !(
    normalizeSessionField(session.peerNick, 'peerNick')
    && normalizeSessionField(session.peerRole)
    && normalizeSessionField(session.selfRole)
    && normalizeSessionField(session.itemId, 'itemId')
    && normalizeSessionField(session.itemTitle)
    && normalizeSessionField(session.itemPic)
  )
}

export function buildSessionIdentityPatch(source = {}, fallback = {}) {
  return {
    sessionId: String(source.sessionId || fallback.sessionId || '').trim(),
    sessionType: Number(source.sessionType || fallback.sessionType || 1),
    peerUid: pickSessionField(source.peerUid, fallback.peerUid, 'peerUid'),
    peerNick: pickSessionField(source.peerNick, fallback.peerNick, 'peerNick'),
    peerLogo: pickSessionField(source.peerLogo, fallback.peerLogo, 'peerLogo'),
    itemId: pickSessionField(source.itemId, fallback.itemId, 'itemId'),
    itemTitle: pickSessionField(source.itemTitle, fallback.itemTitle, 'itemTitle'),
    itemPic: pickSessionField(source.itemPic, fallback.itemPic, 'itemPic'),
    peerRole: pickSessionField(source.peerRole, fallback.peerRole, 'peerRole'),
    selfRole: pickSessionField(source.selfRole, fallback.selfRole, 'selfRole'),
    summary: pickSessionField(source.summary, fallback.summary, 'summary'),
  }
}

export function collectAccountsNeedingRemoteSync(accounts = [], sessionsByUid = {}) {
  return accounts
    .map((account) => String(account?.uid || '').trim())
    .filter(Boolean)
    .filter((uid) => (sessionsByUid[uid] || []).some((session) => needsSessionHydration(session)))
}
