// @ts-nocheck
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../../store/authStore', () => ({
  useAuthStore: {
    getState: () => ({
      token: 'user-jwt',
      user: { username: 'worker-a' },
    }),
  },
}))

vi.mock('../../api/client', () => ({
  getAuthToken: () => 'user-jwt',
}))

vi.mock('../license/session', () => ({
  loadSession: () => ({
    license: {
      bound_device_id: 'device-a',
    },
  }),
}))

describe('matchesLiveDiagnosticPolicy', () => {
  beforeEach(() => {
    vi.resetModules()
  })

  it('matches device scoped policies and keeps deterministic sample-rate decisions', async () => {
    const { matchesLiveDiagnosticPolicy } = await import('./liveDiagnostics')

    const policy = {
      enabled: true,
      level: 'DEBUG',
      modules: ['messages-live'],
      usernames: ['worker-a'],
      deviceIds: ['device-a'],
      accountUids: ['uid-1'],
      sessionIds: ['sid-1'],
      startsAt: null,
      endsAt: null,
      includePayloads: true,
      sampleRate: 0.5,
      refreshedAt: Date.now(),
      source: '/api/logs/diagnostics/policy',
    }
    const scope = {
      module: 'messages-live',
      username: 'worker-a',
      deviceId: 'device-a',
      accountUid: 'uid-1',
      sessionId: 'sid-1',
    }

    const first = matchesLiveDiagnosticPolicy(policy, scope)
    const second = matchesLiveDiagnosticPolicy(policy, scope)

    expect(first).toBe(second)
  })

  it('rejects mismatched device scopes even when the rest of the policy matches', async () => {
    const { matchesLiveDiagnosticPolicy } = await import('./liveDiagnostics')

    expect(
      matchesLiveDiagnosticPolicy(
        {
          enabled: true,
          level: 'DEBUG',
          modules: ['messages-live'],
          usernames: ['worker-a'],
          deviceIds: ['device-b'],
          accountUids: ['uid-1'],
          sessionIds: ['sid-1'],
          startsAt: null,
          endsAt: null,
          includePayloads: false,
          sampleRate: 1,
          refreshedAt: Date.now(),
          source: '/api/logs/diagnostics/policy',
        },
        {
          module: 'messages-live',
          username: 'worker-a',
          deviceId: 'device-a',
          accountUid: 'uid-1',
          sessionId: 'sid-1',
        },
      ),
    ).toBe(false)
  })
})
