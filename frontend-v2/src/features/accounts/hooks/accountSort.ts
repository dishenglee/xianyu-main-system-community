import type { Account } from '../../../api/types'
import { compareDeleteOnlyLast } from '../../../utils/accountAccess'

function normalizeAddedAt(account: Partial<Account> & Record<string, any>): number {
  const raw = Number(account?.added_at ?? account?.addedAt ?? account?.bound_at ?? account?.boundAt ?? 0)
  return Number.isFinite(raw) ? raw : 0
}

export function compareAccountsByAddedAt(
  left: Partial<Account> & Record<string, any>,
  right: Partial<Account> & Record<string, any>,
): number {
  const deleteOnlyDiff = compareDeleteOnlyLast(left, right)
  if (deleteOnlyDiff !== 0) return deleteOnlyDiff
  const addedAtDiff = normalizeAddedAt(right) - normalizeAddedAt(left)
  if (addedAtDiff !== 0) return addedAtDiff
  return String(left?.uid ?? '').localeCompare(String(right?.uid ?? ''), 'zh-Hans-CN')
}

export function sortAccountsByAddedAt<T extends Partial<Account> & Record<string, any>>(
  accounts: T[],
): T[] {
  return [...accounts].sort(compareAccountsByAddedAt)
}
