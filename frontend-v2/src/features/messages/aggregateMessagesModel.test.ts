import { describe, expect, it } from 'vitest'

import {
  groupSessionsByBucket,
  isPlatformNotificationSession,
  isSessionTypeEnabled,
} from './aggregateMessagesModel'

const NOW = new Date('2026-04-18T12:00:00+08:00')

describe('groupSessionsByBucket', () => {
  it('splits sessions into today, yesterday, this week, and older buckets', () => {
    const groups = groupSessionsByBucket([
      { sessionId: 'today', ts: new Date('2026-04-18T09:00:00+08:00').getTime() },
      { sessionId: 'yesterday', ts: new Date('2026-04-17T09:00:00+08:00').getTime() },
      { sessionId: 'week', ts: new Date('2026-04-15T09:00:00+08:00').getTime() },
      { sessionId: 'older', ts: new Date('2026-04-10T09:00:00+08:00').getTime() },
    ], NOW)

    expect(groups.map((group) => group.label)).toEqual(['今天', '昨天', '本周', '更早'])
    expect(groups[0]?.sessions.map((session) => session.sessionId)).toEqual(['today'])
    expect(groups[1]?.sessions.map((session) => session.sessionId)).toEqual(['yesterday'])
    expect(groups[2]?.sessions.map((session) => session.sessionId)).toEqual(['week'])
    expect(groups[3]?.sessions.map((session) => session.sessionId)).toEqual(['older'])
  })
})

describe('isSessionTypeEnabled', () => {
  it('treats buyer chat as chat and only keeps enabled categories', () => {
    const filters = { chat: true, system: false, favorite: true }

    expect(isSessionTypeEnabled({ sessionType: 1 }, filters)).toBe(true)
    expect(isSessionTypeEnabled({ sessionType: 3 }, filters)).toBe(false)
    expect(isSessionTypeEnabled({ sessionType: 6 }, filters)).toBe(true)
  })
})

describe('isPlatformNotificationSession', () => {
  it('treats notification, interaction, and workbench sessions as platform channels', () => {
    expect(isPlatformNotificationSession({ sessionType: 1 })).toBe(false)
    expect(isPlatformNotificationSession({ sessionType: 3 })).toBe(true)
    expect(isPlatformNotificationSession({ sessionType: 6 })).toBe(true)
    expect(isPlatformNotificationSession({ sessionType: 53 })).toBe(true)
  })
})
