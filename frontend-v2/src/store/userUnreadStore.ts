import { create } from 'zustand'

// Per-session unread state keyed by `<uid>:<sessionId>` — we only ever store
// real user-to-user sessions (sessionType=1). System notifications (3)
// and interaction messages (6) are excluded so the global "消息" badge only
// reflects buyers/sellers we'd actually reply to.
interface SessionEntry {
  unread: number
  /** Highest message version we've already counted for this session. WS
   *  pushes whose `event.version <= lastVersion` are historical re-sends
   *  (reconnect replay, upstream re-sync) and must not bump unread again. */
  lastVersion: number
}

interface UserUnreadState {
  // Map<`${uid}:${sessionId}`, SessionEntry>. We store an object map rather
  // than a Map<> so React's reference equality / Zustand's shallow comparator
  // both work normally.
  entries: Record<string, SessionEntry>
  /** Session the user is currently viewing — incoming messages auto-clear,
   *  so we skip the live-WS increment instead of letting it bounce up and
   *  back down. Empty string = no session focused. */
  focused: string
  /** Replace the entire snapshot for a uid — used by the initial pcSessions fetch.
   *  This is the canonical re-sync point: any drift between the badge and the
   *  session list gets reconciled here (every session list refresh wipes the
   *  uid's previous entries and re-seeds them from the API response). */
  setSessionsForUid: (uid: string, sessions: any[]) => void
  /** Overwrite a single session's unread count (called when user opens a session, etc.). */
  setUnread: (uid: string, sessionId: string, sessionType: number, unread: number) => void
  /** Bump unread by 1 for an incoming user message (live WS push). Gated on
   *  `version` — if `version <= lastVersion` the event is a historical replay
   *  and we return state unchanged. */
  incrementUnread: (uid: string, sessionId: string, sessionType: number, version?: number) => void
  /** Mark a session as focused (chat view open). */
  setFocused: (uid: string, sessionId: string) => void
  /** Drop entries for accounts that are no longer active. */
  pruneToUids: (activeUids: string[]) => void
  reset: () => void
}

function isUserSession(sessionType: unknown) {
  return Number(sessionType ?? 1) === 1
}

function keyFor(uid: string, sessionId: string) {
  return `${uid}:${sessionId}`
}

function toPositiveNumber(value: unknown): number {
  const n = Number(value || 0)
  return Number.isFinite(n) && n > 0 ? n : 0
}

export const useUserUnreadStore = create<UserUnreadState>((set) => ({
  entries: {},
  focused: '',
  setFocused: (uid, sessionId) => set(() => ({
    focused: uid && sessionId ? keyFor(uid, sessionId) : '',
  })),
  setSessionsForUid: (uid, sessions) => set((state) => {
    if (!uid) return state
    const next = { ...state.entries }
    // Drop any existing entries for this uid before re-seeding — this is what
    // keeps the badge from drifting from the session list. If the API returns
    // unread=0 for a session that previously had a live-WS increment, the
    // stale entry gets removed here.
    for (const k of Object.keys(next)) {
      if (k.startsWith(`${uid}:`)) delete next[k]
    }
    for (const session of sessions || []) {
      if (!isUserSession(session?.sessionType)) continue
      const sid = String(session?.sessionId || '').trim()
      if (!sid) continue
      const unread = Number(session?.unread || 0)
      // Always seed lastVersion even when unread==0 so a subsequent WS replay
      // of an already-known message can't sneak past the version gate.
      const lastVersion = toPositiveNumber(session?.version)
      if (unread > 0 || lastVersion > 0) {
        next[keyFor(uid, sid)] = { unread: Math.max(unread, 0), lastVersion }
      }
    }
    return { entries: next }
  }),
  setUnread: (uid, sessionId, sessionType, unread) => set((state) => {
    if (!uid || !sessionId) return state
    if (!isUserSession(sessionType)) return state
    const k = keyFor(uid, sessionId)
    const next = { ...state.entries }
    const prev = next[k]
    if (unread > 0) {
      next[k] = { unread, lastVersion: prev?.lastVersion || 0 }
    } else if (prev?.lastVersion) {
      // Preserve the version watermark even after clearing — otherwise a
      // historical WS replay arriving right after the user opened the chat
      // could re-bump unread.
      next[k] = { unread: 0, lastVersion: prev.lastVersion }
    } else {
      delete next[k]
    }
    return { entries: next }
  }),
  incrementUnread: (uid, sessionId, sessionType, version) => set((state) => {
    if (!uid || !sessionId) return state
    if (!isUserSession(sessionType)) return state
    const k = keyFor(uid, sessionId)
    // If the user is actively viewing this session, the message is already
    // "seen" — don't inflate the badge.
    if (state.focused === k) return state
    const prev = state.entries[k]
    const eventVersion = toPositiveNumber(version)
    const lastVersion = prev?.lastVersion || 0
    // Version gate: historical re-pushes (reconnect replay, upstream re-sync)
    // arrive with `event.version <= lastVersion`. Drop them so the nav badge
    // doesn't get phantom-incremented for messages that were already counted
    // (or already read upstream).
    if (eventVersion > 0 && lastVersion > 0 && eventVersion <= lastVersion) {
      return state
    }
    const nextVersion = Math.max(lastVersion, eventVersion)
    return {
      entries: {
        ...state.entries,
        [k]: { unread: (prev?.unread || 0) + 1, lastVersion: nextVersion },
      },
    }
  }),
  pruneToUids: (activeUids) => set((state) => {
    const allow = new Set(activeUids)
    const next: Record<string, SessionEntry> = {}
    for (const [k, v] of Object.entries(state.entries)) {
      const uid = k.split(':', 1)[0]
      if (allow.has(uid)) next[k] = v
    }
    return { entries: next }
  }),
  reset: () => set({ entries: {}, focused: '' }),
}))

/** Selector: total unread count across all real user sessions. */
export function selectUserUnreadTotal(state: UserUnreadState) {
  let total = 0
  for (const entry of Object.values(state.entries)) total += entry.unread
  return total
}
