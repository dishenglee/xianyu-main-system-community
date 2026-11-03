export type RuntimeModeEnv = {
  VITE_APP_RUNTIME_MODE?: string
  [key: string]: unknown
}

export type AppRuntimeMode = 'production' | 'debug'

export function getAppRuntimeMode(env: RuntimeModeEnv = import.meta.env): AppRuntimeMode {
  return String(env.VITE_APP_RUNTIME_MODE ?? '').trim().toLowerCase() === 'debug'
    ? 'debug'
    : 'production'
}

export function isDebugRuntimeMode(env: RuntimeModeEnv = import.meta.env): boolean {
  return getAppRuntimeMode(env) === 'debug'
}
