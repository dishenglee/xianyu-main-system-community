// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { resolveRefreshInterval } from './refreshPolicy'

it('resolveRefreshInterval uses fallback when transport is disconnected', () => {
  assert.equal(resolveRefreshInterval({
    transportConnected: false,
    activeUids: ['u1'],
    connectionStates: {
      u1: { status: 'connected' },
    },
    liveRefreshInterval: 25000,
    fallbackRefreshInterval: 5000,
  }), 5000)
})

it('resolveRefreshInterval uses fallback while any active account is not connected', () => {
  assert.equal(resolveRefreshInterval({
    transportConnected: true,
    activeUids: ['u1', 'u2'],
    connectionStates: {
      u1: { status: 'connected' },
      u2: { status: 'reconnecting' },
    },
    liveRefreshInterval: 25000,
    fallbackRefreshInterval: 5000,
  }), 5000)
})

it('resolveRefreshInterval disables polling when all active accounts are connected', () => {
  assert.equal(resolveRefreshInterval({
    transportConnected: true,
    activeUids: ['u1', 'u2'],
    connectionStates: {
      u1: { status: 'connected' },
      u2: { status: 'connected' },
    },
    fallbackRefreshInterval: 5000,
  }), null)
})
