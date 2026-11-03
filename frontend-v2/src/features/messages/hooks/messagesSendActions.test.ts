// @ts-nocheck
import assert from 'node:assert'
import { it } from 'vitest'

import { removeOptimisticMessageFromHistory } from './messagesSendActions'

it('removeOptimisticMessageFromHistory removes only the failed optimistic bubble', () => {
  const prev = {
    'uid-1:sid-1:1': {
      messages: [
        { messageId: 'real-1', text: 'before' },
        { messageId: 'optimistic_1', text: 'pending' },
        { messageId: 'real-2', text: 'after' },
      ],
    },
  }

  const next = removeOptimisticMessageFromHistory(prev, 'uid-1:sid-1:1', 'optimistic_1')

  assert.deepEqual(next['uid-1:sid-1:1'].messages, [
    { messageId: 'real-1', text: 'before' },
    { messageId: 'real-2', text: 'after' },
  ])
})

it('removeOptimisticMessageFromHistory keeps state unchanged when the bubble is absent', () => {
  const prev = {
    'uid-1:sid-1:1': {
      messages: [{ messageId: 'real-1', text: 'before' }],
    },
  }

  const next = removeOptimisticMessageFromHistory(prev, 'uid-1:sid-1:1', 'optimistic_1')

  assert.equal(next, prev)
})
