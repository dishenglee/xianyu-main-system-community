import { act, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import CommandPalette from './CommandPalette'
import { useAuthStore } from '../store/authStore'

const { searchMock, loadAccountsMock, storage } = vi.hoisted(() => {
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
  return {
    searchMock: vi.fn(),
    loadAccountsMock: vi.fn(),
    storage,
  }
})

vi.mock('../hooks/useAccounts', () => ({
  useAccounts: () => ({
    accounts: [{ uid: 'uid-a', nick: '账号A', status: 'active' }],
    loadAccounts: loadAccountsMock,
  }),
}))

vi.mock('../api/index', () => ({
  search: (...args: unknown[]) => searchMock(...args),
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

describe('CommandPalette auth scope', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    searchMock.mockReset()
    loadAccountsMock.mockReset()
    storage.clear()
    useAuthStore.setState({ token: '', user: null, _hydrated: true })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('does not render stale search results after auth scope changes', async () => {
    const pending = deferred<{ items: Array<{ itemId: string; title: string }> }>()
    searchMock.mockReturnValueOnce(pending.promise)

    useAuthStore.getState().setAuth('token-a', { username: 'user-a', role: 'user' })

    render(
      <MemoryRouter>
        <CommandPalette />
      </MemoryRouter>,
    )

    await act(async () => {
      fireEvent.keyDown(window, { key: 'k', ctrlKey: true })
    })
    const input = screen.getByPlaceholderText('搜索商品 · Tab 切类型 · Esc 关闭')
    fireEvent.change(input, { target: { value: '硬盘' } })

    await act(async () => {
      await vi.advanceTimersByTimeAsync(300)
    })
    expect(searchMock).toHaveBeenCalledWith('uid-a', '硬盘', { page: 1 })

    act(() => {
      useAuthStore.getState().setAuth('token-b', { username: 'user-b', role: 'user' })
    })
    await act(async () => {
      pending.resolve({ items: [{ itemId: 'old-item', title: '旧用户商品' }] })
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(screen.queryByText('旧用户商品')).toBeNull()
  })
})
