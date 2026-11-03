// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { filterMessageCapableAccounts, isMessageCapableAccount } from './messageAccounts'

it('includes restricted accounts when the underlying idle session is active and healthy', () => {
  assert.equal(isMessageCapableAccount({
    uid: '90000000002',
    status: 'restricted',
    raw_status: 'active',
    last_session_status: 'ok',
  }), true)
})

it('excludes disabled or inactive accounts from the message transport scope', () => {
  assert.equal(isMessageCapableAccount({ uid: 'disabled', status: 'disabled' }), false)
  assert.equal(isMessageCapableAccount({ uid: 'inactive', status: 'inactive' }), false)
})

it('excludes delete-only accounts even when the business status still looks active', () => {
  assert.equal(isMessageCapableAccount({
    uid: 'delete-only-1',
    status: 'active',
    deleteOnly: true,
  }), false)
  assert.equal(isMessageCapableAccount({
    uid: 'delete-only-2',
    status: 'active',
    remote_binding_state: 'released_from_me',
  }), false)
  assert.equal(isMessageCapableAccount({
    uid: 'usable-string-false',
    status: 'active',
    deleteOnly: 'false',
    delete_only: '0',
  }), true)
})

it('filters the message account list without dropping restricted online accounts', () => {
  const accounts = [
    { uid: 'active-1', status: 'active' },
    { uid: 'restricted-1', status: 'restricted', raw_status: 'active' },
    { uid: 'restricted-2', status: 'restricted', last_session_status: 'ok' },
    { uid: 'delete-only-1', status: 'active', delete_only: true },
    { uid: 'inactive-1', status: 'inactive' },
    { uid: 'disabled-1', status: 'disabled' },
  ]

  assert.deepEqual(
    filterMessageCapableAccounts(accounts).map((account) => account.uid),
    ['active-1', 'restricted-1', 'restricted-2'],
  )
})
