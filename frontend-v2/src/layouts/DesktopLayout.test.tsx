import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'
import { DesktopLayout } from './DesktopLayout'

const {
  checkAppUpdateMock,
  getAppVersionInfoMock,
  installAppUpdateMock,
  requestAppRestartMock,
} = vi.hoisted(() => ({
  checkAppUpdateMock: vi.fn(async () => ({
    configured: true,
    policy_configured: true,
    current_version: '0.1.2',
    available: true,
    version: '0.1.3',
    body: '1. 修复消息通知\n2. 优化账号中心',
    date: null,
    minimum_version: null,
    blocked: false,
    block_reason: null,
    download_url: null,
    download_page_url: null,
  })),
  getAppVersionInfoMock: vi.fn(async () => ({ current_version: '0.1.2', update_configured: true })),
  installAppUpdateMock: vi.fn(async () => ({
    mode: 'tauri_updater',
    restart_required: true,
    message: '新版本已安装，应用即将重启。',
    path: null,
  })),
  requestAppRestartMock: vi.fn(async () => undefined),
}))

vi.mock('../hooks/useAccounts', () => ({
  useAccounts: () => ({
    accounts: [
      { uid: 'u1', status: 'active' },
      { uid: 'u2', status: 'restricted' },
      { uid: 'u3', status: 'disabled' },
    ],
    loadAccounts: vi.fn(),
  }),
}))

vi.mock('../features/update/api', () => ({
  checkAppUpdate: checkAppUpdateMock,
  getAppVersionInfo: getAppVersionInfoMock,
  installAppUpdate: installAppUpdateMock,
  requestAppRestart: requestAppRestartMock,
}))

vi.mock('../store/authStore', () => ({
  useAuthStore: (selector: any) => selector({ clearAuth: vi.fn() }),
}))

vi.mock('../components/CommandPalette', () => ({
  default: () => null,
}))

vi.mock('../components/TaskProgressFloat', () => ({
  TaskProgressFloat: () => null,
}))

vi.mock('../components/GlobalMessageNotify', () => ({
  GlobalMessageNotify: ({ startupSessionRefresh }: { startupSessionRefresh?: string }) => (
    <div data-testid="global-message-notify" data-startup-session-refresh={startupSessionRefresh || ''} />
  ),
}))

vi.mock('../routePreload', () => ({
  preloadDesktopRoutesWhenIdle: vi.fn(() => vi.fn()),
}))

describe('DesktopLayout', () => {
  it('keeps non-message pages on the same fixed-height shell model as IM', () => {
    const { container } = render(
      <MemoryRouter initialEntries={['/orders']}>
        <Routes>
          <Route element={<DesktopLayout />}>
            <Route path="/orders" element={<div>orders page</div>} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    const main = container.querySelector('.desktop-shell-main')
    expect(main).toBeTruthy()
    expect(main?.className).toContain('overflow-hidden')
    expect(main?.className).not.toContain('overflow-y-auto')
  })

  it('renders the mascot plus enlarged wordmark in the top-left toolbar area instead of the packaged app icon', () => {
    render(
      <MemoryRouter initialEntries={['/accounts']}>
        <Routes>
          <Route element={<DesktopLayout />}>
            <Route path="/accounts" element={<div>accounts page</div>} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByRole('img', { name: '鱼管家客服吉祥物' })).toBeTruthy()
    expect(screen.getByRole('img', { name: '鱼管家品牌字标' })).toBeTruthy()
    expect(screen.queryByRole('img', { name: '鱼管家应用图标' })).toBeNull()
  })

  it('can navigate away from the promotion tab through the desktop toolbar', () => {
    render(
      <MemoryRouter initialEntries={['/growth']}>
        <Routes>
          <Route element={<DesktopLayout />}>
            <Route path="/growth" element={<div>promotion page</div>} />
            <Route path="/super-polish" element={<div>super polish page</div>} />
            <Route path="/accounts" element={<div>accounts page</div>} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByText('promotion page')).toBeTruthy()
    fireEvent.click(screen.getByRole('link', { name: /账号/ }))
    expect(screen.getByText('accounts page')).toBeTruthy()
  })

  it('shows super polish as a standalone first-level desktop entry', () => {
    render(
      <MemoryRouter initialEntries={['/super-polish']}>
        <Routes>
          <Route element={<DesktopLayout />}>
            <Route path="/super-polish" element={<div>super polish page</div>} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByRole('link', { name: /超级擦亮/ })).toBeTruthy()
    expect(screen.getByText('super polish page')).toBeTruthy()
  })

  it('uses cached-first global unread seeding on desktop to avoid startup request storms', () => {
    render(
      <MemoryRouter initialEntries={['/accounts']}>
        <Routes>
          <Route element={<DesktopLayout />}>
            <Route path="/accounts" element={<div>accounts page</div>} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByTestId('global-message-notify').getAttribute('data-startup-session-refresh')).toBe('cached-first')
  })

  it('checks for updates from the bottom-left version entry and can install immediately', async () => {
    render(
      <MemoryRouter initialEntries={['/accounts']}>
        <Routes>
          <Route element={<DesktopLayout />}>
            <Route path="/accounts" element={<div>accounts page</div>} />
          </Route>
        </Routes>
      </MemoryRouter>,
    )

    const versionButton = await screen.findByRole('button', { name: /发现新版本 0.1.3/ })
    expect(versionButton.querySelector('.desktop-version-dot')).toBeTruthy()

    fireEvent.click(versionButton)
    expect(screen.getByText('发现新版本 v0.1.3')).toBeTruthy()
    expect(screen.getByText('1. 修复消息通知')).toBeTruthy()
    expect(screen.getByText('2. 优化账号中心')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '立即更新' }))

    await waitFor(() => {
      expect(installAppUpdateMock).toHaveBeenCalledWith({ targetVersion: '0.1.3' })
      expect(requestAppRestartMock).toHaveBeenCalledTimes(1)
    })
  })
})
