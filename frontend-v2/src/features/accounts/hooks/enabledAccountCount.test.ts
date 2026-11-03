import { describe, expect, it } from 'vitest'

import { countEnabledAccounts } from './useAccountsPageState'

describe('countEnabledAccounts', () => {
  it('counts non-disabled local accounts and excludes delete-only entries', () => {
    expect(
      countEnabledAccounts([
        { uid: 'active', status: 'active' },
        { uid: 'restricted', status: 'restricted' },
        { uid: 'expired', status: 'expired' },
        { uid: 'disabled', status: 'disabled' },
        { uid: 'empty', status: '' },
      ]),
    ).toBe(4)
  })
})
