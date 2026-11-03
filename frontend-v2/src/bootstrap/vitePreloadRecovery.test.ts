import { describe, expect, it, vi } from 'vitest'
import {
  attachVitePreloadRecovery,
  handleVitePreloadError,
  VITE_PRELOAD_RELOAD_COOLDOWN_MS,
  VITE_PRELOAD_RELOAD_KEY,
} from './vitePreloadRecovery'

function createStorage() {
  const store = new Map<string, string>()
  return {
    getItem(key: string) {
      return store.has(key) ? store.get(key)! : null
    },
    setItem(key: string, value: string) {
      store.set(key, value)
    },
    removeItem(key: string) {
      store.delete(key)
    },
  }
}

function createWindowStub() {
  const handlers: Record<string, Array<(event: Event) => void>> = {}
  const sessionStorage = createStorage()
  return {
    __xianyuVitePreloadRecoveryBound: false,
    sessionStorage,
    location: {
      reload: vi.fn(),
    },
    addEventListener: vi.fn((type: string, listener: (event: Event) => void) => {
      handlers[type] ||= []
      handlers[type].push(listener)
    }),
    handlers,
  }
}

describe('vite preload recovery', () => {
  it('reloads once and prevents the default chunk-load crash on the first preload error', () => {
    const win = createWindowStub()
    const event = new Event('vite:preloadError', { cancelable: true })

    const outcome = handleVitePreloadError(win, event, 1_000)

    expect(outcome).toBe('reloaded')
    expect(event.defaultPrevented).toBe(true)
    expect(win.location.reload).toHaveBeenCalledTimes(1)
    expect(win.sessionStorage.getItem(VITE_PRELOAD_RELOAD_KEY)).toBe('1000')
  })

  it('surfaces the error instead of looping reloads when another preload failure happens immediately after a reload', () => {
    const win = createWindowStub()
    win.sessionStorage.setItem(VITE_PRELOAD_RELOAD_KEY, '1_000')
    const event = new Event('vite:preloadError', { cancelable: true })

    const outcome = handleVitePreloadError(win, event, 1_000 + VITE_PRELOAD_RELOAD_COOLDOWN_MS - 1)

    expect(outcome).toBe('escalated')
    expect(event.defaultPrevented).toBe(false)
    expect(win.location.reload).not.toHaveBeenCalled()
    expect(win.sessionStorage.getItem(VITE_PRELOAD_RELOAD_KEY)).toBeNull()
  })

  it('attaches the global recovery listener only once', () => {
    const win = createWindowStub()

    attachVitePreloadRecovery(win)
    attachVitePreloadRecovery(win)

    expect(win.addEventListener).toHaveBeenCalledTimes(1)
    expect(win.handlers['vite:preloadError']).toHaveLength(1)
  })
})
