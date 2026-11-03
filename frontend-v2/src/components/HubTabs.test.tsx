import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { HubTabs, HUB_PRESETS } from './HubTabs'
import { useAuthStore } from '../store/authStore'

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

describe('HubTabs local single-user visibility', () => {
  beforeEach(() => {
    storage.clear()
    useAuthStore.setState({ token: '', user: null, _hydrated: true })
    Object.defineProperty(window, 'matchMedia', {
      configurable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    })
  })

  it('keeps local admin tools available when legacy login code attempts to switch users', () => {
    useAuthStore.getState().setAuth('token-user', { username: 'admin', role: 'user' })

    render(
      <MemoryRouter initialEntries={['/accounts']}>
        <HubTabs tabs={HUB_PRESETS.accounts} />
      </MemoryRouter>,
    )

    expect(screen.getByRole('link', { name: '账号列表' })).toBeTruthy()
    expect(screen.getByRole('link', { name: '网络代理' })).toBeTruthy()
  })

  it('shows proxy tab to role admin users', () => {
    useAuthStore.getState().setAuth('token-admin', { username: 'admin', role: 'admin' })

    render(
      <MemoryRouter initialEntries={['/accounts']}>
        <HubTabs tabs={HUB_PRESETS.accounts} />
      </MemoryRouter>,
    )

    expect(screen.getByRole('link', { name: '网络代理' })).toBeTruthy()
  })

  it('places 消息通知 under messages instead of automation', () => {
    expect(HUB_PRESETS.messages.map((tab) => tab.to)).toContain('/notifications')
    expect(HUB_PRESETS.automation.map((tab) => tab.to)).not.toContain('/notifications')
  })
})
