// @ts-nocheck
import { describe, it } from 'vitest'
import assert from 'node:assert'

import { summarizeCardPools } from './deliveryCardPools'

describe('summarizeCardPools', () => {
  it('groups cards by pool tag and counts total and unused cards', () => {
    assert.deepEqual(
      summarizeCardPools([
        { cardPoolTag: 'pool-b', used: false },
        { cardPoolTag: 'pool-a', used: true },
        { cardPoolTag: 'pool-a', used: false },
        { cardPoolTag: '', used: false },
      ]),
      [
        { tag: 'pool-a', total: 2, unused: 1, label: 'pool-a（可用 1 / 总数 2）' },
        { tag: 'pool-b', total: 1, unused: 1, label: 'pool-b（可用 1 / 总数 1）' },
      ],
    )
  })

  it('returns an empty list when no valid pool tags are present', () => {
    assert.deepEqual(
      summarizeCardPools([
        { cardPoolTag: '', used: false },
        { used: false },
      ]),
      [],
    )
  })
})
