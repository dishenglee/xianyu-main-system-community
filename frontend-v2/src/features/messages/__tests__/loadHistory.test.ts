import { describe, expect, it, vi } from 'vitest'

import { loadCompleteHistory } from '../loadHistory'

describe('loadCompleteHistory', () => {
  it('merges all pages until nextCursor is empty', async () => {
    const fetchPage = vi
      .fn()
      .mockResolvedValueOnce({
        messages: [{ messageId: 'm2' }, { messageId: 'm3' }],
        nextCursor: 'p2',
        latestCursor: 'lc1',
      })
      .mockResolvedValueOnce({
        messages: [{ messageId: 'm1' }],
        nextCursor: null,
        latestCursor: 'lc2',
      })

    const progress: Array<{ count: number; pages: number }> = []

    const result = await loadCompleteHistory({
      fetchPage,
      uid: 'u1',
      sessionId: 's1',
      sessionType: 1,
      onProgress: (state) => {
        progress.push({ count: state.count, pages: state.pages })
      },
    })

    expect(fetchPage).toHaveBeenCalledTimes(2)
    expect(result.messages).toEqual([
      { messageId: 'm2' },
      { messageId: 'm3' },
      { messageId: 'm1' },
    ])
    expect(result.nextCursor).toBeNull()
    expect(progress).toEqual([
      { count: 2, pages: 1 },
      { count: 3, pages: 2 },
    ])
  })

  it('stops after page 50 when the backend keeps returning cursors', async () => {
    const fetchPage = vi.fn(async (_uid: string, _sessionId: string, _sessionType: number, _num: number, cursor: unknown) => {
      const pageIndex = Number(cursor || 0)
      return {
        messages: [{ messageId: `m-${pageIndex}` }],
        nextCursor: pageIndex + 1,
      }
    })

    const result = await loadCompleteHistory({
      fetchPage,
      uid: 'u1',
      sessionId: 's1',
      sessionType: 1,
    })

    expect(fetchPage).toHaveBeenCalledTimes(50)
    expect(result.messages).toHaveLength(50)
    expect(result.reachedLimit).toBe(true)
    expect(result.notice).toBe('')
  })
})
