import assert from 'node:assert'
import { describe, it } from 'vitest'

import { countAccountUnread } from './platformUnread'

describe('countAccountUnread', () => {
  it('excludes platform notification unread when platform messages are disabled', () => {
    const sessions = [
      { sessionType: 1, unread: 2 },
      { sessionType: 6, unread: 5 },
      { sessionType: 53, unread: 3 },
    ]

    assert.equal(countAccountUnread(sessions, false), 2)
  })

  it('keeps the existing total when platform messages are enabled', () => {
    const sessions = [
      { sessionType: 1, unread: 2 },
      { sessionType: 6, unread: 5 },
      { sessionType: 53, unread: 3 },
    ]

    assert.equal(countAccountUnread(sessions, true), 10)
  })
})
