// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { resolveMessageLane } from './messagePresentation'

it('buyer text messages keep self on the right', () => {
  const lane = resolveMessageLane(
    { sessionType: 1 },
    { isSelf: true, kind: 'text' },
  )

  assert.equal(lane, 'outgoing')
})

it('card messages render as outgoing bubbles when isSelf', () => {
  const lane = resolveMessageLane(
    { sessionType: 1 },
    { isSelf: true, kind: 'card' },
  )

  assert.equal(lane, 'outgoing')
})

it('card messages render as incoming bubbles when not isSelf', () => {
  const lane = resolveMessageLane(
    { sessionType: 1 },
    { isSelf: false, kind: 'card' },
  )

  assert.equal(lane, 'incoming')
})

it('tip messages render in system lane', () => {
  const lane = resolveMessageLane(
    { sessionType: 1 },
    { isSelf: true, kind: 'tip' },
  )

  assert.equal(lane, 'system')
})

it('notification sessions always render as notification cards', () => {
  const lane = resolveMessageLane(
    { sessionType: 6 },
    { isSelf: true, kind: 'text' },
  )

  assert.equal(lane, 'notification')
})
