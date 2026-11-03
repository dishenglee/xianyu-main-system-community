import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { accountStoreMock, apiMocks, notifyMocks, emitAccountsSyncedMock } = vi.hoisted(() => ({
  accountStoreMock: {
    accounts: [] as Array<Record<string, unknown>>,
    loadAccounts: vi.fn().mockResolvedValue([]),
  },
  apiMocks: {
    discoverAiModels: vi.fn(),
    getAiSettings: vi.fn(),
    testAiSettingsConnection: vi.fn(),
    updateAiSettings: vi.fn(),
    updateAccountSetting: vi.fn(),
  },
  notifyMocks: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
  emitAccountsSyncedMock: vi.fn(),
}))

vi.mock('../../api', () => ({
  discoverAiModels: apiMocks.discoverAiModels,
  getAiSettings: apiMocks.getAiSettings,
  testAiSettingsConnection: apiMocks.testAiSettingsConnection,
  updateAiSettings: apiMocks.updateAiSettings,
}))
vi.mock('../../api/accounts', () => ({
  updateAccountSetting: apiMocks.updateAccountSetting,
}))
vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false, isDesktop: true, device: 'desktop' }),
}))
vi.mock('../../components/TopBar', () => ({ TopBar: () => null }))
vi.mock('../../components/HubTabs', () => ({
  HubTabs: () => null,
  HUB_PRESETS: { automation: [] },
}))
vi.mock('../../components/Notify', () => ({
  useNotify: () => notifyMocks,
}))
vi.mock('../../components/AppDialog', () => ({
  useAppDialog: () => ({ showConfirm: vi.fn().mockResolvedValue(true) }),
}))
vi.mock('../runtimeEvents', () => ({ emitAccountsSynced: emitAccountsSyncedMock }))
vi.mock('../../hooks/useAuthScope', () => ({
  useAuthScope: () => ({
    authToken: 'token-a',
    captureScope: (key = '') => ({ token: 'token-a', key }),
    isScopeCurrent: (scope: { key: string }, key = scope.key) => scope.key === key,
  }),
}))

import AiRepliesPage from './AiRepliesPage'

function renderPage() {
  return render(
    <MemoryRouter>
      <AiRepliesPage />
    </MemoryRouter>,
  )
}

function accountSections() {
  const pending = screen.getByText('待配置账号').closest('section')
  const configured = screen.getByText('已配置账号').closest('section')
  if (!pending || !configured) throw new Error('账号配置分区未渲染')
  return { pending, configured }
}

