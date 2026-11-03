import { describe, it, expect } from 'vitest'
import { limitConcurrency } from './limitConcurrency'

describe('limitConcurrency', () => {
  it('processes all items and returns results in order', async () => {
    const results = await limitConcurrency([1, 2, 3, 4, 5], 2, async (n) => n * 2)
    expect(results).toEqual([2, 4, 6, 8, 10])
  })

  it('handles empty array', async () => {
    const results = await limitConcurrency([], 3, async (n: number) => n)
    expect(results).toEqual([])
  })

  it('respects concurrency limit', async () => {
    let active = 0
    let maxActive = 0
    const results = await limitConcurrency(
      [1, 2, 3, 4, 5, 6],
      2,
      async (n) => {
        active++
        maxActive = Math.max(maxActive, active)
        await new Promise((r) => setTimeout(r, 10))
        active--
        return n
      },
    )
    expect(results).toEqual([1, 2, 3, 4, 5, 6])
    expect(maxActive).toBeLessThanOrEqual(2)
  })
})
