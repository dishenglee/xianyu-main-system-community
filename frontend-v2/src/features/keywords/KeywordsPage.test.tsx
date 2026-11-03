import type { ReactNode } from 'react'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const {
  accountStoreMock,
  apiMocks,
  notifyMock,
  loadStoredAccountSelectionMock,
  saveStoredAccountSelectionMock,
} = vi.hoisted(() => ({
  accountStoreMock: {
    accounts: [] as Array<Record<string, unknown>>,
    loadAccounts: vi.fn().mockResolvedValue([]),
  },
  apiMocks: {
    getKeywords: vi.fn(),
    createKeyword: vi.fn(),
    updateKeyword: vi.fn(),
    deleteKeyword: vi.fn(),
    updateAccountSetting: vi.fn(),
  },
  notifyMock: vi.fn(),
  loadStoredAccountSelectionMock: vi.fn(),
  saveStoredAccountSelectionMock: vi.fn(),
}))

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../api/index', () => apiMocks)
vi.mock('../../components/TopBar', () => ({
  TopBar: ({ title }: { title: string }) => <div>{title}</div>,
}))
vi.mock('../../components/HubTabs', () => ({
  HubTabs: () => null,
  HUB_PRESETS: { automation: [] },
}))
vi.mock('../../components/Modal', () => ({
  Modal: ({ open, title, children }: { open: boolean; title: string; children: ReactNode }) => (
    open ? <div><div>{title}</div>{children}</div> : null
  ),
}))
vi.mock('../../components/StatusBadge', () => ({
  StatusBadge: ({ label }: { label: string }) => <span>{label}</span>,
}))
vi.mock('../../components/Notify', () => ({
  useSafeNotify: () => notifyMock,
}))
vi.mock('../../components/AppDialog', () => ({
  useAppDialog: () => ({ showConfirm: vi.fn().mockResolvedValue(true) }),
}))
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false, isDesktop: true, device: 'desktop' }),
}))
vi.mock('../automation/accountSelectionStorage', () => ({
  loadStoredAccountSelection: loadStoredAccountSelectionMock,
  saveStoredAccountSelection: saveStoredAccountSelectionMock,
}))
vi.mock('../../hooks/useAuthScope', () => ({
  useAuthScope: () => ({
    authToken: 'token-a',
    captureScope: (key = '') => ({ token: 'token-a', key }),
    isScopeCurrent: (scope: { key: string }, key = scope.key) => scope.key === key,
  }),
}))

import KeywordsPage from './KeywordsPage'

function renderPage() {
  return render(
    <MemoryRouter>
      <KeywordsPage />
    </MemoryRouter>,
  )
}

function accountSections() {
  const pending = screen.getByText('待配置账号').closest('section')
  const configured = screen.getByText('已配置账号').closest('section')
  if (!pending || !configured) throw new Error('账号配置分区未渲染')
  return { pending, configured }
}

