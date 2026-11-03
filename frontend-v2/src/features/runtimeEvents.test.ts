import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  emitAccountsSynced,
  emitItemsSynced,
  emitOrdersSynced,
  listenBatchedSyncEvent,
  listenSyncEvent,
  XIANYU_ACCOUNTS_SYNCED_EVENT,
  XIANYU_ITEMS_SYNCED_EVENT,
  XIANYU_ORDERS_SYNCED_EVENT,
} from './runtimeEvents'

describe('runtimeEvents', () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('normalizes uid and uids payloads for listeners', () => {
    const itemListener = vi.fn()
    const orderListener = vi.fn()

    const stopItems = listenSyncEvent(XIANYU_ITEMS_SYNCED_EVENT, itemListener)
    const stopOrders = listenSyncEvent(XIANYU_ORDERS_SYNCED_EVENT, orderListener)

    emitItemsSynced({ uid: 90000000002 as unknown as string, source: 'test' })
    emitOrdersSynced({ uids: ['u1', 2 as unknown as string, '', 'u1'], total: 3 })

    expect(itemListener).toHaveBeenCalledWith(['90000000002'], {
      uid: 90000000002,
      source: 'test',
    })
    expect(orderListener).toHaveBeenCalledWith(['u1', '2', 'u1'], {
      uids: ['u1', 2, '', 'u1'],
      total: 3,
    })

    stopItems()
    stopOrders()
  })

  it('passes an empty uid list when detail is omitted', () => {
    const listener = vi.fn()
    const stop = listenSyncEvent(XIANYU_ACCOUNTS_SYNCED_EVENT, listener)

    emitAccountsSynced()

    expect(listener).toHaveBeenCalledWith([], {})

    stop()
  })

  it('stops receiving events after unsubscribe', () => {
    const listener = vi.fn()
    const stop = listenSyncEvent(XIANYU_ITEMS_SYNCED_EVENT, listener)

    stop()
    emitItemsSynced({ uid: 'u1' })

    expect(listener).not.toHaveBeenCalled()
  })

  it('batches repeated sync events and merges uids', async () => {
    vi.useFakeTimers()
    const listener = vi.fn()
    const stop = listenBatchedSyncEvent(XIANYU_ITEMS_SYNCED_EVENT, 100, listener)

    emitItemsSynced({ uid: 'u1', source: 'a' })
    emitItemsSynced({ uids: ['u1', 'u2'], source: 'b' })
    emitItemsSynced({ uid: 'u2', source: 'c' })

    vi.advanceTimersByTime(100)
    await Promise.resolve()

    expect(listener).toHaveBeenCalledTimes(1)
    expect(listener).toHaveBeenCalledWith(
      ['u1', 'u2'],
      [
        { uid: 'u1', source: 'a' },
        { uids: ['u1', 'u2'], source: 'b' },
        { uid: 'u2', source: 'c' },
      ],
    )

    stop()
  })
})
