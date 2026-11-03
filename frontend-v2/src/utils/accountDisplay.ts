const DISPLAY_KEYS = [
  'displayName',
  'display_name',
  'displayNick',
  'display_nick',
  'snsNick',
  'sns_nick',
  'accountNick',
  'account_nick',
  'remark',
  'alias',
  'nickname',
  'nickName',
  'nick',
]

function cleanDisplayText(value: unknown) {
  return String(value ?? '').replace(/\s+/g, ' ').trim()
}

function isPlaceholderName(value: string, uid = '') {
  if (!value) return true
  if (uid && value === uid) return true
  if (value === '未知' || value === '未知账号') return true
  return false
}

export function accountDisplayName(account: Record<string, unknown> | null | undefined, uidFallback = '') {
  const uid = cleanDisplayText(uidFallback || account?.uid)
  for (const key of DISPLAY_KEYS) {
    const value = cleanDisplayText(account?.[key])
    if (!isPlaceholderName(value, uid)) return value
  }
  return uid || '账号'
}

export function accountDisplayNameByUid(
  uid: string,
  accounts: Array<Record<string, unknown>> = [],
) {
  const account = accounts.find((item) => cleanDisplayText(item?.uid) === cleanDisplayText(uid))
  return accountDisplayName(account, uid)
}
