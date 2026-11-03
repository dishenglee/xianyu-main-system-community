import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

let appModuleLoaded = false
let preloadRecoveryLoaded = false
let authStoreLoaded = false
let clientLoggingLoaded = false
const renderSpy = vi.fn()

vi.mock('./App.tsx', () => {
  appModuleLoaded = true
  return { default: () => null }
})

vi.mock('./bootstrap/vitePreloadRecovery', () => {
  preloadRecoveryLoaded = true
  return {
    attachVitePreloadRecovery: vi.fn(),
  }
})

vi.mock('./bootstrap/clientLogging', () => {
  clientLoggingLoaded = true
  return {
    installGlobalClientLogging: vi.fn(),
  }
})

vi.mock('./store/authStore', () => {
  authStoreLoaded = true
  return {
    useAuthStore: {
      setState: vi.fn(),
    },
  }
})

vi.mock('react-dom/client', () => ({
  createRoot: vi.fn(() => ({
    render: renderSpy,
  })),
}))

const tauriInternalsDescriptor = Object.getOwnPropertyDescriptor(window, '__TAURI_INTERNALS__')
const isTauriDescriptor = Object.getOwnPropertyDescriptor(window, 'isTauri')

async function flushMicrotasks() {
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
}

describe('main entrypoint', () => {
  beforeEach(() => {
    vi.resetModules()
    appModuleLoaded = false
    preloadRecoveryLoaded = false
    authStoreLoaded = false
    clientLoggingLoaded = false
    renderSpy.mockReset()
    document.body.innerHTML = '<div id="root"><span>stale content</span></div>'
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
    delete (window as Window & { isTauri?: boolean }).isTauri
  })

  afterEach(() => {
    document.body.innerHTML = ''
    vi.doUnmock('./tauriRuntime')
    if (tauriInternalsDescriptor) {
      Object.defineProperty(window, '__TAURI_INTERNALS__', tauriInternalsDescriptor)
    } else {
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
    }

    if (isTauriDescriptor) {
      Object.defineProperty(window, 'isTauri', isTauriDescriptor)
    } else {
      delete (window as Window & { isTauri?: boolean }).isTauri
    }
  })

  it('does not load the app runtime in a plain browser', async () => {
    vi.useFakeTimers()

    await import('./main.tsx')
    await flushMicrotasks()

    expect(document.getElementById('root')?.textContent).toContain('正在连接桌面运行时')
    expect(appModuleLoaded).toBe(false)
    expect(preloadRecoveryLoaded).toBe(false)
    expect(authStoreLoaded).toBe(false)

    await vi.advanceTimersByTimeAsync(2100)
    await flushMicrotasks()

    expect(document.getElementById('root')?.textContent).toContain('桌面运行时未就绪')
    expect(appModuleLoaded).toBe(false)

    vi.useRealTimers()
  })

  it('loads the app runtime when the Tauri shell marks the window as tauri', async () => {
    Object.defineProperty(window, 'isTauri', {
      configurable: true,
      value: true,
    })

    await import('./main.tsx')
    await flushMicrotasks()

    expect(appModuleLoaded).toBe(true)
    expect(authStoreLoaded).toBe(true)
    expect(preloadRecoveryLoaded).toBe(true)
    expect(clientLoggingLoaded).toBe(true)
    expect(authStoreLoaded).toBe(true)
  })

})
