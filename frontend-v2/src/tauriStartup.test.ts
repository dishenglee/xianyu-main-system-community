import { describe, expect, it, vi } from 'vitest'

import { waitForTauriRuntime } from './tauriStartup'

describe('waitForTauriRuntime', () => {
  it('retries until the desktop bridge becomes available', async () => {
    vi.useFakeTimers()

    let checks = 0
    const pending = waitForTauriRuntime({
      detect: () => {
        checks += 1
        return checks >= 3
      },
      timeoutMs: 500,
      pollMs: 50,
    })

    await vi.advanceTimersByTimeAsync(120)

    await expect(pending).resolves.toBe(true)
    expect(checks).toBeGreaterThanOrEqual(3)

    vi.useRealTimers()
  })

  it('returns false after the timeout expires', async () => {
    vi.useFakeTimers()

    const pending = waitForTauriRuntime({
      detect: () => false,
      timeoutMs: 120,
      pollMs: 40,
    })

    await vi.advanceTimersByTimeAsync(160)

    await expect(pending).resolves.toBe(false)

    vi.useRealTimers()
  })
})
