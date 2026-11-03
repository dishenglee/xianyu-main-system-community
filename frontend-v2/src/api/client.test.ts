import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

function jsonResponse(status: number, payload: unknown) {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: status === 401 ? 'Unauthorized' : 'OK',
    json: vi.fn(async () => payload),
  } as unknown as Response
}

function createStorageMock(): Storage {
  const store = new Map<string, string>()
  return {
    get length() {
      return store.size
    },
    clear: vi.fn(() => store.clear()),
    getItem: vi.fn((key: string) => store.get(key) ?? null),
    key: vi.fn((index: number) => Array.from(store.keys())[index] ?? null),
    removeItem: vi.fn((key: string) => store.delete(key)),
    setItem: vi.fn((key: string, value: string) => {
      store.set(key, String(value))
    }),
  }
}

async function flushMicrotasks() {
  await Promise.resolve()
  await Promise.resolve()
}

describe('api request auth recovery', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.clearAllMocks()
    const storage = createStorageMock()
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: storage,
    })
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: storage,
    })
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_backend_port') return 64797
      throw new Error(`unexpected invoke ${command}`)
    })
  })

  it('sends local API requests without product credentials', async () => {
    const fetchMock = vi.fn(async (url: string, _init?: RequestInit) => url.endsWith('/api/qr/generate')
      ? jsonResponse(200, { success: true, qr_code_url: 'data:image/svg+xml;base64,ok' })
      : jsonResponse(200, { status: 'ok' }))
    vi.stubGlobal('fetch', fetchMock)

    const { request } = await import('./client')

    await expect(request('POST', '/api/qr/generate')).resolves.toMatchObject({
      success: true,
    })
    await flushMicrotasks()

    expect(fetchMock.mock.calls.map(([url]) => String(url))).toEqual([
      'http://127.0.0.1:64797/api/qr/generate',
      'http://127.0.0.1:64797/api/logs/client-events',
    ])
    expect(fetchMock.mock.calls[0]?.[1]?.headers).not.toHaveProperty('Authorization')
  })

  it('does not attach stored stale bearer tokens to local requests', async () => {
    localStorage.setItem('auth_token', 'old-jwt')
    const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) => jsonResponse(200, [{ uid: 'uid-1', nick: '账号1' }]))
    vi.stubGlobal('fetch', fetchMock)

    const { request } = await import('./client')

    await expect(request('GET', '/api/accounts')).resolves.toEqual([{ uid: 'uid-1', nick: '账号1' }])

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[1]?.headers).not.toHaveProperty('Authorization')
  })

  it('emits a structured client log for successful mutating requests', async () => {
    localStorage.setItem('auth_token', 'user-jwt')
    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/api/accounts/save')) {
        return jsonResponse(200, { ok: true })
      }
      if (url.endsWith('/api/logs/client-events')) {
        return jsonResponse(200, { status: 'ok' })
      }
      throw new Error(`unexpected url ${url}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    const { request } = await import('./client')

    await expect(request('POST', '/api/accounts/save', { uid: 'u-1' })).resolves.toEqual({ ok: true })
    await flushMicrotasks()

    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      'http://127.0.0.1:64797/api/logs/client-events',
      expect.objectContaining({
        method: 'POST',
        body: expect.stringContaining('"action":"request_ok"'),
      }),
    )
  })

  it('normalizes remote asset URLs in API payloads for the desktop app runtime', async () => {
    localStorage.setItem('auth_token', 'user-jwt')
    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/api/accounts')) {
        return jsonResponse(200, {
          account: {
            avatar: 'http://img.alicdn.com/avatar.png',
            profile: {
              peerLogo: '//gw.alicdn.com/logo.jpg',
            },
            imageUrls: ['http://img.alicdn.com/item-1.jpg'],
            plainUrl: 'http://license.gooofish.shop/health',
          },
        })
      }
      throw new Error(`unexpected url ${url}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    const { request } = await import('./client')

    await expect(request('GET', '/api/accounts')).resolves.toEqual({
      account: {
        avatar: 'https://img.alicdn.com/avatar.png',
        profile: {
          peerLogo: 'https://gw.alicdn.com/logo.jpg',
        },
        imageUrls: ['https://img.alicdn.com/item-1.jpg'],
        plainUrl: 'http://license.gooofish.shop/health',
      },
    })
  })

  it('retries backend port discovery once when the embedded backend is still starting', async () => {
    localStorage.setItem('auth_token', 'user-jwt')
    invokeMock
      .mockRejectedValueOnce(new Error('backend not ready within 30s'))
      .mockResolvedValueOnce(64797)

    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/api/accounts')) {
        return jsonResponse(200, [{ uid: 'uid-1', nick: '账号1' }])
      }
      throw new Error(`unexpected url ${url}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    const { request } = await import('./client')

    await expect(request('GET', '/api/accounts')).resolves.toEqual([{ uid: 'uid-1', nick: '账号1' }])

    expect(invokeMock).toHaveBeenCalledTimes(2)
    expect(fetchMock).toHaveBeenCalledWith(
      'http://127.0.0.1:64797/api/accounts',
      expect.objectContaining({
        method: 'GET',
      }),
    )
  })

  it('does not poison the cached backend base url after an early startup timeout', async () => {
    localStorage.setItem('auth_token', 'user-jwt')
    invokeMock
      .mockRejectedValueOnce(new Error('backend not ready within 30s'))
      .mockRejectedValueOnce(new Error('backend not ready within 30s'))
      .mockResolvedValueOnce(64797)

    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/api/accounts')) {
        return jsonResponse(200, [{ uid: 'uid-1', nick: '账号1' }])
      }
      throw new Error(`unexpected url ${url}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    const { request } = await import('./client')

    await expect(request('GET', '/api/accounts')).rejects.toThrow('backend not ready within 30s')
    await expect(request('GET', '/api/accounts')).resolves.toEqual([{ uid: 'uid-1', nick: '账号1' }])

    expect(invokeMock).toHaveBeenCalledTimes(3)
    expect(fetchMock).toHaveBeenCalledWith(
      'http://127.0.0.1:64797/api/accounts',
      expect.objectContaining({
        method: 'GET',
      }),
    )
  })
})
