import { describe, expect, it, vi } from 'vitest'

import { loadAccountsUntilQrAccountVisible } from './useAccountsPageState'

type LoadedAccount = { uid: string; nick: string }

describe('QR account list refresh', () => {
  it('keeps force-refreshing until the scanned account appears in the account list', async () => {
    const loadAccounts = vi.fn()
      .mockResolvedValueOnce([{ uid: 'old-account', nick: '旧账号' }])
      .mockResolvedValueOnce([
        { uid: 'old-account', nick: '旧账号' },
        { uid: 'new-account', nick: '新账号' },
      ])

    const result = await loadAccountsUntilQrAccountVisible({
      loadAccounts,
      beforeUids: new Set(['old-account']),
      resolvedUid: 'new-account',
      delayMs: 0,
      maxAttempts: 3,
      isCurrent: () => true,
    })

    expect(loadAccounts).toHaveBeenCalledTimes(2)
    expect(loadAccounts).toHaveBeenNthCalledWith(1, { force: true })
    expect(loadAccounts).toHaveBeenNthCalledWith(2, { force: true })
    expect(result?.matchedAccount?.uid).toBe('new-account')
    expect(result?.accounts.map((account: LoadedAccount) => account.uid)).toEqual(['old-account', 'new-account'])
  })

  it('can detect a newly added account even when qr status does not include uid', async () => {
    const loadAccounts = vi.fn()
      .mockResolvedValueOnce([{ uid: 'old-account', nick: '旧账号' }])
      .mockResolvedValueOnce([
        { uid: 'fresh-account', nick: '新账号' },
        { uid: 'old-account', nick: '旧账号' },
      ])

    const result = await loadAccountsUntilQrAccountVisible({
      loadAccounts,
      beforeUids: new Set(['old-account']),
      resolvedUid: '',
      delayMs: 0,
      maxAttempts: 3,
      isCurrent: () => true,
    })

    expect(loadAccounts).toHaveBeenCalledTimes(2)
    expect(result?.matchedAccount?.uid).toBe('fresh-account')
  })

  it('stops after the configured maximum attempts when the new account is not visible', async () => {
    const loadAccounts = vi.fn()
      .mockResolvedValue([{ uid: 'old-account', nick: '旧账号' }])

    const result = await loadAccountsUntilQrAccountVisible({
      loadAccounts,
      beforeUids: new Set(['old-account']),
      resolvedUid: 'new-account',
      delayMs: 0,
      maxAttempts: 3,
      isCurrent: () => true,
    })

    expect(loadAccounts).toHaveBeenCalledTimes(3)
    expect(result?.matchedAccount).toBeNull()
    expect(result?.accounts.map((account: LoadedAccount) => account.uid)).toEqual(['old-account'])
  })
})
