import { describe, expect, it } from 'vitest'

import { sortAccountsByAddedAt } from './accountSort'

describe('sortAccountsByAddedAt', () => {
  it('keeps account lists ordered by current owner add time instead of updated_at or runtime status', () => {
    const accounts = [
      {
        uid: 'older-updated-last',
        created_at: 1710000000,
        bound_at: 1710000000,
        updated_at: 1719999999,
        status: 'active',
        last_session_status: 'ok',
      },
      {
        uid: 'newer-created-first',
        created_at: 1720000000,
        bound_at: 1720000000,
        updated_at: 1710000001,
        status: 'restricted',
        last_session_status: 'error',
      },
      {
        uid: 'middle-created',
        created_at: 1715000000,
        bound_at: 1715000000,
        updated_at: 1715000001,
        status: 'active',
        last_session_status: 'checking',
      },
    ]

    const sorted = sortAccountsByAddedAt(accounts)

    expect(sorted.map((account) => account.uid)).toEqual([
      'newer-created-first',
      'middle-created',
      'older-updated-last',
    ])
  })

  it('uses uid as a deterministic tiebreaker when added time matches', () => {
    const sorted = sortAccountsByAddedAt([
      { uid: 'uid-b', bound_at: 1710000000, created_at: 1720000000 },
      { uid: 'uid-a', bound_at: 1710000000, created_at: 1730000000 },
    ])

    expect(sorted.map((account) => account.uid)).toEqual(['uid-a', 'uid-b'])
  })

  it('prefers added_at over bound_at and historical created_at', () => {
    const sorted = sortAccountsByAddedAt([
      { uid: 'old-db-row-bound-today', created_at: 1710000000, bound_at: 1781000000 },
      { uid: 'newer-added', created_at: 1700000000, bound_at: 1780000000, added_at: 1782000000 },
    ])

    expect(sorted.map((account) => account.uid)).toEqual([
      'newer-added',
      'old-db-row-bound-today',
    ])
  })

  it('does not sort by historical created_at when current owner add time is missing', () => {
    const sorted = sortAccountsByAddedAt([
      { uid: 'uid-b', created_at: 1790000000 },
      { uid: 'uid-a', created_at: 1710000000 },
      { uid: 'uid-c', bound_at: 1720000000, created_at: 1700000000 },
    ])

    expect(sorted.map((account) => account.uid)).toEqual([
      'uid-c',
      'uid-a',
      'uid-b',
    ])
  })

  it('always places delete-only accounts after usable accounts', () => {
    const sorted = sortAccountsByAddedAt([
      { uid: 'delete-only-newer', added_at: 1783000000, status: 'active', deleteOnly: true },
      { uid: 'usable-older', added_at: 1710000000, status: 'active' },
      { uid: 'delete-only-older', added_at: 1710000001, status: 'active', delete_only: true },
    ])

    expect(sorted.map((account) => account.uid)).toEqual([
      'usable-older',
      'delete-only-newer',
      'delete-only-older',
    ])
  })
})
