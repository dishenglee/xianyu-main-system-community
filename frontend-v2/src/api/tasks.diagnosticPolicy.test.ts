// @ts-nocheck
import { beforeEach, describe, expect, it, vi } from 'vitest'

const clientMocks = vi.hoisted(() => ({
  getBaseURL: vi.fn(async () => 'http://127.0.0.1:64797'),
  getAuthToken: vi.fn(() => 'user-jwt'),
  request: vi.fn(),
}))

vi.mock('./client', () => ({
  getBaseURL: clientMocks.getBaseURL,
  getAuthToken: clientMocks.getAuthToken,
  request: clientMocks.request,
}))

vi.mock('./market', () => ({
  search: vi.fn(),
}))

function jsonResponse(status: number, payload: unknown) {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: status === 404 ? 'Not Found' : 'OK',
    json: vi.fn(async () => payload),
  } as unknown as Response
}

describe('getRuntimeDiagnosticPolicy', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.clearAllMocks()
  })

  it('normalizes wrapped camelCase and snake_case policy fields from the primary endpoint', async () => {
    const fetchMock = vi.fn(async () => jsonResponse(200, {
      data: {
        enabled: true,
        level: 'warning',
        modules: ['messages-live'],
        usernames: ['worker-b'],
        device_ids: ['device-1'],
        account_uids: ['uid-1'],
        sessionIds: ['sid-1'],
        starts_at: 1710000000000,
        endsAt: 1710003600000,
        include_payloads: true,
        sample_rate: 0.25,
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const { getRuntimeDiagnosticPolicy } = await import('./tasks')

    await expect(getRuntimeDiagnosticPolicy()).resolves.toEqual({
      enabled: true,
      level: 'WARN',
      modules: ['messages-live'],
      usernames: ['worker-b'],
      deviceIds: ['device-1'],
      accountUids: ['uid-1'],
      sessionIds: ['sid-1'],
      startsAt: 1710000000000,
      endsAt: 1710003600000,
      includePayloads: true,
      sampleRate: 0.25,
      refreshedAt: expect.any(Number),
      source: '/api/logs/diagnostic-policy',
    })

    expect(fetchMock).toHaveBeenCalledWith(
      'http://127.0.0.1:64797/api/logs/diagnostic-policy',
      expect.objectContaining({
        headers: {
          Accept: 'application/json',
        },
      }),
    )
  })

  it('normalizes second-based timestamps from the backend into millisecond windows', async () => {
    const fetchMock = vi.fn(async () => jsonResponse(200, {
      data: {
        enabled: true,
        modules: ['messages-live'],
        starts_at: 1710000000,
        ends_at: 1710003600,
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const { getRuntimeDiagnosticPolicy } = await import('./tasks')

    await expect(getRuntimeDiagnosticPolicy()).resolves.toEqual({
      enabled: true,
      level: 'DEBUG',
      modules: ['messages-live'],
      usernames: [],
      deviceIds: [],
      accountUids: [],
      sessionIds: [],
      startsAt: 1710000000 * 1000,
      endsAt: 1710003600 * 1000,
      includePayloads: false,
      sampleRate: 1,
      refreshedAt: expect.any(Number),
      source: '/api/logs/diagnostic-policy',
    })
  })

  it('tries fallback endpoints quietly and returns a disabled policy when the backend route is absent', async () => {
    const fetchMock = vi.fn(async () => jsonResponse(404, {}))
    vi.stubGlobal('fetch', fetchMock)

    const { getRuntimeDiagnosticPolicy } = await import('./tasks')

    await expect(getRuntimeDiagnosticPolicy()).resolves.toEqual({
      enabled: false,
      level: 'DEBUG',
      modules: [],
      usernames: [],
      deviceIds: [],
      accountUids: [],
      sessionIds: [],
      startsAt: null,
      endsAt: null,
      includePayloads: false,
      sampleRate: 1,
      refreshedAt: expect.any(Number),
      source: '',
    })

    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      'http://127.0.0.1:64797/api/logs/diagnostic-policy',
      'http://127.0.0.1:64797/api/logs/diagnostics/policy',
    ])
  })

  it('falls back to the scoped client endpoint when the admin endpoint returns 403', async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/api/logs/diagnostic-policy')) {
        return jsonResponse(403, { detail: 'forbidden' })
      }
      return jsonResponse(200, {
        data: {
          enabled: true,
          modules: ['messages-live'],
          includePayloads: false,
        },
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const { getRuntimeDiagnosticPolicy } = await import('./tasks')

    await expect(getRuntimeDiagnosticPolicy()).resolves.toEqual({
      enabled: true,
      level: 'DEBUG',
      modules: ['messages-live'],
      usernames: [],
      deviceIds: [],
      accountUids: [],
      sessionIds: [],
      startsAt: null,
      endsAt: null,
      includePayloads: false,
      sampleRate: 1,
      refreshedAt: expect.any(Number),
      source: '/api/logs/diagnostics/policy',
    })
  })
})
