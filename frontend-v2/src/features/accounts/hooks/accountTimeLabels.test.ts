import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  accountAddedAt,
  formatAddedAge,
  formatLiveConnectionLabel,
  formatRefreshedAt,
} from './useAccountsPageState'

describe('account time labels', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-06-09T14:00:00Z'))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('labels added_at as current owner add age instead of online duration', () => {
    const addedAt56DaysAgo = Date.parse('2026-04-14T14:00:00Z') / 1000

    expect(formatAddedAge(addedAt56DaysAgo)).toBe('添加 56 天')
    expect(formatAddedAge(addedAt56DaysAgo)).not.toContain('已在线')
  })

  it('labels cookie_refreshed_at as the recent authorization refresh time', () => {
    const refreshedTenMinutesAgo = Date.parse('2026-06-09T13:50:00Z') / 1000

    expect(formatRefreshedAt(refreshedTenMinutesAgo)).toBe('10 分钟前')
  })

  it('labels message-channel duration from last_connected_at instead of account add time', () => {
    const addedAt56DaysAgo = Date.parse('2026-04-14T14:00:00Z') / 1000
    const connectedTenMinutesAgo = Date.parse('2026-06-09T13:50:00Z') / 1000

    expect(formatAddedAge(addedAt56DaysAgo)).toBe('添加 56 天')
    expect(formatLiveConnectionLabel({
      liveStatus: 'connected',
      lastConnectedAt: connectedTenMinutesAgo,
      addedAt: addedAt56DaysAgo,
    })).toBe('消息通道已连接 10 分钟')
  })

  it('does not use historical created_at as current owner add time', () => {
    expect(accountAddedAt({
      uid: 'uid-reused-by-another-user',
      created_at: Date.parse('2026-04-14T14:00:00Z') / 1000,
    })).toBeUndefined()
  })

  it('prefers IM live status over the account platform status field', () => {
    const connectedTenMinutesAgo = Date.parse('2026-06-09T13:50:00Z') / 1000

    expect(formatLiveConnectionLabel({
      status: 'active',
      liveStatus: 'connected',
      liveLastConnectedAt: connectedTenMinutesAgo,
    })).toBe('消息通道已连接 10 分钟')
  })

  it('does not show an online duration when IM is disconnected', () => {
    const disconnectedTwoHoursAgo = Date.parse('2026-06-09T12:00:00Z') / 1000

    expect(formatLiveConnectionLabel({
      liveStatus: 'disconnected',
      lastDisconnectedAt: disconnectedTwoHoursAgo,
      lastConnectedAt: Date.parse('2026-04-14T14:00:00Z') / 1000,
    })).toBe('消息通道已断开 2 小时')
  })
})
