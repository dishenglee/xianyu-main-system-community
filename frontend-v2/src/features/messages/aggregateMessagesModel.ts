import { normalizeSessionTimestamp } from './sessionState'

export type SessionTypeFilterKey = 'chat' | 'system' | 'favorite'

export interface SessionTypeFilters {
  chat: boolean
  system: boolean
  favorite: boolean
}

export interface AggregateSessionLike {
  sessionId?: string
  sessionType?: number
  ts?: number | string
}

export interface SessionBucket<TSession extends AggregateSessionLike> {
  label: '今天' | '昨天' | '本周' | '更早'
  sessions: TSession[]
}

const BUCKET_ORDER: SessionBucket<AggregateSessionLike>['label'][] = ['今天', '昨天', '本周', '更早']

export function isPlatformNotificationSession(session: AggregateSessionLike): boolean {
  return [3, 6, 53].includes(Number(session?.sessionType))
}

function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
}

function startOfWeek(date: Date): number {
  const day = date.getDay()
  const offset = day === 0 ? 6 : day - 1
  return startOfDay(new Date(date.getTime() - offset * 24 * 60 * 60 * 1000))
}

export function resolveSessionTypeFilterKey(sessionType?: number): SessionTypeFilterKey {
  // 53 = 工作台通知 (seller workbench): grouped with 通知消息 under the same
  // "system" filter — both are platform-broadcast channels rather than chats.
  if (Number(sessionType) === 3 || Number(sessionType) === 53) return 'system'
  if (Number(sessionType) === 6) return 'favorite'
  return 'chat'
}

export function isSessionTypeEnabled(
  session: AggregateSessionLike,
  filters: SessionTypeFilters,
): boolean {
  return filters[resolveSessionTypeFilterKey(session.sessionType)]
}

export function resolveBucketLabel(
  ts: number | string | undefined,
  now = new Date(),
): SessionBucket<AggregateSessionLike>['label'] {
  const timestamp = normalizeSessionTimestamp(ts)
  if (!timestamp) return '更早'

  const todayStart = startOfDay(now)
  if (timestamp >= todayStart) return '今天'
  if (timestamp >= todayStart - 24 * 60 * 60 * 1000) return '昨天'
  if (timestamp >= startOfWeek(now)) return '本周'
  return '更早'
}

export function groupSessionsByBucket<TSession extends AggregateSessionLike>(
  sessions: TSession[],
  now = new Date(),
): SessionBucket<TSession>[] {
  const grouped = new Map<SessionBucket<TSession>['label'], TSession[]>()
  for (const label of BUCKET_ORDER) grouped.set(label, [])

  const sortedSessions = [...sessions].sort(
    (left, right) => normalizeSessionTimestamp(right.ts) - normalizeSessionTimestamp(left.ts),
  )

  for (const session of sortedSessions) {
    grouped.get(resolveBucketLabel(session.ts, now))?.push(session)
  }

  return BUCKET_ORDER
    .map((label) => ({ label, sessions: grouped.get(label) ?? [] }))
    .filter((group) => group.sessions.length > 0)
}
