import { describe, expect, it } from 'vitest'

import { computeReadHint } from './messageBubbleLogic'

describe('computeReadHint', () => {
  it('hides read hint when unreadCount is unknown', () => {
    expect(
      computeReadHint({
        message: { version: 1, unreadCount: -1 },
        mine: true,
        sessionType: 1,
      }),
    ).toEqual({ show: false, isRead: false })
  })
})
