import { isTauri as isOfficialTauri } from '@tauri-apps/api/core'

export type TauriRuntimeWindow = Window & {
  __TAURI_INTERNALS__?: unknown
  isTauri?: boolean
}

function getRuntimeWindow(): TauriRuntimeWindow | undefined {
  if (typeof window === 'undefined') return undefined
  return window as TauriRuntimeWindow
}

export function isTauriRuntime(win: TauriRuntimeWindow | undefined = getRuntimeWindow()): boolean {
  if (!win) return false
  if ('__TAURI_INTERNALS__' in win) return true
  if (typeof win.isTauri === 'boolean') return win.isTauri
  try {
    return isOfficialTauri()
  } catch {
    return false
  }
}
