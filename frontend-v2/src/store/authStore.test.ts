import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useAuthStore } from './authStore'

const { storage } = vi.hoisted(() => {
  const storage = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => { storage.set(key, String(value)) },
      removeItem: (key: string) => { storage.delete(key) },
      clear: () => { storage.clear() },
    },
  })
  return { storage }
})

describe('local single-user auth store', () => {
  beforeEach(() => {
    storage.clear()
    useAuthStore.getState().clearAuth()
  })

  it('always exposes the fixed local admin identity', () => {
    expect(useAuthStore.getState()).toMatchObject({
      token: 'local-single-user',
      user: { username: 'local-app', role: 'admin' },
      _hydrated: true,
    })
  })

  it('does not switch identity when legacy login code calls setAuth', () => {
    useAuthStore.getState().setAuth('remote-user-token', { username: 'remote-user', role: 'user' })

    expect(useAuthStore.getState()).toMatchObject({
      token: 'local-single-user',
      user: { username: 'local-app', role: 'admin' },
    })
  })
})
