import { useState, useCallback, useEffect, useMemo } from 'react'
import { getAccounts, deleteAccount as apiDelete, refreshAccount as apiRefresh } from '../api/index'
import { getAuthToken } from '../api/client'
import type { Account } from '../api/types'
import { compareDeleteOnlyLast, filterBusinessAccounts } from '../utils/accountAccess'
import {
  getAccountsCache,
  getAccountsInFlight,
  invalidateAccountsCache,
  isAccountsCacheFresh,
  setAccountsCache,
  setAccountsInFlight,
} from './accountsCache'

type LoadAccountsOptions = {
  signal?: AbortSignal
  force?: boolean
}

export { invalidateAccountsCache } from './accountsCache'

async function fetchAccountsShared(options: LoadAccountsOptions = {}) {
  const token = getAuthToken()
  const cached = getAccountsCache(token)
  if (!options.force && isAccountsCacheFresh(token)) {
    return cached?.accounts ?? []
  }
  const inFlight = getAccountsInFlight(token)
  if (!options.force && inFlight) {
    return inFlight
  }

  const request = getAccounts({ signal: options.signal })
    .then((data) => {
      const next = Array.isArray(data) ? data : []
      setAccountsCache(token, next)
      return next
    })
    .finally(() => {
      setAccountsInFlight(token, null)
    })

  setAccountsInFlight(token, request)
  return request
}

export function resetAccountsCacheForTest() {
  invalidateAccountsCache()
}

export function useAccounts() {
  const [token, setToken] = useState(() => getAuthToken())
  const [accounts, setAccounts] = useState<Account[]>(() => getAccountsCache(token)?.accounts ?? [])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    setAccounts(getAccountsCache(token)?.accounts ?? [])
  }, [token])

  useEffect(() => {
    const syncToken = () => setToken(getAuthToken())
    window.addEventListener('xy:authChanged', syncToken)
    window.addEventListener('storage', syncToken)
    return () => {
      window.removeEventListener('xy:authChanged', syncToken)
      window.removeEventListener('storage', syncToken)
    }
  }, [])

  const loadAccounts = useCallback(async (options?: AbortSignal | LoadAccountsOptions) => {
    const loadOptions: LoadAccountsOptions = options instanceof AbortSignal
      ? { signal: options }
      : (options ?? {})

    const requestToken = getAuthToken()
    const cached = getAccountsCache(requestToken)
    if (!loadOptions.force && isAccountsCacheFresh(requestToken)) {
      setAccounts(cached?.accounts ?? [])
      return cached?.accounts ?? []
    }

    setLoading(true)
    try {
      const data = await fetchAccountsShared(loadOptions)
      if (getAuthToken() !== requestToken) {
        return getAccountsCache(getAuthToken())?.accounts ?? []
      }
      setAccounts(data)
      return data
    } catch (e: unknown) {
      if (e instanceof Error && e.name === 'AbortError') return
      console.error('加载账号失败:', e)
      return getAccountsCache(getAuthToken())?.accounts ?? []
    } finally {
      if (getAuthToken() === requestToken) setLoading(false)
    }
  }, [])

  const removeAccount = useCallback(async (uid: string) => {
    await apiDelete(uid)
    invalidateAccountsCache()
    await loadAccounts({ force: true })
  }, [loadAccounts])

  const refreshCookie = useCallback(async (uid: string) => {
    await apiRefresh(uid)
    invalidateAccountsCache()
    await loadAccounts({ force: true })
  }, [loadAccounts])

  const orderedAccounts = useMemo(() => [...accounts].sort(compareDeleteOnlyLast), [accounts])
  const activeAccounts = useMemo(() => filterBusinessAccounts(orderedAccounts), [orderedAccounts])

  return { accounts: orderedAccounts, activeAccounts, loading, loadAccounts, removeAccount, refreshCookie }
}