describe('KeywordsPage account configuration', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    accountStoreMock.accounts = []
    accountStoreMock.loadAccounts.mockResolvedValue([])
    loadStoredAccountSelectionMock.mockReturnValue([])
    apiMocks.getKeywords.mockResolvedValue([])
    apiMocks.createKeyword.mockResolvedValue({ status: 'ok' })
    apiMocks.updateKeyword.mockResolvedValue({ status: 'ok' })
    apiMocks.deleteKeyword.mockResolvedValue({ status: 'ok' })
    apiMocks.updateAccountSetting.mockResolvedValue({ status: 'ok' })
  })

  it('uses the responsive split and moves only the selected pending account after keyword mode is saved', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-new', nick: '待配置店铺', status: 'active', replyMode: 'off' },
      { uid: 'uid-keyword', nick: '关键词店铺', status: 'active', replyMode: 'keyword' },
      { uid: 'uid-default', nick: '默认回复店铺', status: 'active', replyMode: 'default' },
      { uid: 'uid-ai', nick: 'AI 店铺', status: 'active', replyMode: 'ai' },
    ]
    renderPage()

    const { pending, configured } = accountSections()
    expect(screen.getByTestId('account-configuration-split').className).toContain('lg:grid-cols-2')
    expect(screen.getByTestId('pending-account-grid').className).toContain('grid-cols-2')
    expect(screen.getByTestId('configured-account-grid').className).toContain('grid-cols-2')
    expect(within(configured).getByText('关键词店铺')).toBeTruthy()
    expect(within(configured).queryByText('默认回复店铺')).toBeNull()
    expect(within(configured).queryByText('AI 店铺')).toBeNull()
    expect(within(pending).getByText('待配置店铺')).toBeTruthy()

    fireEvent.click(within(pending).getByRole('checkbox', { name: '选择待配置账号 待配置店铺' }))
    fireEvent.click(within(pending).getByRole('button', { name: '保存配置' }))

    await waitFor(() => {
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledWith('uid-new', { replyMode: 'keyword' })
      expect(within(pending).queryByText('待配置店铺')).toBeNull()
      expect(within(configured).getByText('待配置店铺')).toBeTruthy()
    })
    expect(apiMocks.createKeyword).not.toHaveBeenCalled()
    expect(saveStoredAccountSelectionMock).toHaveBeenCalledWith('keywords', ['uid-keyword', 'uid-new'])
  })

  it('deletes every rule for the configured UID before turning keyword mode off and moving it left', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-1', nick: '已配置店铺', status: 'active', replyMode: 'keyword' },
    ]
    apiMocks.getKeywords.mockImplementation(async (cookieId: string) => (
      cookieId === 'uid-1'
        ? [
            { id: 31, cookieId: 'uid-1', keyword: '发货', reply: '马上发' },
            { id: 32, cookie_id: 'uid-1', keyword: '优惠', reply: '已是最低价' },
          ]
        : []
    ))
    renderPage()

    const { pending, configured } = accountSections()
    expect(await within(configured).findByText('已配置店铺')).toBeTruthy()
    fireEvent.click(within(configured).getByRole('button', { name: '移除已配置账号 已配置店铺' }))

    await waitFor(() => {
      expect(apiMocks.deleteKeyword).toHaveBeenCalledWith(31)
      expect(apiMocks.deleteKeyword).toHaveBeenCalledWith(32)
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledWith('uid-1', { replyMode: 'off' })
      expect(within(configured).queryByText('已配置店铺')).toBeNull()
      expect(within(pending).getByText('已配置店铺')).toBeTruthy()
    })
    expect(saveStoredAccountSelectionMock).toHaveBeenCalledWith('keywords', [])
  })

  it('moves successful accounts right and keeps failed accounts selected for retry', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-ok', nick: '成功店铺', status: 'active', replyMode: 'off' },
      { uid: 'uid-fail', nick: '失败店铺', status: 'active', replyMode: 'off' },
    ]
    apiMocks.updateAccountSetting.mockImplementation(async (uid: string) => {
      if (uid === 'uid-fail') throw new Error('启用失败')
      return { status: 'ok' }
    })
    renderPage()

    const { pending, configured } = accountSections()
    fireEvent.click(within(pending).getByRole('checkbox', { name: '选择待配置账号 成功店铺' }))
    fireEvent.click(within(pending).getByRole('checkbox', { name: '选择待配置账号 失败店铺' }))
    fireEvent.click(within(pending).getByRole('button', { name: '保存配置' }))

    await waitFor(() => {
      expect(within(configured).getByText('成功店铺')).toBeTruthy()
      expect(within(pending).getByText('失败店铺')).toBeTruthy()
      expect((within(pending).getByRole('checkbox', { name: '选择待配置账号 失败店铺' }) as HTMLInputElement).checked).toBe(true)
    })
    expect(notifyMock).toHaveBeenCalledWith('已配置 1 个账号，1 个失败并保留待重试', 'error')
  })

  it('keeps the account configured when deleting one of its rules fails', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-1', nick: '失败保留店铺', status: 'active', replyMode: 'keyword' },
    ]
    apiMocks.getKeywords.mockImplementation(async (cookieId: string) => (
      cookieId === 'uid-1'
        ? [{ id: 41, cookieId: 'uid-1', keyword: '发货', reply: '马上发' }]
        : []
    ))
    apiMocks.deleteKeyword.mockRejectedValue(new Error('删除失败'))
    renderPage()

    const { pending, configured } = accountSections()
    fireEvent.click(await within(configured).findByRole('button', { name: '移除已配置账号 失败保留店铺' }))

    await waitFor(() => expect(notifyMock).toHaveBeenCalledWith('仍有 1 条关键词规则删除失败', 'error'))
    expect(apiMocks.updateAccountSetting).not.toHaveBeenCalled()
    expect(within(configured).getByText('失败保留店铺')).toBeTruthy()
    expect(within(pending).queryByText('失败保留店铺')).toBeNull()
    expect(saveStoredAccountSelectionMock).not.toHaveBeenCalled()
  })
})
