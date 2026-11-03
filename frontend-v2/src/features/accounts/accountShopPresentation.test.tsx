// @ts-nocheck
import React from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import DesktopAccounts from './DesktopAccounts'
import { MobileAccountCard } from './mobile/MobileAccountCard'
import { AccountDetailContent } from './AccountDetailContent'
import { NotifyProvider } from '../../components/Notify'

const stateMock = {
  accounts: [
    {
      uid: 'uid-1',
      nick: '鱼小铺账号',
      status: 'active',
      created_at: 1710000000,
      added_at: 1710001000,
      bound_at: 1710001000,
      cookie_refreshed_at: 1710003600,
      location: null,
    },
  ],
  sortedAccounts: [
    {
      uid: 'uid-1',
      nick: '鱼小铺账号',
      status: 'active',
      created_at: 1710000000,
      added_at: 1710001000,
      bound_at: 1710001000,
      cookie_refreshed_at: 1710003600,
      location: null,
    },
  ],
  profiles: {
    'uid-1': {
      uid: 'uid-1',
      coinBalance: 12,
      fishPower: 785,
      totalEarned: 1234.56,
      fishShopEnabled: true,
      fishShopLevel: 'Lv.3',
      securityScore: { totalScore: 88 },
      violations: { totalCount: 0 },
      seller30d: {},
    },
  },
  kpi: {
    online: 1,
    total: 1,
    sessionErrors: 0,
    coin: 12,
    avgSec: '88',
    lowSec: 0,
    fp: 785,
    vio: 0,
    sold: 0,
    totalEarned: 1234.56,
  },
  loadAccounts: async () => {},
  showQr: false,
  qrUrl: '',
  qrStatusText: '',
  qrScanned: false,
  qrBootstrapSync: null,
  qrConflict: null,
  unbindApplying: false,
  handleAddAccount: () => {},
  handleReloginAccount: () => {},
  handleApplyAccountUnbind: () => {},
  refreshQr: () => {},
  closeQr: () => {},
  refreshingAll: false,
  handleRefreshAll: () => {},
  expandedUid: null,
  toggleExpand: () => {},
  handleRefreshAccount: () => {},
  handleDeleteAccount: () => {},
  savingMap: {},
  handleToggle: () => {},
  locationModal: null,
  openLocationModal: () => {},
  closeLocationModal: () => {},
}

let currentState = stateMock
const updateAccountProfileMock = vi.fn()
const accountEditableProfileMock = vi.fn(async () => currentState.profiles['uid-1'])
const uploadItemImageMock = vi.fn()

vi.mock('./hooks/useAccountsPageState', () => ({
  useAccountsPageState: () => currentState,
  accountOnlineDays: () => null,
  accountRuntimeStatus: () => ({
    key: 'active',
    label: '正常',
    tone: '#34C759',
    chipBg: '#ECFDF3',
    chipText: '#166534',
  }),
  accountRuntimeReason: () => '',
  accountAddedAt: (account) => account?.added_at ?? account?.addedAt ?? account?.bound_at ?? account?.boundAt,
  formatCurrencyMetric: (value) => `¥${Number(value).toFixed(2)}`,
  formatDateTime: () => '2026/05/23 10:00:00',
  formatAddedAge: () => '添加 10 天',
  formatLiveConnectionLabel: () => '消息通道未连接',
  formatRefreshedAt: () => '刚刚',
}))

vi.mock('./accountUi', async (importOriginal) => {
  const actual = await importOriginal()
  return {
    ...actual,
    getAccountActions: () => [],
    getAccountCapabilityToggles: () => [],
  }
})

vi.mock('../../components/HubTabs', () => ({
  HubTabs: () => <div>HubTabs</div>,
  HUB_PRESETS: { accounts: [] },
}))

vi.mock('../../components/EmptyState', () => ({
  EmptyState: ({ title }) => <div>{title}</div>,
}))

vi.mock('../../components/Modal', () => ({
  Modal: ({ children, open, title, footer }) => (open ? <div><div>{title}</div>{children}{footer}</div> : null),
}))