describe('AiRepliesPage account configuration', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    accountStoreMock.accounts = []
    accountStoreMock.loadAccounts.mockResolvedValue([])
    apiMocks.getAiSettings.mockResolvedValue({})
    apiMocks.updateAccountSetting.mockResolvedValue({ status: 'ok' })
  })

  it('places Base URL and API Key on one desktop row and renders three prompt columns', async () => {
    renderPage()
    await screen.findByText('API 类型')

    const connectionGrid = screen.getByTestId('ai-connection-grid')
    expect(connectionGrid.className).toContain('md:grid-cols-2')
    expect(screen.getByText('Base URL').closest('label')?.className).not.toContain('col-span-2')
    expect(screen.getByText('API Key').closest('label')?.className).not.toContain('col-span-2')
    const promptGrid = screen.getByTestId('ai-prompt-grid')
    expect(promptGrid.className).toContain('grid-cols-3')
    expect(within(promptGrid).getByText('违禁词配置')).toBeTruthy()
  })

  it('renders the responsive split and moves only the selected pending account right after AI mode succeeds', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-new', nick: '待配置店铺', status: 'active', replyMode: 'off' },
      { uid: 'uid-enabled', nick: 'AI 开关店铺', status: 'active', aiEnabled: true },
      { uid: 'uid-mode', nick: 'AI 模式店铺', status: 'active', replyMode: 'ai' },
      { uid: 'uid-default', nick: '默认回复店铺', status: 'active', replyMode: 'default', aiEnabled: true },
    ]
    renderPage()

    const { pending, configured } = accountSections()
    expect(screen.getByTestId('account-configuration-split').className).toContain('lg:grid-cols-2')
    expect(screen.getByTestId('pending-account-grid').className).toContain('grid-cols-2')
    expect(screen.getByTestId('configured-account-grid').className).toContain('grid-cols-2')
    expect(within(configured).getByText('AI 开关店铺')).toBeTruthy()
    expect(within(configured).getByText('AI 模式店铺')).toBeTruthy()
    expect(within(configured).queryByText('默认回复店铺')).toBeNull()

    const pendingCheckbox = within(pending).getByRole('checkbox', { name: '选择待配置账号 待配置店铺' }) as HTMLInputElement
    await waitFor(() => expect(pendingCheckbox.disabled).toBe(false))
    fireEvent.click(pendingCheckbox)
    fireEvent.click(within(pending).getByRole('button', { name: '保存配置' }))

    await waitFor(() => {
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledWith('uid-new', { replyMode: 'ai' })
      expect(within(pending).queryByText('待配置店铺')).toBeNull()
      expect(within(configured).getByText('待配置店铺')).toBeTruthy()
    })
    expect(apiMocks.updateAccountSetting).toHaveBeenCalledTimes(1)
  })

  it('turns AI mode off before moving a configured account left', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-ai', nick: '已配置店铺', status: 'active', replyMode: 'ai' },
    ]
    renderPage()

    const { pending, configured } = accountSections()
    const removeButton = await within(configured).findByRole('button', { name: '移除已配置账号 已配置店铺' }) as HTMLButtonElement
    await waitFor(() => expect(removeButton.disabled).toBe(false))
    fireEvent.click(removeButton)

    await waitFor(() => {
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledWith('uid-ai', { replyMode: 'off' })
      expect(within(configured).queryByText('已配置店铺')).toBeNull()
      expect(within(pending).getByText('已配置店铺')).toBeTruthy()
    })
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
    const successCheckbox = within(pending).getByRole('checkbox', { name: '选择待配置账号 成功店铺' }) as HTMLInputElement
    await waitFor(() => expect(successCheckbox.disabled).toBe(false))
    fireEvent.click(successCheckbox)
    fireEvent.click(within(pending).getByRole('checkbox', { name: '选择待配置账号 失败店铺' }))
    fireEvent.click(within(pending).getByRole('button', { name: '保存配置' }))

    await waitFor(() => {
      expect(within(configured).getByText('成功店铺')).toBeTruthy()
      expect(within(pending).getByText('失败店铺')).toBeTruthy()
      expect((within(pending).getByRole('checkbox', { name: '选择待配置账号 失败店铺' }) as HTMLInputElement).checked).toBe(true)
    })
    expect(notifyMocks.error).toHaveBeenCalledWith('已配置 1 个账号，1 个失败并保留待重试')
  })

  it('does not loop model discovery when the automatic request returns no models', async () => {
    apiMocks.getAiSettings.mockResolvedValue({
      apiType: 'openai',
      apiKey: 'test-key',
      baseUrl: 'https://example.invalid/v1',
    })
    apiMocks.discoverAiModels.mockResolvedValue({ models: [] })
    renderPage()

    await waitFor(() => expect(apiMocks.discoverAiModels).toHaveBeenCalledTimes(1), { timeout: 1500 })
    await new Promise((resolve) => window.setTimeout(resolve, 700))
    expect(apiMocks.discoverAiModels).toHaveBeenCalledTimes(1)
  })

  it('keeps a pending account on the left when enabling AI mode fails', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-new', nick: '启用失败店铺', status: 'active', replyMode: 'off' },
      { uid: 'uid-ai', nick: '原有 AI 店铺', status: 'active', replyMode: 'ai' },
    ]
    apiMocks.updateAccountSetting.mockRejectedValueOnce(new Error('启用失败'))
    renderPage()
    await screen.findByText('API 类型')

    const { pending, configured } = accountSections()
    const enableFailureCheckbox = within(pending).getByRole('checkbox', { name: '选择待配置账号 启用失败店铺' }) as HTMLInputElement
    await waitFor(() => expect(enableFailureCheckbox.disabled).toBe(false))
    fireEvent.click(enableFailureCheckbox)
    fireEvent.click(within(pending).getByRole('button', { name: '保存配置' }))

    await waitFor(() => expect(notifyMocks.error).toHaveBeenCalledWith('启用失败'))
    expect(within(pending).getByText('启用失败店铺')).toBeTruthy()
    expect(within(configured).queryByText('启用失败店铺')).toBeNull()
    expect(emitAccountsSyncedMock).not.toHaveBeenCalled()
  })

  it('keeps a configured account on the right when turning AI mode off fails', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-ai', nick: '失败保留店铺', status: 'active', replyMode: 'ai' },
    ]
    apiMocks.updateAccountSetting.mockRejectedValueOnce(new Error('关闭失败'))
    renderPage()
    await screen.findByText('API 类型')

    const { pending, configured } = accountSections()
    const removeFailureButton = await within(configured).findByRole('button', { name: '移除已配置账号 失败保留店铺' }) as HTMLButtonElement
    await waitFor(() => expect(removeFailureButton.disabled).toBe(false))
    fireEvent.click(removeFailureButton)

    await waitFor(() => expect(notifyMocks.error).toHaveBeenCalledWith('关闭失败'))
    expect(within(configured).getByText('失败保留店铺')).toBeTruthy()
    expect(within(pending).queryByText('失败保留店铺')).toBeNull()
    expect(emitAccountsSyncedMock).not.toHaveBeenCalled()
  })
})
