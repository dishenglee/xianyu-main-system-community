// @ts-nocheck
import { waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const runtimeProfileMocks = vi.hoisted(() => ({
  buildBackendWsUrl: vi.fn(async () => 'ws://127.0.0.1:64797/api/live/ws'),
}))

const clientMocks = vi.hoisted(() => ({
  getAuthToken: vi.fn(() => 'user-jwt'),
}))

const diagnosticMocks = vi.hoisted(() => ({
  buildLiveDiagnosticContext: vi.fn(() => ({ matched: true })),
  maybeRefreshLiveDiagnosticPolicy: vi.fn(),
  shouldLogLiveDiagnostics: vi.fn(() => true),
}))

const runtimeMocks = vi.hoisted(() => ({
  createClientTraceId: vi.fn(() => 'trace-live-1'),
  logRuntimeEvent: vi.fn(),
  logRuntimeWarn: vi.fn(),
}))

const actionLogMocks = vi.hoisted(() => ({
  emitActionLog: vi.fn(),
}))

vi.mock('../../runtimeProfile', () => ({
  buildBackendWsUrl: runtimeProfileMocks.buildBackendWsUrl,
}))

vi.mock('../../api/client', () => ({
  getAuthToken: clientMocks.getAuthToken,
}))

vi.mock('../../bootstrap/actionLogging', () => ({
  emitActionLog: actionLogMocks.emitActionLog,
}))

vi.mock('../../observability/runtime', () => ({
  createClientTraceId: runtimeMocks.createClientTraceId,
  logRuntimeEvent: runtimeMocks.logRuntimeEvent,
  logRuntimeWarn: runtimeMocks.logRuntimeWarn,
}))

vi.mock('./liveDiagnostics', () => ({
  buildLiveDiagnosticContext: diagnosticMocks.buildLiveDiagnosticContext,
  maybeRefreshLiveDiagnosticPolicy: diagnosticMocks.maybeRefreshLiveDiagnosticPolicy,
  shouldLogLiveDiagnostics: diagnosticMocks.shouldLogLiveDiagnostics,
}))

class FakeWebSocket {
  static OPEN = 1
  static instances: FakeWebSocket[] = []

  url: string
  readyState = 0
  sent: string[] = []
  onopen?: () => void
  onmessage?: (event: { data: string }) => void
  onerror?: (event: unknown) => void
  onclose?: (event: { code: number; reason: string }) => void

  constructor(url: string) {
    this.url = url
    FakeWebSocket.instances.push(this)
  }

  send(payload: string) {
    this.sent.push(payload)
  }

  close() {
    this.readyState = 3
  }
}

describe('createLiveTransport', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.clearAllMocks()
    FakeWebSocket.instances = []
    vi.stubGlobal('WebSocket', FakeWebSocket)
    Object.defineProperty(window, 'WebSocket', {
      configurable: true,
      value: FakeWebSocket,
    })
  })

  it('emits structured action logs for transport lifecycle events', async () => {
    const onEvent = vi.fn()
    const { createLiveTransport } = await import('./useLiveTransport')

    const transport = createLiveTransport({
      authToken: 'user-jwt',
      onEvent,
    })

    transport.connect()

    await waitFor(() => {
      expect(runtimeProfileMocks.buildBackendWsUrl).toHaveBeenCalledWith('/api/live/ws')
      expect(FakeWebSocket.instances).toHaveLength(1)
    })
    expect(FakeWebSocket.instances[0].url).toBe('ws://127.0.0.1:64797/api/live/ws?token=user-jwt')

    const socket = FakeWebSocket.instances[0]
    socket.readyState = FakeWebSocket.OPEN
    socket.onopen?.()
    expect(actionLogMocks.emitActionLog).toHaveBeenCalledWith(expect.objectContaining({
      module: 'messages-live-transport',
      action: 'connect_ws',
      outcome: 'success',
    }))
    socket.onmessage?.({
      data: JSON.stringify({
        type: 'message.received',
        uid: 'uid-1',
        sessionId: 'sid-1',
        text: '这里不应该上传原文',
      }),
    })

    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({
      type: 'message.received',
      uid: 'uid-1',
      sessionId: 'sid-1',
    }))
    socket.onmessage?.({
      data: JSON.stringify({
        type: 'system.disconnect',
        reason: 'server restart',
      }),
    })
    expect(actionLogMocks.emitActionLog).toHaveBeenCalledWith(expect.objectContaining({
      module: 'messages-live-transport',
      action: 'system_disconnect',
      outcome: 'failed',
    }))
  })
})
