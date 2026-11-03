import type { Account } from '../../api/types'
import { filterBusinessAccounts, isBusinessAccount } from '../../utils/accountAccess'

export const capabilityKeys = ['autoReply', 'autoShip', 'aiEnabled'] as const

export type CapabilityKey = (typeof capabilityKeys)[number]

const capabilityFieldAliases = {
  autoReply: ['autoReply', 'auto_reply_enabled'],
  autoShip: ['autoShip', 'auto_ship_enabled'],
  aiEnabled: ['aiEnabled', 'ai_enabled'],
} as const satisfies Record<CapabilityKey, readonly string[]>

export const capabilityLabels = {
  autoReply: '回复',
  autoShip: '自动发货',
  aiEnabled: 'AI 助手',
} as const satisfies Record<CapabilityKey, string>

export type CapabilityAccount = Partial<Account> & Record<string, unknown>

export function getAccountUid(account: CapabilityAccount): string | null {
  return typeof account.uid === 'string' && account.uid.length > 0 ? account.uid : null
}

export function getCapabilityValue(account: CapabilityAccount | null | undefined, key: CapabilityKey) {
  if (!account) return false

  const replyMode = typeof account.replyMode === 'string'
    ? String(account.replyMode).trim()
    : typeof account.reply_mode === 'string'
      ? String(account.reply_mode).trim()
      : ''
  if (key === 'autoReply' && replyMode) {
    return replyMode !== 'off'
  }
  if (key === 'aiEnabled' && replyMode) {
    return replyMode === 'ai'
  }

  for (const field of capabilityFieldAliases[key]) {
    if (field in account) {
      return Boolean(account[field])
    }
  }

  return false
}

export function filterEnabledAccounts<T extends CapabilityAccount>(accounts: T[] = [], capability: CapabilityKey) {
  return accounts.filter((account) => isBusinessAccount(account) && getCapabilityValue(account, capability))
}

export function sanitizeSelectedUids<T extends CapabilityAccount>(
  selectedUids: readonly string[] = [],
  accounts: T[] = [],
  capability: CapabilityKey,
) {
  const enabledUids = new Set(filterEnabledAccounts(accounts, capability).map(getAccountUid).filter((uid): uid is string => Boolean(uid)))
  return selectedUids.filter((uid) => enabledUids.has(uid))
}

export function sanitizeSelectableUids<T extends CapabilityAccount>(
  selectedUids: readonly string[] = [],
  accounts: T[] = [],
  capability: CapabilityKey,
  { allowEnableDisabledAccounts = false }: { allowEnableDisabledAccounts?: boolean } = {},
) {
  if (!allowEnableDisabledAccounts) {
    return sanitizeSelectedUids(selectedUids, accounts, capability)
  }

  const activeUids = new Set(
    filterBusinessAccounts(accounts)
      .map(getAccountUid)
      .filter((uid): uid is string => Boolean(uid)),
  )
  return selectedUids.filter((uid) => activeUids.has(uid))
}

function uniqueUids(uids: readonly string[] = []) {
  const seen = new Set<string>()
  const result: string[] = []

  for (const uid of uids) {
    const normalized = String(uid || '').trim()
    if (!normalized || seen.has(normalized)) continue
    seen.add(normalized)
    result.push(normalized)
  }

  return result
}

export function enabledCapabilityUids<T extends CapabilityAccount>(
  accounts: T[] = [],
  capability: CapabilityKey,
) {
  return filterEnabledAccounts(accounts, capability)
    .map(getAccountUid)
    .filter((uid): uid is string => Boolean(uid))
}

export function diffCapabilitySelection(savedUids: readonly string[] = [], draftUids: readonly string[] = []) {
  const saved = uniqueUids(savedUids)
  const draft = uniqueUids(draftUids)
  const savedSet = new Set(saved)
  const draftSet = new Set(draft)
  const toEnable = draft.filter((uid) => !savedSet.has(uid))
  const toDisable = saved.filter((uid) => !draftSet.has(uid))

  return {
    toEnable,
    toDisable,
    dirty: toEnable.length > 0 || toDisable.length > 0,
  }
}

export function nextCapabilitySelection({
  currentUids = [],
  uid,
  checked,
  singleSelect = false,
}: {
  currentUids?: readonly string[]
  uid: string
  checked: boolean
  singleSelect?: boolean
}) {
  const normalized = String(uid || '').trim()
  if (!normalized) return uniqueUids(currentUids)

  if (singleSelect) {
    return checked ? [normalized] : []
  }

  const current = uniqueUids(currentUids)
  return checked
    ? uniqueUids([...current, normalized])
    : current.filter((selectedUid) => selectedUid !== normalized)
}
