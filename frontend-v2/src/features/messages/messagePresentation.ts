// @ts-nocheck
import { getSessionMeta } from './sessionLayout'

export function resolveMessageLane(session, message) {
  const section = getSessionMeta(session?.sessionType).section
  if (section === 'notification') return 'notification'
  if (message?.kind === 'tip') return 'system'
  return message?.isSelf ? 'outgoing' : 'incoming'
}
