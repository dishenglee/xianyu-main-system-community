import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { MemoryRouter, useNavigate } from 'react-router-dom'

const { storage } = vi.hoisted(() => {
  const storage = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, String(value)),
      removeItem: (key: string) => storage.delete(key),
      clear: () => storage.clear(),
    },
  })
  return { storage }
})
import { SensitiveItemImage, SensitiveScopeProvider, SensitiveValue, SensitiveVisibilityToggle, maskSensitiveText, resolveSensitiveScope, useSensitiveAvatarInitial } from './SensitiveValue'
import { SENSITIVE_VISIBILITY_STORAGE_KEY, useSensitiveVisibilityStore } from '../store/sensitiveVisibilityStore'

describe('SensitiveValue', () => {
  beforeEach(() => {
    storage.clear()
    window.history.replaceState({}, '', '/')
    useSensitiveVisibilityStore.setState({ hiddenByScope: {} })
  })

  it('uses kind-specific masks through the shared toggle', async () => {
    const user = userEvent.setup()
    render(<>
      <SensitiveVisibilityToggle compact />
      <SensitiveValue kind="buyer" value="张三" title="张三" />
      <SensitiveValue kind="item-title" value="年轻学生" title="年轻学生" />
      <SensitiveValue kind="phone" value="13800138000" />
      <SensitiveValue kind="order-id" value="#202607280001" />
      <SensitiveItemImage src="/item.jpg" alt="商品图" />
    </>)

    expect(screen.getByText('张三')).toBeTruthy()
    expect(screen.getByText('年轻学生')).toBeTruthy()
    const toggle = screen.getByRole('button', { name: '隐藏敏感信息' })
    expect(toggle.textContent).toBe('')
    expect(toggle.classList.contains('sensitive-visibility-toggle')).toBe(true)
    expect(screen.getByText('张三').classList.contains('sensitive-visibility-toggle')).toBe(false)
    await user.click(toggle)
    expect(screen.queryByText('张三')).toBeNull()
    expect(screen.queryByText('年轻学生')).toBeNull()
    expect(screen.getByText('年******生')).toBeTruthy()
    expect(screen.getByText('138****8000')).toBeTruthy()
    expect(screen.getByText('#202******0001')).toBeTruthy()
    expect(screen.getByText('年******生').getAttribute('title')).toBe('年******生')
    expect(screen.getByText('138****8000').getAttribute('title')).toBe('138****8000')
    expect(screen.getByText('#202******0001').getAttribute('title')).toBe('#202******0001')
    const shownToggle = screen.getByRole('button', { name: '显示敏感信息' })
    expect(shownToggle.getAttribute('aria-pressed')).toBe('true')
    expect(shownToggle.classList.contains('sensitive-visibility-toggle')).toBe(true)
    expect(screen.getByRole('img', { name: '商品图' }).getAttribute('data-sensitive-hidden')).toBe('true')
    expect(storage.get(SENSITIVE_VISIBILITY_STORAGE_KEY)).toContain('hiddenByScope')
  })

  it('never reveals both ends of short text or identifiers', () => {
    expect(maskSensitiveText('account', '张三')).toBe('张******')
    expect(maskSensitiveText('buyer', 'AB')).toBe('A******')
    expect(maskSensitiveText('item-title', '书')).toBe('••••••')
    expect(maskSensitiveText('uid', '1234')).toBe('1******')
    expect(maskSensitiveText('order-id', '#12')).toBe('#1******')
  })

  it('replaces avatar fallback initials while hidden', () => {
    function AvatarInitial() {
      return <span>{useSensitiveAvatarInitial('敏感账号')}</span>
    }

    render(<AvatarInitial />)
    expect(screen.getByText('敏')).toBeTruthy()
    act(() => useSensitiveVisibilityStore.setState({ hiddenByScope: { app: true } }))
    expect(screen.getByText('•')).toBeTruthy()
  })

  it('keeps visibility isolated by route family while sharing the same component API', async () => {
    const user = userEvent.setup()
    window.history.replaceState({}, '', '/accounts')
    const { rerender } = render(<><SensitiveVisibilityToggle compact /><SensitiveValue kind="account" value="账号甲" /></>)

    await user.click(screen.getByRole('button', { name: '隐藏敏感信息' }))
    expect(screen.getByText('账******甲')).toBeTruthy()
    expect(useSensitiveVisibilityStore.getState().hiddenByScope).toEqual({ accounts: true })

    window.history.replaceState({}, '', '/orders')
    rerender(<><SensitiveVisibilityToggle compact /><SensitiveValue kind="buyer" value="买家乙" /></>)
    expect(resolveSensitiveScope()).toBe('orders')
    expect(screen.getByText('买家乙')).toBeTruthy()
    expect(screen.getByRole('button', { name: '隐藏敏感信息' })).toBeTruthy()
  })

  it('updates the scope for persistent components when React Router navigates', async () => {
    const user = userEvent.setup()
    function RouteFixture() {
      const navigate = useNavigate()
      return (
        <>
          <SensitiveVisibilityToggle compact />
          <SensitiveValue kind="account" value="账号甲" />
          <button type="button" onClick={() => navigate('/orders')}>前往订单</button>
        </>
      )
    }

    render(
      <MemoryRouter initialEntries={['/accounts']}>
        <SensitiveScopeProvider><RouteFixture /></SensitiveScopeProvider>
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: '隐藏敏感信息' }))
    expect(screen.getByText('账******甲')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: '前往订单' }))
    expect(screen.getByText('账号甲')).toBeTruthy()
    expect(screen.getByRole('button', { name: '隐藏敏感信息' })).toBeTruthy()
  })
})
