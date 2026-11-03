import { describe, expect, it, vi } from 'vitest'
import { buildActionLogEvent, emitActionFailure, emitActionSuccess } from './actionLogging'

vi.mock('../api/client', () => ({
  emitClientLog: vi.fn(),
}))

describe('actionLogging', () => {
  it('builds structured success events for user actions', () => {
    const event = buildActionLogEvent({
      module: 'other-automation',
      action: 'save_schedule',
      outcome: 'success',
      message: '定时任务保存成功',
      context: { scheduleId: 'auto-1' },
    })

    expect(event).toMatchObject({
      level: 'info',
      logger: 'frontend.action',
      module: 'other-automation',
      action: 'save_schedule_success',
      errorCode: 'save_schedule_success',
      message: '定时任务保存成功',
      context: { outcome: 'success', scheduleId: 'auto-1' },
    })
  })

  it('promotes failed user actions to incidents by default', () => {
    const event = buildActionLogEvent({
      module: 'messages',
      action: 'send_item',
      outcome: 'failed',
      message: '发送宝贝失败',
      error: new Error('Failed to fetch'),
      context: { uid: '1729' },
    })

    expect(event).toMatchObject({
      level: 'error',
      action: 'send_item_failed',
      errorCode: 'send_item_failed',
      detail: 'Failed to fetch',
      retryable: true,
      promoteToIncident: true,
    })
  })
})
