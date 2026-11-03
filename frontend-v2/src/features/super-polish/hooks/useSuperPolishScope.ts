import { useOutletContext } from 'react-router-dom'
import type { Account } from '../../../api/types'

export interface SuperPolishScope {
  uid: string
  account: Account | null
  accounts: Account[]
  accountsLoading: boolean
  setUid: (uid: string) => void
  buildPath: (path: string) => string
  mobile: boolean
}

export function useSuperPolishScope() {
  return useOutletContext<SuperPolishScope>()
}
