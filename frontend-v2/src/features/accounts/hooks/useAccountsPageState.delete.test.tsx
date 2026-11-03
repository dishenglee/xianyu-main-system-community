import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useAccountsPageState } from './useAccountsPageState'

const mocks = vi.hoisted(() => ({
  deleteAccount: vi.fn(),
  getAuthToken: vi.fn(),
  loadAccounts: vi.fn(),
  notify: vi.fn(),
  showConfirm: vi.fn(),
  emitAccountsSynced: vi.fn(),
  listenSyncEvent: vi.fn(),
  taskProgressStart: vi.fn(),
  taskProgressTick: vi.fn(),
  taskProgressFinish: vi.fn(),
}))

vi.mock('../../../hooks/useAccounts', () => ({
  useAccounts: () => ({
    accounts: [{ uid: 'uid-1', nick: '账号甲', status: 'active' }],
    loadAccounts: mocks.loadAccounts,
  }),
}))

vi.mock('../../../hooks/useAccountProfiles', () => ({
  useAccountProfiles: () => ({ profiles: {} }),
  getSellerMetric: () => 0,
  SELLER_METRIC_ALIASES: { soldCount: [] },
}))

vi.mock('../../../api/index', () => ({
  addAccount: vi.fn(),
  createAccountUnbindRequest: vi.fn(),
  deleteAccount: mocks.deleteAccount,
  qrStatus: vi.fn(),
  refreshAccount: vi.fn(),
  refreshAllAccounts: vi.fn(),
  syncItems: vi.fn(),
  syncOrders: vi.fn(),
  updateAccountSetting: vi.fn(),
}))

vi.mock('../../../components/Notify', () => ({
  useNotify: () => mocks.notify,
}))

vi.mock('../../../components/AppDialog', () => ({
  useAppDialog: () => ({
    showConfirm: mocks.showConfirm,
    showPrompt: vi.fn(),
  }),
}))

vi.mock('../../../store/taskProgressStore', () => ({
  useTaskProgressStore: (selector: (state: {
    start: typeof mocks.taskProgressStart
    tick: typeof mocks.taskProgressTick
    finish: typeof mocks.taskProgressFinish
  }) => unknown) => selector({
    start: mocks.taskProgressStart,
    tick: mocks.taskProgressTick,
    finish: mocks.taskProgressFinish,
  }),
}))

vi.mock('../../../api/client', () => ({
  getAuthToken: mocks.getAuthToken,
  emitClientLog: vi.fn(),
}))

vi.mock('../../../hooks/useAuthScope', () => ({
  useAuthScope: () => ({ authToken: 'token-a' }),
}))

vi.mock('../../../store/authStore', () => ({
  useAuthStore: (selector: (state: { user: null }) => unknown) => selector({ user: null }),
}))

vi.mock('../../runtimeEvents', () => ({
  emitAccountsSynced: mocks.emitAccountsSynced,
  emitItemsSynced: vi.fn(),
  emitOrdersSynced: vi.fn(),
  listenSyncEvent: mocks.listenSyncEvent,
  XIANYU_ACCOUNTS_SYNCED_EVENT: 'xy:accounts-synced',
}))

describe('useAccountsPageState delete account', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.deleteAccount.mockResolvedValue(undefined)
    mocks.getAuthToken.mockReturnValue('token-a')
    mocks.loadAccounts.mockResolvedValue([])
    mocks.showConfirm.mockResolvedValue(true)
    mocks.listenSyncEvent.mockReturnValue(() => {})
  })

  it('force reloads accounts after deleting so the removed account disappears immediately', async () => {
    const { result } = renderHook(() => useAccountsPageState())

    mocks.loadAccounts.mockClear()
    await act(async () => {
      await result.current.handleDeleteAccount('uid-1', '账号甲')
    })

    expect(mocks.deleteAccount).toHaveBeenCalledWith('uid-1')
    expect(mocks.loadAccounts).toHaveBeenCalledWith({ force: true })
    expect(mocks.emitAccountsSynced).toHaveBeenCalledWith({ uid: 'uid-1', source: 'accounts-delete' })
    expect(mocks.notify).toHaveBeenCalledWith('已删除账号：账号甲', 'success', 2600)
  })
})
