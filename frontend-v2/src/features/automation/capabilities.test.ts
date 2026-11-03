import { describe, expect, it } from 'vitest'

import { diffCapabilitySelection, getCapabilityValue, nextCapabilitySelection, sanitizeSelectedUids, sanitizeSelectableUids } from './capabilities'

describe('automation capabilities', () => {
  it('reads capability values from camelCase and snake_case fields', () => {
    expect(getCapabilityValue({ uid: 'a', status: 'active', autoReply: true }, 'autoReply')).toBe(true)
    expect(getCapabilityValue({ uid: 'b', status: 'active', auto_reply_enabled: true }, 'autoReply')).toBe(true)
    expect(getCapabilityValue({ uid: 'c', status: 'active', auto_ship_enabled: true }, 'autoShip')).toBe(true)
    expect(getCapabilityValue({ uid: 'd', status: 'active', aiEnabled: false, ai_enabled: true }, 'aiEnabled')).toBe(false)
    expect(getCapabilityValue({ uid: 'e', status: 'active' }, 'aiEnabled')).toBe(false)
  })

  it('keeps only active accounts with the selected capability enabled', () => {
    const accounts = [
      { uid: 'enabled-camel', status: 'active', autoReply: true },
      { uid: 'enabled-snake', status: 'active', auto_reply_enabled: true },
      { uid: 'disabled', status: 'active', autoReply: false },
      { uid: 'inactive', status: 'inactive', autoReply: true },
    ]

    expect(sanitizeSelectedUids(
      ['enabled-camel', 'enabled-snake', 'disabled', 'inactive', 'missing'],
      accounts,
      'autoReply',
    )).toEqual(['enabled-camel', 'enabled-snake'])
  })

  it('drops autoShip selections when account management disables autoShip', () => {
    const accounts = [
      { uid: 'still-on', status: 'active', autoShip: true },
      { uid: 'turned-off', status: 'active', autoShip: false },
    ]

    expect(sanitizeSelectedUids(['still-on', 'turned-off'], accounts, 'autoShip')).toEqual(['still-on'])
  })

  it('allows global selectors to keep active accounts before capability sync', () => {
    const accounts = [
      { uid: 'enabled', status: 'active', autoReply: true },
      { uid: 'disabled', status: 'active', autoReply: false },
      { uid: 'inactive', status: 'inactive', autoReply: true },
    ]

    expect(sanitizeSelectableUids(
      ['enabled', 'disabled', 'inactive', 'missing'],
      accounts,
      'autoReply',
      { allowEnableDisabledAccounts: true },
    )).toEqual(['enabled', 'disabled'])
  })

  it('drops selected accounts when account management disables the capability', () => {
    const accounts = [
      { uid: 'enabled', status: 'active', aiEnabled: true },
      { uid: 'disabled', status: 'active', aiEnabled: false },
    ]

    expect(sanitizeSelectableUids(['enabled', 'disabled'], accounts, 'aiEnabled')).toEqual(['enabled'])
  })

  it('diffs draft account selection against the saved selection for explicit apply', () => {
    expect(diffCapabilitySelection(['keep', 'disable'], ['keep', 'enable', 'enable'])).toEqual({
      toEnable: ['enable'],
      toDisable: ['disable'],
      dirty: true,
    })
  })

  it('treats reordered draft selection as unchanged', () => {
    expect(diffCapabilitySelection(['b', 'a'], ['a', 'b'])).toEqual({
      toEnable: [],
      toDisable: [],
      dirty: false,
    })
  })

  it('replaces the previous account when single-select picks a disabled capability account', () => {
    expect(nextCapabilitySelection({
      currentUids: ['already-enabled'],
      uid: 'needs-enable',
      checked: true,
      singleSelect: true,
    })).toEqual(['needs-enable'])
  })
})
