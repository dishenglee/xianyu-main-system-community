import { describe, expect, it } from 'vitest'
import { getAppRuntimeMode, isDebugRuntimeMode } from './runtimeMode'

describe('runtime mode', () => {
  it('defaults to production when no explicit runtime mode is provided', () => {
    expect(getAppRuntimeMode({})).toBe('production')
    expect(isDebugRuntimeMode({})).toBe(false)
  })

  it('enables debug mode only for the explicit debug runtime flag', () => {
    expect(getAppRuntimeMode({ VITE_APP_RUNTIME_MODE: 'debug' })).toBe('debug')
    expect(isDebugRuntimeMode({ VITE_APP_RUNTIME_MODE: 'debug' })).toBe(true)
    expect(getAppRuntimeMode({ VITE_APP_RUNTIME_MODE: 'production' })).toBe('production')
    expect(isDebugRuntimeMode({ VITE_APP_RUNTIME_MODE: 'production' })).toBe(false)
  })
})
