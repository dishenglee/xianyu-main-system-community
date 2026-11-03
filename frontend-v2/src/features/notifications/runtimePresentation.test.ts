// @ts-nocheck
import { describe, expect, it } from 'vitest'

import { presentNotificationRuntime } from './runtimePresentation'

describe('presentNotificationRuntime', () => {
  it('keeps listener_status precedence over runtime.status and exposes operator facts', () => {
    const presentation = presentNotificationRuntime({
      runtime: {
        status: 'error',
        last_error: 'wechat poll failed',
        last_attempt_at: 1710000000,
        last_success_at: 1710000100,
        runtime_json: {
          listener_status: 'listening',
          last_callback_source: 'wechat.callback',
          last_callback_method: 'POST',
          last_callback_at: 1710000200,
          worker_restart_status: 'spawned',
          worker_restart_source: 'wechat.manager',
          worker_restart_reason: 'manual restart',
          worker_channel_type: 'openclaw_weixin',
        },
      },
    })

    expect(presentation.status).toBe('listening')
    expect(presentation.label).toBe('监听中')
    expect(presentation.dot).toBe('var(--success)')
    expect(presentation.lastError).toBe('wechat poll failed')
    expect(presentation.callback.source).toBe('wechat.callback')
    expect(presentation.callback.method).toBe('POST')
    expect(presentation.workerRestart.status).toBe('spawned')
    expect(presentation.workerRestart.reason).toBe('manual restart')
    expect(presentation.facts.map((fact) => fact.key)).toEqual([
      'listener',
      'attempt',
      'success',
      'callback',
      'worker_restart',
      'error',
    ])
  })

  it('falls back to idle and omits empty diagnostics', () => {
    const presentation = presentNotificationRuntime({ runtime: {} })

    expect(presentation.status).toBe('idle')
    expect(presentation.label).toBe('待激活')
    expect(presentation.dot).toBe('var(--text-muted)')
    expect(presentation.facts).toEqual([])
    expect(presentation.callback.at).toBe('')
    expect(presentation.workerRestart.updatedAt).toBe('')
  })

  it('surfaces failed runtime states in danger tone', () => {
    const presentation = presentNotificationRuntime({
      runtime: {
        status: 'failed',
        last_error: 'missing bot_token',
        runtime_json: {
          worker_restart_status: 'error',
          worker_restart_source: 'wechat.manager',
        },
      },
    })

    expect(presentation.label).toBe('失败')
    expect(presentation.tone).toBe('danger')
    expect(presentation.facts.find((fact) => fact.key === 'error')?.tone).toBe('danger')
  })
})
