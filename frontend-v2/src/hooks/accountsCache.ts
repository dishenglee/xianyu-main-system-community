import type { Account } from '../api/types'

export const ACCOUNT_CACHE_TTL_MS = 15_000

export interface AccountsCacheEntry {
  accounts: Account[]
  cachedAt: number
}

let cachedToken = ''
let cachedEntry: AccountsCacheEntry | null = null
let inFlightToken = ''
let accountsInFlight: Promise<Account[]> | null = null

export function getAccountsCache(token: string): AccountsCacheEntry | null {
  if (token !== cachedToken) return null
  return cachedEntry
}

export function isAccountsCacheFresh(token: string): boolean {
  const entry = getAccountsCache(token)
  return entry !== null && Date.now() - entry.cachedAt < ACCOUNT_CACHE_TTL_MS
}

export function setAccountsCache(token: string, accounts: Account[]) {
  cachedToken = token
  cachedEntry = {
    accounts,
    cachedAt: Date.now(),
  }
}

export function getAccountsInFlight(token: string): Promise<Account[]> | null {
  if (token !== inFlightToken) return null
  return accountsInFlight
}

export function setAccountsInFlight(token: string, promise: Promise<Account[]> | null) {
  inFlightToken = promise ? token : ''
  accountsInFlight = promise
}

export function invalidateAccountsCache() {
  cachedToken = ''
  cachedEntry = null
  inFlightToken = ''
  accountsInFlight = null
}
