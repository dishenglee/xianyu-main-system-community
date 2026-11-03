import { act, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { resetAccountsCacheForTest } from '../../hooks/useAccounts'
import { SuperPolishRouteLayout } from './SuperPolishRouteLayout'
import { useSuperPolishScope } from './hooks/useSuperPolishScope'

const getAccountsMock = vi.fn()
const getSuperPolishAccountInfoMock = vi.fn()

vi.mock('../../api/index', () => ({
  getAccounts: (...args: unknown[]) => getAccountsMock(...args),
  deleteAccount: vi.fn(),
  refreshAccount: vi.fn(),
}))

vi.mock('../../api/superPolish', () => ({
  getSuperPolishAccountInfo: (...args: unknown[]) => getSuperPolishAccountInfoMock(...args),
  unwrapMtopData: vi.fn((payload: unknown) => (payload as { data?: unknown })?.data ?? payload),
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

function ScopeProbe() {
  const { uid } = useSuperPolishScope()
  return <div>当前 UID：{uid || '空'}</div>
}

describe('SuperPolishRouteLayout', () => {
  const storage = new Map<string, string>()

  beforeEach(() => {
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, String(value))
      },
      removeItem: (key: string) => {
        storage.delete(key)
      },
      clear: () => {
        storage.clear()
      },
    })
    storage.clear()
    getAccountsMock.mockReset()
    getSuperPolishAccountInfoMock.mockReset()
    getSuperPolishAccountInfoMock.mockResolvedValue({ data: { usableBalance: 168, totalBalance: 200 } })
    resetAccountsCacheForTest()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('waits for the first account load before rendering child routes', async () => {
    const pending = deferred<Array<{ uid: string; nick: string; status: string }>>()
    getAccountsMock.mockReturnValueOnce(pending.promise)

    render(
      <MemoryRouter initialEntries={['/super-polish/campaigns/new']}>
        <Routes>
          <Route path="/super-polish" element={<SuperPolishRouteLayout />}>
            <Route path="campaigns/new" element={<ScopeProbe />} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByText('正在加载可用账号，完成后会自动进入超级擦亮工作区。')).toBeTruthy()
    expect(screen.getByText('加载账号中…')).toBeTruthy()
    expect(screen.queryByText('暂无可用账号')).toBeNull()
    expect(screen.queryByText('当前 UID：u1')).toBeNull()

    await act(async () => {
      pending.resolve([{ uid: 'u1', nick: '账号甲', status: 'active' }])
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(await screen.findByText('当前 UID：u1')).toBeTruthy()
    expect(screen.getByRole('option', { name: '账号甲' })).toBeTruthy()
    expect(await screen.findByText('可用余额')).toBeTruthy()
    expect(screen.getByText('¥1.68')).toBeTruthy()
    expect(screen.getByText('¥2')).toBeTruthy()
    expect(getSuperPolishAccountInfoMock).toHaveBeenCalledWith('u1')
  })
})
