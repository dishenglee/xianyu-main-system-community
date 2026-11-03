import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  clearClientLogDedup,
  installGlobalClientLogging,
  normalizeConsoleArgs,
  shouldDropDuplicateClientLog,
} from './clientLogging'

describe('clientLogging', () => {
  beforeEach(() => {
    clearClientLogDedup()
  })

  it('deduplicates repeated identical events inside the cooldown window', () => {
    const event = {
      level: 'error' as const,
      logger: 'frontend.console',
      module: 'frontend-console',
      action: 'console_error',
      route: '/messages',
      errorCode: 'console_error',
      message: 'send message error',
    }

    expect(shouldDropDuplicateClientLog(event, 1000)).toBe(false)
    expect(shouldDropDuplicateClientLog(event, 1200)).toBe(true)
    expect(shouldDropDuplicateClientLog(event, 7001)).toBe(false)
  })

  it('normalizes console arguments into message and detail text', () => {
    const payload = normalizeConsoleArgs([
      'send message error',
      new Error('socket dropped'),
      { code: 503 },
    ])

    expect(payload.message).toBe('send message error')
    expect(payload.detail).toContain('socket dropped')
    expect(payload.detail).toContain('"code":503')
  })

  it('installs global handlers and forwards console error/warn with deduplication', () => {
    const emitter = vi.fn()
    installGlobalClientLogging(emitter)

    console.error('upload/send image error', new Error('HTTP 413'))
    console.error('upload/send image error', new Error('HTTP 413'))
    console.warn('dashboard fallback', { source: 'stats' })

    expect(emitter).toHaveBeenCalledTimes(2)
    expect(emitter.mock.calls[0][0]).toMatchObject({
      logger: 'frontend.console',
      action: 'console_error',
      message: 'upload/send image error',
    })
    expect(emitter.mock.calls[1][0]).toMatchObject({
      level: 'warn',
      logger: 'frontend.console',
      action: 'console_warn',
      message: 'dashboard fallback',
    })
  })
})