vi.mock('./PublishLocationModal', () => ({
  PublishLocationModal: () => null,
}))

vi.mock('../items/itemUtils', () => ({
  composeAccountLocation: () => '',
}))

vi.mock('../../api/index', () => ({
  accountCoinDetail: async () => ({ data: { userScoreDetailDOList: [] } }),
  accountEditableProfile: (...args) => accountEditableProfileMock(...args),
  accountProfile: (...args) => accountEditableProfileMock(...args),
  accountSellerData: async () => ({ data: { data: { graphBannerBenchData: {} } } }),
  accountSecurityScoreRecords: async () => ({ data: { data: [] } }),
  accountViolationsAll: async () => ({ data: { data: [] } }),
  updateAccountProfile: (...args) => updateAccountProfileMock(...args),
  isSuccess: (payload) => String(payload?.ret?.[0] ?? payload?.ret ?? '').includes('SUCCESS'),
  retMsg: (payload, fallback = '操作失败') => String(payload?.ret?.[0] ?? payload?.ret ?? fallback).split('::')[1] || fallback,
}))

vi.mock('../../api/market', () => ({
  uploadItemImage: (...args) => uploadItemImageMock(...args),
}))

vi.mock('../../api/shopGrowth', () => ({
  getCoinSigninHome: async () => ({}),
  mtopRetText: (payload) => String(payload?.ret?.[0] ?? payload?.ret ?? ''),
  signInCoin: async () => ({ ret: ['SUCCESS::调用成功'] }),
}))

