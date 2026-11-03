export const VITE_PRELOAD_RELOAD_KEY = 'xianyu:vite-preload-reload-at'
export const VITE_PRELOAD_RELOAD_COOLDOWN_MS = 15_000

interface PreloadRecoveryStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

interface PreloadRecoveryLocation {
  reload(): void
}

type PreloadRecoveryWindow = {
  addEventListener(type: string, listener: (event: Event) => void): void
  location: PreloadRecoveryLocation
  sessionStorage: PreloadRecoveryStorage
  __xianyuVitePreloadRecoveryBound?: boolean
}

function readLastReloadAt(storage: PreloadRecoveryStorage): number | null {
  const raw = storage.getItem(VITE_PRELOAD_RELOAD_KEY)
  if (!raw) return null
  const value = Number(raw.replace(/_/g, ''))
  return Number.isFinite(value) ? value : null
}

export function handleVitePreloadError(
  win: Pick<PreloadRecoveryWindow, 'location' | 'sessionStorage'>,
  event: Event,
  now = Date.now(),
): 'reloaded' | 'escalated' {
  const lastReloadAt = readLastReloadAt(win.sessionStorage)
  if (lastReloadAt !== null && now - lastReloadAt < VITE_PRELOAD_RELOAD_COOLDOWN_MS) {
    win.sessionStorage.removeItem(VITE_PRELOAD_RELOAD_KEY)
    return 'escalated'
  }

  event.preventDefault()
  win.sessionStorage.setItem(VITE_PRELOAD_RELOAD_KEY, String(now))
  win.location.reload()
  return 'reloaded'
}

export function attachVitePreloadRecovery(win: PreloadRecoveryWindow = window) {
  if (win.__xianyuVitePreloadRecoveryBound) return
  win.__xianyuVitePreloadRecoveryBound = true

  win.addEventListener('vite:preloadError', (event) => {
    handleVitePreloadError(win, event)
  })
}
