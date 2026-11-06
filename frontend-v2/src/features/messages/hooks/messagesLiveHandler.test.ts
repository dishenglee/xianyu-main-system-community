// @ts-nocheck
import { beforeEach, describe, expect, it, vi } from 'vitest'

const liveEventMocks = vi.hoisted(() => ({
  applyLiveEventToHistory: vi.fn((existing: any) => existing),
  applyLiveEventToSessions: vi.fn((sessions: any[]) => sessions),
}))

const diagnosticMocks = vi.hoisted(() => ({
  buildLiveDiagnosticContext: vi.fn(() => ({ matched: true })),
  maybeRefreshLiveDiagnosticPolicy: vi.fn(),
  shouldLogLiveDiagnostics: vi.fn(() => false),
}))

const runtimeMocks = vi.hoisted(() => ({
  logRuntimeEvent: vi.fn(),
}))

vi.mock('../liveEvents', () => ({
  applyLiveEventToHistory: liveEventMocks.applyLiveEventToHistory,
  applyLiveEventToSessions: liveEventMocks.applyLiveEventToSessions,
}))

vi.mock('../liveDiagnostics', () => ({
  buildLiveDiagnosticContext: diagnosticMocks.buildLiveDiagnosticContext,
  maybeRefreshLiveDiagnosticPolicy: diagnosticMocks.maybeRefreshLiveDiagnosticPolicy,
  shouldLogLiveDiagnostics: diagnosticMocks.shouldLogLiveDiagnostics,
}))

vi.mock('../../../observability/runtime', () => ({
  logRuntimeEvent: runtimeMocks.logRuntimeEvent,
}))

describe('createLiveEventHandler', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.clearAllMocks()
  })

  it('does not include message text in development logs', async () => {
    diagnosticMocks.shouldLogLiveDiagnostics.mockReturnValue(true)
    const consoleLog = vi.spyOn(console, 'log').mockImplementation(() => {})
    const { createLiveEventHandler } = await import('./messagesLiveHandler')

    const setSessionsByUid = vi.fn((updater) => updater({ uid_1: [] }))
    const setHistoryByKey = vi.fn((updater) => updater({}))
    const setConnectionStates = vi.fn((updater) => updater({}))

    const handleLiveEvent = createLiveEventHandler({
      selectedSessionIdRef: { current: 'uid_1:sid-1:1' },
      sessionsByUidRef: { current: { uid_1: [{ sessionId: 'sid-1' }] } },
      setSessionsByUid,
      setHistoryByKey,
      setConnectionStates,
    })

    handleLiveEvent({
      type: 'connection.listening',
      uid: 'uid_1',
      sessionId: 'sid-1',
      reason: 'ok',
      snapshot: true,
    })
    handleLiveEvent({
      type: 'message.received',
      uid: 'uid_1',
      sessionId: 'sid-1',
      sessionType: 1,
      text: '买家原文不应该直接进日志',
    })

    expect(consoleLog.mock.calls.flat().join(' ')).not.toContain('买家原文不应该直接进日志')
    expect(setSessionsByUid).toHaveBeenCalledOnce()
    expect(setHistoryByKey).toHaveBeenCalledOnce()
    expect(runtimeMocks.logRuntimeEvent).not.toHaveBeenCalled()
    consoleLog.mockRestore()
  })

  it('stays quiet when the remote diagnostic policy does not match', async () => {
    diagnosticMocks.shouldLogLiveDiagnostics.mockReturnValue(false)
    const { createLiveEventHandler } = await import('./messagesLiveHandler')

    const handleLiveEvent = createLiveEventHandler({
      selectedSessionIdRef: { current: '' },
      sessionsByUidRef: { current: {} },
      setSessionsByUid: vi.fn(),
      setHistoryByKey: vi.fn(),
      setConnectionStates: vi.fn(),
    })

    handleLiveEvent({
      type: 'message.received',
      uid: 'uid_2',
      sessionId: 'sid-2',
      sessionType: 1,
      text: '不应记录',
    })

    expect(runtimeMocks.logRuntimeEvent).not.toHaveBeenCalled()
  })
})
