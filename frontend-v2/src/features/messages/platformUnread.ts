import { isPlatformNotificationSession, type AggregateSessionLike } from './aggregateMessagesModel'

export function countAccountUnread(
  sessions: AggregateSessionLike[] | undefined,
  platformMessagesEnabled: boolean,
): number {
  return (sessions || []).reduce((sum, session) => {
    if (!platformMessagesEnabled && isPlatformNotificationSession(session)) return sum
    return sum + Number((session as any)?.unread || 0)
  }, 0)
}
