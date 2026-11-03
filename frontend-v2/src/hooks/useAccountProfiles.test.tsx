import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { Account } from '../api/types'
import { useAccountProfiles } from './useAccountProfiles'
import { useAuthStore } from '../store/authStore'

const { accountProfileMock, storage } = vi.hoisted(() => {
  const storage = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => { storage.set(key, String(value)) },
      removeItem: (key: string) => { storage.delete(key) },
    },
  })
  return {
    accountProfileMock: vi.fn(),
    storage,
  }
})

vi.mock('../api/index', () => ({
  accountProfile: (...args: unknown[]) => accountProfileMock(...args),
}))

describe('useAccountProfiles', () => {
  beforeEach(() => {
    vi.setSystemTime(new Date('2026-05-23T12:00:00Z'))
    accountProfileMock.mockReset()
    storage.clear()
    useAuthStore.setState({ token: '', user: null, _hydrated: true })
  })

  it('skips remote profile loading when summary cache is still fresh', async () => {
    const accounts: Account[] = [
      {
        uid: 'uid-fresh',
        nick: 'fresh',
        status: 'active',
        coin_balance: 12,
        fish_power: 34,
        violation_count: 1,
        totalEarned: 56.78,
        fishShopEnabled: true,
        fishShopLevel: 'Lv.3',
        summaryCachedAt: 1779537600,
      },
    ]

    const { result } = renderHook(() => useAccountProfiles(accounts, 1))

    await waitFor(() => expect(result.current.loading).toBe(false))

    expect(accountProfileMock).not.toHaveBeenCalled()
    expect(result.current.profiles['uid-fresh']?.totalEarned).toBe(56.78)
    expect(result.current.profiles['uid-fresh']?.fishShopLevel).toBe('Lv.3')
  })

  it('forces a refresh when summary cache is older than 6000 seconds', async () => {
    accountProfileMock.mockResolvedValue({
      uid: 'uid-stale',
      totalEarned: 99.99,
      fishShopEnabled: true,
      fishShopLevel: 'Lv.8',
      _cached_at: 1779537600,
    })

    const accounts: Account[] = [
      {
        uid: 'uid-stale',
        nick: 'stale',
        status: 'active',
        totalEarned: 12.34,
        summaryCachedAt: 1779531599,
      },
    ]

    const { result } = renderHook(() => useAccountProfiles(accounts, 1))

    await waitFor(() => {
      expect(accountProfileMock).toHaveBeenCalledWith('uid-stale', true)
    })
    await waitFor(() => {
      expect(result.current.profiles['uid-stale']?.totalEarned).toBe(99.99)
    })
  })

  it('does not refetch stale profiles when parent passes a new array with identical account data', async () => {
    accountProfileMock.mockResolvedValue({
      uid: 'uid-stale',
      totalEarned: 99.99,
      _cached_at: 1779537600,
    })

    const staleAccount: Account = {
      uid: 'uid-stale',
      nick: 'stale',
      status: 'active',
      totalEarned: 12.34,
      summaryCachedAt: 1779531599,
    }

    const { result, rerender } = renderHook(
      ({ accounts }) => useAccountProfiles(accounts, 1),
      { initialProps: { accounts: [staleAccount] } },
    )

    await waitFor(() => {
      expect(result.current.profiles['uid-stale']?.totalEarned).toBe(99.99)
    })

    rerender({ accounts: [{ ...staleAccount }] })
    rerender({ accounts: [{ ...staleAccount }] })

    expect(accountProfileMock).toHaveBeenCalledTimes(1)
  })

  it('does not reuse a previous token profile for the same uid', async () => {
    accountProfileMock
      .mockResolvedValueOnce({
        uid: 'same-uid',
        totalEarned: 100,
      })
      .mockReturnValue(new Promise(() => {}))

    act(() => {
      useAuthStore.getState().setAuth('token-a', { username: 'user-a' })
    })

    const tokenAAccounts: Account[] = [{
      uid: 'same-uid',
      nick: 'same',
      status: 'active',
      totalEarned: 1,
      summaryCachedAt: 0,
    }]

    const { result, rerender } = renderHook(
      ({ accounts }) => useAccountProfiles(accounts, 1),
      { initialProps: { accounts: tokenAAccounts } },
    )

    await waitFor(() => {
      expect(result.current.profiles['same-uid']?.totalEarned).toBe(100)
    })

    act(() => {
      useAuthStore.getState().setAuth('token-b', { username: 'user-b' })
    })

    rerender({
      accounts: [{
        uid: 'same-uid',
        nick: 'same',
        status: 'active',
        totalEarned: 2,
        summaryCachedAt: 0,
      }],
    })

    await waitFor(() => {
      expect(result.current.profiles['same-uid']?.totalEarned).toBe(2)
    })
  })

  it('keeps empty profile state stable across fresh empty account arrays', () => {
    const { result, rerender } = renderHook(
      ({ accounts }) => useAccountProfiles(accounts, 1),
      { initialProps: { accounts: [] as Account[] } },
    )

    const firstProfiles = result.current.profiles

    rerender({ accounts: [] })
    rerender({ accounts: [] })

    expect(accountProfileMock).not.toHaveBeenCalled()
    expect(result.current.profiles).toBe(firstProfiles)
  })
})
