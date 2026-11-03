// @ts-nocheck
import { isDeleteOnlyAccount } from '../../utils/accountAccess'

// Messaging should include accounts whose IM session is still healthy even if
// business status is derived as "restricted" from active violation metadata.
export function isMessageCapableAccount(account: any) {
  const status = String(account?.status || '').trim()
  if (isDeleteOnlyAccount(account)) return false
  if (status === 'disabled' || status === 'inactive') return false
  if (status === 'active') return true

  const rawStatus = String(account?.raw_status || account?.rawStatus || '').trim()
  const sessionStatus = String(account?.last_session_status || account?.lastSessionStatus || '').trim()
  if (status === 'restricted') {
    return rawStatus === 'active' || sessionStatus === 'ok'
  }

  return false
}

export function filterMessageCapableAccounts(accounts: any[]) {
  return (Array.isArray(accounts) ? accounts : []).filter(isMessageCapableAccount)
}
