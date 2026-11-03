import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useAuthStore } from '../../store/authStore'
import { loadStoredAccountSelection, saveStoredAccountSelection } from './accountSelectionStorage'

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

describe('accountSelectionStorage', () => {
  beforeEach(() => {
    storage.clear()
    useAuthStore.setState({ token: '', user: null, _hydrated: true })
  })

  it('keeps keyword and item-reply selections isolated for the same user', () => {
    useAuthStore.setState({
      token: 'token-a',
      user: { username: 'owner-a', role: 'admin' },
      _hydrated: true,
    })

    saveStoredAccountSelection('keywords', ['kw-1', 'kw-2'])
    saveStoredAccountSelection('item-replies', ['ir-1'])

    expect(loadStoredAccountSelection('keywords')).toEqual(['kw-1', 'kw-2'])
    expect(loadStoredAccountSelection('item-replies')).toEqual(['ir-1'])
  })

  it('scopes saved selections by authenticated user', () => {
    useAuthStore.setState({
      token: 'token-a',
      user: { username: 'owner-a', role: 'admin' },
      _hydrated: true,
    })
    saveStoredAccountSelection('keywords', ['user-a'])

    useAuthStore.setState({
      token: 'token-b',
      user: { username: 'owner-b', role: 'admin' },
      _hydrated: true,
    })
    saveStoredAccountSelection('keywords', ['user-b'])

    expect(loadStoredAccountSelection('keywords')).toEqual(['user-b'])

    useAuthStore.setState({
      token: 'token-a',
      user: { username: 'owner-a', role: 'admin' },
      _hydrated: true,
    })
    expect(loadStoredAccountSelection('keywords')).toEqual(['user-a'])
  })
})