describe('account fish shop presentation', () => {
  beforeEach(() => {
    currentState = stateMock
    updateAccountProfileMock.mockReset()
    updateAccountProfileMock.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    accountEditableProfileMock.mockClear()
    accountEditableProfileMock.mockImplementation(async () => currentState.profiles['uid-1'])
    uploadItemImageMock.mockReset()
    uploadItemImageMock.mockResolvedValue({ success: true, url: 'https://img.example/avatar.png' })
  })

  it('renders fish shop status and level on desktop accounts list', () => {
    currentState = stateMock
    const { container } = render(<DesktopAccounts />)

    expect(screen.getByText('鱼小铺')).toBeTruthy()
    const fishShopLevel = screen.getByText('L3')
    expect(fishShopLevel).toBeTruthy()
    expect(fishShopLevel.getAttribute('style')).toContain('var(--status-warning-bg)')
    const fishShop = screen.getByText('鱼小铺')
    const nameRow = fishShop.closest('div')
    expect(nameRow?.textContent).toContain('正常')
    expect(nameRow?.textContent).not.toContain('编辑资料')
    expect(screen.getByRole('button', { name: '编辑资料' })).toBeTruthy()
    const runtimeChip = screen.getByText('正常')
    expect(runtimeChip.className).toContain('whitespace-nowrap')
    expect(runtimeChip.className).toContain('shrink-0')
    expect(container.textContent).not.toContain('发布地 · 未设置鱼小铺L3')
  })

  it('keeps account row metrics in one desktop row and removes the duplicate AI settings action', () => {
    currentState = stateMock
    const { container } = render(<DesktopAccounts />)

    expect(screen.queryByRole('button', { name: 'AI 助手配置' })).toBeNull()
    const metricGrid = container.querySelector('.account-row-metrics')
    expect(metricGrid?.textContent).toContain('闲鱼币')
    expect(metricGrid?.textContent).toContain('安全分')
    expect(metricGrid?.textContent).toContain('鱼力值')
    expect(metricGrid?.textContent).toContain('违规')
    expect(metricGrid?.textContent).toContain('30天销')
    expect(metricGrid?.textContent).toContain('累计赚')
  })

  it('opens profile editor from desktop account row beside fish shop badge', async () => {
    const user = userEvent.setup()
    currentState = {
      ...stateMock,
      profiles: {
        'uid-1': {
          ...stateMock.profiles['uid-1'],
          snsNick: '旧昵称',
        },
      },
    }

    render(<DesktopAccounts />)

    await user.click(screen.getByRole('button', { name: '编辑资料' }))
    expect(screen.getByLabelText('昵称')).toBeTruthy()
    expect(screen.getByDisplayValue('旧昵称')).toBeTruthy()
  })

  it('shows a scanned check overlay on the QR modal after scan', () => {
    currentState = {
      ...stateMock,
      showQr: true,
      qrUrl: 'data:image/png;base64,qr',
      qrStatusText: '已扫码，请在手机上确认…',
      qrScanned: true,
    }

    render(<DesktopAccounts />)

    expect(screen.getByLabelText('二维码已扫码')).toBeTruthy()
  })

  it('keeps first-login data sync out of the QR modal', () => {
    currentState = {
      ...stateMock,
      showQr: true,
      qrUrl: 'data:image/png;base64,qr',
      qrStatusText: '账号已添加，正在后台同步订单和商品…',
      qrScanned: true,
      qrBootstrapSync: {
        active: true,
        current: 1,
        total: 3,
        steps: [
          { key: 'messages', label: '最近消息', status: 'success', detail: '已同步 12 条' },
          { key: 'orders', label: '最近订单', status: 'running', detail: '同步中…' },
          { key: 'items', label: '最近商品', status: 'pending' },
        ],
      },
    }

    render(<DesktopAccounts />)

    expect(screen.getByText('账号已添加，正在后台同步订单和商品…')).toBeTruthy()
    expect(screen.queryByText('同步最近数据')).toBeNull()
    expect(screen.queryByText('最近消息')).toBeNull()
    expect(screen.queryByText('最近订单')).toBeNull()
    expect(screen.queryByText('最近商品')).toBeNull()
  })

  it('hydrates full profile fields when opening editor from desktop account row summary', async () => {
    const user = userEvent.setup()
    currentState = {
      ...stateMock,
      profiles: {
        'uid-1': {
          uid: 'uid-1',
          fishShopEnabled: true,
          fishShopLevel: 'Lv.3',
          securityScore: { totalScore: 88 },
        },
      },
    }
    accountEditableProfileMock.mockResolvedValueOnce({
      uid: 'uid-1',
      snsNick: '完整昵称',
      signature: '完整签名',
      city: '长沙',
    })

    render(<DesktopAccounts />)

    await user.click(screen.getByRole('button', { name: '编辑资料' }))
    expect(await screen.findByDisplayValue('完整昵称')).toBeTruthy()
    expect(screen.getByDisplayValue('完整签名')).toBeTruthy()
    expect(screen.getByDisplayValue('长沙')).toBeTruthy()
  })

  it('renders fish shop status and level on mobile account card', () => {
    currentState = stateMock
    render(
      <MobileAccountCard
        account={stateMock.sortedAccounts[0]}
        profile={stateMock.profiles['uid-1']}
        onTap={() => {}}
        onRelogin={() => {}}
      />,
    )

    expect(screen.getByText('鱼小铺')).toBeTruthy()
    expect(screen.getByText('L3')).toBeTruthy()
  })

  it('renders fish shop status and level in account detail header', async () => {
    render(
      <AccountDetailContent
        uid="uid-1"
        account={stateMock.sortedAccounts[0]}
        initialProfile={stateMock.profiles['uid-1']}
        showHeader
      />,
    )

    expect(await screen.findAllByText('鱼小铺')).toHaveLength(1)
    expect(screen.getByText('L3')).toBeTruthy()
    expect(screen.getByRole('button', { name: '编辑资料' })).toBeTruthy()
  })

  it('removes duplicate promotion and task lanes from account detail', async () => {
    const user = userEvent.setup()
    render(
      <AccountDetailContent
        uid="uid-1"
        account={stateMock.sortedAccounts[0]}
        initialProfile={stateMock.profiles['uid-1']}
        showHeader
      />,
    )

    expect((await screen.findAllByText('安全分')).length).toBeGreaterThan(0)
    expect(screen.getAllByText('闲鱼币').length).toBeGreaterThan(0)
    expect(screen.getByText('鱼小铺经营')).toBeTruthy()
    expect(screen.queryByText('曝光任务')).toBeNull()
    expect(screen.queryByText('商品推广')).toBeNull()
    expect(screen.queryByText('闲鱼币任务')).toBeNull()
    await user.click(screen.getByRole('button', { name: '闲鱼币' }))
    expect(screen.queryByRole('button', { name: '签到领币' })).toBeNull()
    expect(screen.queryByRole('button', { name: '今日已签' })).toBeNull()
  })

  it('opens profile editor modal with current values and saves profile fields', async () => {
    const user = userEvent.setup()
    currentState = {
      ...stateMock,
      profiles: {
        'uid-1': {
          ...stateMock.profiles['uid-1'],
          snsNick: '旧昵称',
          signature: '旧签名',
          city: '杭州',
        },
      },
    }

    render(
      <AccountDetailContent
        uid="uid-1"
        account={{ ...stateMock.sortedAccounts[0], nick: '旧昵称' }}
        initialProfile={currentState.profiles['uid-1']}
        showHeader
      />,
    )

    await user.click(await screen.findByRole('button', { name: '编辑资料' }))
    expect(screen.getByDisplayValue('旧昵称')).toBeTruthy()
    expect(screen.getByDisplayValue('旧签名')).toBeTruthy()
    expect(screen.getByDisplayValue('杭州')).toBeTruthy()
    await user.clear(screen.getByLabelText('昵称'))
    await user.type(screen.getByLabelText('昵称'), '新昵称')
    await user.clear(screen.getByLabelText('简介'))
    await user.type(screen.getByLabelText('简介'), '新的个人签名')
    await user.click(screen.getByRole('button', { name: '保存资料' }))

    expect(updateAccountProfileMock).toHaveBeenCalledWith({
      uid: 'uid-1',
      snsNick: '新昵称',
      signature: '新的个人签名',
    })
    expect(accountEditableProfileMock).toHaveBeenCalledWith('uid-1', true)
  })

  it('does not render duplicate profile editor entry in compact detail mode', () => {
    render(
      <AccountDetailContent
        uid="uid-1"
        account={stateMock.sortedAccounts[0]}
        initialProfile={stateMock.profiles['uid-1']}
        showHeader={false}
      />,
    )

    expect(screen.queryByRole('button', { name: '编辑资料' })).toBeNull()
  })

  it('can clear profile intro fields from account detail profile editor', async () => {
    const user = userEvent.setup()
    currentState = {
      ...stateMock,
      profiles: {
        'uid-1': {
          ...stateMock.profiles['uid-1'],
          snsNick: '旧昵称',
          signature: '旧签名',
        },
      },
    }

    render(
      <AccountDetailContent
        uid="uid-1"
        account={{ ...stateMock.sortedAccounts[0], nick: '旧昵称' }}
        initialProfile={currentState.profiles['uid-1']}
        showHeader
      />,
    )

    await user.click(await screen.findByRole('button', { name: '编辑资料' }))
    await user.clear(screen.getByLabelText('简介'))
    await user.click(screen.getByRole('button', { name: '保存资料' }))

    expect(updateAccountProfileMock).toHaveBeenCalledWith({
      uid: 'uid-1',
      signature: '',
    })
  })

  it('shows upstream profile edit failure message', async () => {
    const user = userEvent.setup()
    updateAccountProfileMock.mockResolvedValue({
      ret: ['FAIL_BIZ_USER_NICK_CAN_NOT_UPDATE::近期已修改过昵称哦,30天内限修改1次'],
    })
    currentState = {
      ...stateMock,
      profiles: {
        'uid-1': {
          ...stateMock.profiles['uid-1'],
          snsNick: '旧昵称',
        },
      },
    }

    render(
      <NotifyProvider>
        <AccountDetailContent
          uid="uid-1"
          account={{ ...stateMock.sortedAccounts[0], nick: '旧昵称' }}
          initialProfile={currentState.profiles['uid-1']}
          showHeader
        />
      </NotifyProvider>,
    )

    await user.click(await screen.findByRole('button', { name: '编辑资料' }))
    await user.clear(screen.getByLabelText('昵称'))
    accountEditableProfileMock.mockClear()
    await user.type(screen.getByLabelText('昵称'), '新昵称')
    await user.click(screen.getByRole('button', { name: '保存资料' }))

    expect(await screen.findByText('近期已修改过昵称哦,30天内限修改1次')).toBeTruthy()
    expect(accountEditableProfileMock).not.toHaveBeenCalled()
  })

  it('uploads avatar image before saving profile', async () => {
    const user = userEvent.setup()

    render(
      <AccountDetailContent
        uid="uid-1"
        account={stateMock.sortedAccounts[0]}
        initialProfile={stateMock.profiles['uid-1']}
        showHeader
      />,
    )

    await user.click(await screen.findByRole('button', { name: '编辑资料' }))
    const file = new File(['avatar-binary'], 'avatar.png', { type: 'image/png' })
    await user.upload(screen.getByLabelText('头像'), file)
    await user.click(screen.getByRole('button', { name: '保存资料' }))

    expect(uploadItemImageMock).toHaveBeenCalledWith('uid-1', file)
    expect(updateAccountProfileMock).toHaveBeenCalledWith({
      uid: 'uid-1',
      avatar: 'https://img.example/avatar.png',
    })
  })

  it('hides fish shop row when account is not enabled', () => {
    currentState = {
      ...stateMock,
      profiles: {
        'uid-1': {
          ...stateMock.profiles['uid-1'],
          fishShopEnabled: false,
          fishShopLevel: '',
        },
      },
    }
    render(<DesktopAccounts />)
    expect(screen.queryByText('鱼小铺')).toBeNull()
    expect(screen.queryByText(/L\d+/)).toBeNull()
  })

  it('hides fish shop row in mobile and detail when account is not enabled', async () => {
    currentState = {
      ...stateMock,
      profiles: {
        'uid-1': {
          ...stateMock.profiles['uid-1'],
          fishShopEnabled: false,
          fishShopLevel: '',
        },
      },
    }

    const disabledProfile = currentState.profiles['uid-1']

    const { rerender } = render(
      <MobileAccountCard
        account={stateMock.sortedAccounts[0]}
        profile={disabledProfile}
        onTap={() => {}}
        onRelogin={() => {}}
      />,
    )
    expect(screen.queryByText('鱼小铺')).toBeNull()

    rerender(
      <AccountDetailContent
        uid="uid-1"
        account={stateMock.sortedAccounts[0]}
        initialProfile={disabledProfile}
        showHeader
      />,
    )
    expect(screen.queryByText('鱼小铺')).toBeNull()
  })

  it('keeps desktop metrics and fish shop visible from cached account fields before live profile returns', () => {
    currentState = {
      ...stateMock,
      accounts: [
        {
          ...stateMock.accounts[0],
          coin_balance: 51605,
          security_score: 70,
          fish_power: 788,
          violation_count: 6,
          seller_30d_sold: '12',
          totalEarned: 1977.14,
          fishShopEnabled: true,
          fishShopLevel: 'Lv.7',
        },
      ],
      sortedAccounts: [
        {
          ...stateMock.sortedAccounts[0],
          coin_balance: 51605,
          security_score: 70,
          fish_power: 788,
          violation_count: 6,
          seller_30d_sold: '12',
          totalEarned: 1977.14,
          fishShopEnabled: true,
          fishShopLevel: 'Lv.7',
        },
      ],
      profiles: {},
      kpi: {
        ...stateMock.kpi,
        coin: 51605,
        avgSec: '70',
        fp: 788,
        vio: 6,
        sold: 12,
        totalEarned: 1977.14,
      },
    }

    const { container } = render(<DesktopAccounts />)

    expect(screen.getByText('鱼小铺')).toBeTruthy()
    expect(screen.getByText('L7')).toBeTruthy()
    expect(container.textContent).toContain('51,605')
    expect(container.textContent).toContain('788')
    expect(container.textContent).toContain('¥1977.14')
  })
})
