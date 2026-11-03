import { isTauriRuntime } from './tauriRuntime'

export function buildBackendUrl(path: string): string {
  return path
}

export async function buildBackendWsUrl(path: string): Promise<string> {
  if (isTauriRuntime()) {
    const { invoke } = await import('@tauri-apps/api/core')
    const port = await invoke<number>('get_backend_port')
    return `ws://127.0.0.1:${port}${path}`
  }

  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const host = window.location.host
  return `${protocol}//${host}${path}`
}

export function getBuildProfile(): string {
  return (import.meta as unknown as { env: Record<string, string> }).env?.VITE_BUILD_PROFILE || 'desktop-full'
}

export function isFeatureEnabled(_feature: string, _profile?: string): boolean {
  return true
}
