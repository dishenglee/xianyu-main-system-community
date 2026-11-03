import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { resetAccountsCacheForTest, useAccounts } from './useAccounts'

const getAccountsMock = vi.fn()

vi.mock('../api/index', () => ({
  getAccounts: (...args: unknown[]) => getAccountsMock(...args),
  deleteAccount: vi.fn(),
  refreshAccount: vi.fn(),
}))

describe('useAccounts', () => {
  const storage = new Map<string, string>()

  beforeEach(() => {
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value)
      },
      removeItem: (key: string) => {
        storage.delete(key)
      },
    })
    storage.clear()
    getAccountsMock.mockReset()
    resetAccountsCacheForTest()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('dedupes concurrent account loads across mounted pages', async () => {
    getAccountsMock.mockResolvedValue([{ uid: 'u1', nick: '账号甲', status: 'active' }])

    const first = renderHook(() => useAccounts())
    const second = renderHook(() => useAccounts())

    await act(async () => {
      await Promise.all([
        first.result.current.loadAccounts(),
        second.result.current.loadAccounts(),
      ])
    })

    expect(getAccountsMock).toHaveBeenCalledTimes(1)
    expect(first.result.current.accounts).toEqual(second.result.current.accounts)
    expect(first.result.current.accounts[0]?.nick).toBe('账号甲')
  })

  it('reuses a fresh account snapshot unless force refresh is requested', async () => {
    getAccountsMock
      .mockResolvedValueOnce([{ uid: 'u1', nick: '账号甲', status: 'active' }])
      .mockResolvedValueOnce([{ uid: 'u2', nick: '账号乙', status: 'active' }])

    const { result } = renderHook(() => useAccounts())

    await act(async () => {
      await result.current.loadAccounts()
      await result.current.loadAccounts()
    })

    expect(getAccountsMock).toHaveBeenCalledTimes(1)
    expect(result.current.accounts[0]?.uid).toBe('u1')

    await act(async () => {
      await result.current.loadAccounts({ force: true })
    })

    await waitFor(() => expect(result.current.accounts[0]?.uid).toBe('u2'))
    expect(getAccountsMock).toHaveBeenCalledTimes(2)
  })

  it('keeps derived account arrays stable across rerenders with unchanged data', async () => {
    getAccountsMock.mockResolvedValue([{ uid: 'u1', nick: '账号甲', status: 'active' }])

    const { result, rerender } = renderHook(() => useAccounts())

    await act(async () => {
      await result.current.loadAccounts()
    })

    const firstAccounts = result.current.accounts
    const firstActiveAccounts = result.current.activeAccounts

    rerender()
    rerender()

    expect(result.current.accounts).toBe(firstAccounts)
    expect(result.current.activeAccounts).toBe(firstActiveAccounts)
  })

  it('does not show cached admin accounts after switching to a user token', async () => {
    getAccountsMock
      .mockResolvedValueOnce([{ uid: 'admin-account', nick: '管理员账号', status: 'active' }])
      .mockResolvedValueOnce([{ uid: 'user-account', nick: '用户账号', status: 'active' }])

    localStorage.setItem('auth_token', 'admin-token')
    const { result } = renderHook(() => useAccounts())

    await act(async () => {
      await result.current.loadAccounts()
    })

    expect(result.current.accounts.map((account) => account.uid)).toEqual(['admin-account'])

    localStorage.setItem('auth_token', 'user-token')

    await act(async () => {
      await result.current.loadAccounts()
    })

    expect(getAccountsMock).toHaveBeenCalledTimes(2)
    expect(result.current.accounts.map((account) => account.uid)).toEqual(['user-account'])
  })

  it('clears mounted account state immediately when auth token changes', async () => {
    getAccountsMock.mockResolvedValue([{ uid: 'admin-account', nick: '管理员账号', status: 'active' }])

    localStorage.setItem('auth_token', 'admin-token')
    const { result } = renderHook(() => useAccounts())

    await act(async () => {
      await result.current.loadAccounts()
    })

    expect(result.current.accounts.map((account) => account.uid)).toEqual(['admin-account'])

    await act(async () => {
      localStorage.setItem('auth_token', 'user-token')
      window.dispatchEvent(new Event('xy:authChanged'))
    })

    await waitFor(() => expect(result.current.accounts).toEqual([]))
  })

  it('keeps delete-only accounts visible at the bottom but excludes them from active accounts', async () => {
    getAccountsMock.mockResolvedValue([
      { uid: 'delete-only', nick: '仅可删除', status: 'active', deleteOnly: true },
      { uid: 'usable', nick: '可用账号', status: 'active' },
    ])

    const { result } = renderHook(() => useAccounts())

    await act(async () => {
      await result.current.loadAccounts()
    })

    expect(result.current.accounts.map((account) => account.uid)).toEqual(['usable', 'delete-only'])
    expect(result.current.activeAccounts.map((account) => account.uid)).toEqual(['usable'])
  })

  it('does not treat string false delete-only flags as restricted accounts', async () => {
    getAccountsMock.mockResolvedValue([
      { uid: 'usable-string-false', status: 'active', deleteOnly: 'false', delete_only: '0' },
      { uid: 'delete-only-state', status: 'active', remoteBindingState: 'owned_by_other' },
    ])

    const { result } = renderHook(() => useAccounts())

    await act(async () => {
      await result.current.loadAccounts()
    })

    expect(result.current.activeAccounts.map((account) => account.uid)).toEqual(['usable-string-false'])
    expect(result.current.accounts.map((account) => account.uid)).toEqual(['usable-string-false', 'delete-only-state'])
  })
})
