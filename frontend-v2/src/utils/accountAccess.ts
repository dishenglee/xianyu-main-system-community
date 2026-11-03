import type { Account } from '../api/types'

export type AccountLike = Partial<Account> & Record<string, any>

function isTruthyFlag(value: unknown) {
  if (typeof value === 'boolean') return value
  if (typeof value === 'number') return value !== 0
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase()
    if (!normalized || normalized === 'false' || normalized === '0' || normalized === 'no' || normalized === 'null' || normalized === 'undefined') {
      return false
    }
    return normalized === 'true' || normalized === '1' || normalized === 'yes'
  }
  return false
}

export function isDeleteOnlyAccount(account: AccountLike | null | undefined) {
  if (!account) return false
  const bindingState = String(account.remoteBindingState ?? account.remote_binding_state ?? '').trim()
  return isTruthyFlag(account.deleteOnly)
    || isTruthyFlag(account.delete_only)
    || bindingState === 'released_from_me'
    || bindingState === 'owned_by_other'
}

export function isActiveAccount(account: AccountLike | null | undefined) {
  return String(account?.status || '').trim() === 'active'
}

export function isBusinessAccount(account: AccountLike | null | undefined) {
  const status = String(account?.status || '').trim()
  return (status === 'active' || status === 'restricted') && !isDeleteOnlyAccount(account)
}

export function filterBusinessAccounts<T extends AccountLike>(accounts: T[] = []) {
  return accounts.filter(isBusinessAccount)
}

export function compareDeleteOnlyLast(left: AccountLike, right: AccountLike) {
  const leftDeleteOnly = isDeleteOnlyAccount(left) ? 1 : 0
  const rightDeleteOnly = isDeleteOnlyAccount(right) ? 1 : 0
  return leftDeleteOnly - rightDeleteOnly
}
