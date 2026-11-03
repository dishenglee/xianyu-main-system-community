export interface GlobalMessageSnapshotEntry {
  key: string
  uid: string
  sessionId: string
  sessionType: number
  unread: number
  version: number
  ts: number
  peerNick: string
  preview: string
}

function toPositiveNumber(value: unknown) {
  const number = Number(value || 0)
  return Number.isFinite(number) && number > 0 ? number : 0
}

function toSessionId(value: unknown) {
  return String(value || '').trim()
}

function pickPreview(session: Record<string, any>) {
  return String(
    session?.summary
      || session?.lastMessage
      || session?.last_message
      || session?.title
      || '',
  ).trim()
}

function pickPeerNick(session: Record<string, any>) {
  return String(
    session?.peerNick
      || session?.peer_nick
      || session?.title
      || '',
  ).trim()
}

function entryKey(uid: string, sessionId: string, sessionType: number) {
  return `${uid}:${sessionId}:${sessionType}`
}

function normalizeSnapshotEntry(uid: string, session: Record<string, any>): GlobalMessageSnapshotEntry | null {
  const sessionId = toSessionId(session?.sessionId ?? session?.session_id)
  if (!uid || !sessionId) return null

  const sessionType = Number(session?.sessionType ?? session?.session_type ?? 1) || 1
  if (sessionType !== 1) return null

  const version = toPositiveNumber(session?.version)
  const ts = toPositiveNumber(session?.ts ?? session?.updated_at ?? session?.updatedAt) || Date.now()
  return {
    key: entryKey(uid, sessionId, sessionType),
    uid,
    sessionId,
    sessionType,
    unread: Math.max(0, Number(session?.unread ?? session?.unread_count ?? 0) || 0),
    version,
    ts,
    peerNick: pickPeerNick(session),
    preview: pickPreview(session),
  }
}

function keyBelongsToUid(key: string, uid: string) {
  return key.startsWith(`${uid}:`)
}

export function reconcilePolledSessions(
  previous: Record<string, GlobalMessageSnapshotEntry>,
  uid: string,
  sessions: Record<string, any>[],
  options: { notifyOnNewKeys?: boolean } = {},
) {
  const notifyOnNewKeys = Boolean(options.notifyOnNewKeys)
  const next: Record<string, GlobalMessageSnapshotEntry> = { ...previous }
  const notifications: GlobalMessageSnapshotEntry[] = []

  for (const key of Object.keys(next)) {
    if (keyBelongsToUid(key, uid)) delete next[key]
  }

  for (const rawSession of Array.isArray(sessions) ? sessions : []) {
    const entry = normalizeSnapshotEntry(uid, rawSession)
    if (!entry) continue
    const previousEntry = previous[entry.key]
    next[entry.key] = entry

    if (entry.unread <= 0) continue
    if (!previousEntry) {
      if (notifyOnNewKeys) notifications.push(entry)
      continue
    }

    const versionAdvanced = entry.version > previousEntry.version
    const unreadIncreased = entry.unread > previousEntry.unread
    if (versionAdvanced || unreadIncreased) {
      notifications.push(entry)
    }
  }

  return { snapshot: next, notifications }
}

export function applyLiveEventSnapshot(
  previous: Record<string, GlobalMessageSnapshotEntry>,
  event: Record<string, any>,
) {
  const uid = String(event?.uid || '').trim()
  const sessionId = toSessionId(event?.sessionId ?? event?.session_id)
  const sessionType = Number(event?.sessionType ?? event?.session_type ?? 1) || 1
  if (!uid || !sessionId || sessionType !== 1) return previous

  const key = entryKey(uid, sessionId, sessionType)
  const previousEntry = previous[key]
  const nextUnread = Math.max(
    0,
    Number(
      event?.unreadCount
      ?? event?.unread_count
      ?? (event?.isSelf ? previousEntry?.unread ?? 0 : (previousEntry?.unread ?? 0) + 1),
    ) || 0,
  )
  const nextEntry: GlobalMessageSnapshotEntry = {
    key,
    uid,
    sessionId,
    sessionType,
    unread: nextUnread,
    version: Math.max(previousEntry?.version || 0, toPositiveNumber(event?.version) || toPositiveNumber(event?.ts)),
    ts: toPositiveNumber(event?.ts) || Date.now(),
    peerNick: String(event?.peerNick || event?.senderNick || previousEntry?.peerNick || '').trim(),
    preview: String(event?.rawPreview || event?.text || previousEntry?.preview || '').trim(),
  }

  return {
    ...previous,
    [key]: nextEntry,
  }
}

export function pruneSnapshotToUids(
  previous: Record<string, GlobalMessageSnapshotEntry>,
  activeUids: string[],
) {
  const allowed = new Set((Array.isArray(activeUids) ? activeUids : []).map((uid) => String(uid).trim()).filter(Boolean))
  const next: Record<string, GlobalMessageSnapshotEntry> = {}

  for (const [key, value] of Object.entries(previous || {})) {
    if (allowed.has(value.uid)) {
      next[key] = value
      continue
    }
    const firstColon = key.indexOf(':')
    const uid = firstColon >= 0 ? key.slice(0, firstColon) : ''
    if (allowed.has(uid)) next[key] = value
  }

  return next
}
