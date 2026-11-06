import React from 'react'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DataCenterStats } from '../../api/types'

type LiveTransportOptions = {
  onEvent?: (event: unknown) => void
  onStateChange?: (state: string) => void
}

const { getDataCenterStatsMock, liveTransportControllers } = vi.hoisted(() => ({
  getDataCenterStatsMock: vi.fn(),
  liveTransportControllers: [] as Array<{
    options: { onEvent?: (event: unknown) => void; onStateChange?: (state: string) => void }
    subscribe: ReturnType<typeof vi.fn>
    close: ReturnType<typeof vi.fn>
  }>,
}))

vi.mock('../../api/index', () => ({
  getDataCenterStats: (...args: unknown[]) => getDataCenterStatsMock(...args),
}))

vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ device: 'desktop', isMobile: false, isTablet: false, isDesktop: true }),
}))

vi.mock('../messages/useLiveTransport', () => ({
  createLiveTransport: (options: LiveTransportOptions = {}) => {
    const controller = {
      options,
      subscribe: vi.fn(),
      close: vi.fn(),
      connect: vi.fn(() => options.onStateChange?.('ws')),
    }
    liveTransportControllers.push(controller)
    return controller
  },
}))

import Dashboard from './Dashboard'

const makeStats = (nick: string): DataCenterStats => ({
  date: '2026-05-25',
  generatedAt: 1779645600,
  totals: {
    todayRevenue: 0,
    todayOrders: 0,
    waitSendOrders: 0,
    waitSendEarliestAt: 0,
    pendingPayOrders: 0,
    pendingPayAmount: 0,
    unreadSessions: 0,
    unreadMessages: 0,
    overdueReplySessions: 0,
    onlineItems: 1,
    totalItems: 1,
    aftersaleOrders: 0,
    closedOrders: 0,
    accountTotal: 1,
    activeAccounts: 1,
    liveOnlineAccounts: 1,
    sessionErrorAccounts: 0,
  },
  accounts: [
    {
      uid: 'u1',
      nick,
      avatar: '',
      status: 'active',
      liveStatus: 'connected',
      lastError: '',
      todayRevenue: 0,
      todayOrders: 0,
      weekRevenue: 0,
      weekOrders: 0,
      totalRevenue: 0,
      totalOrders: 0,
      waitSendOrders: 0,
      pendingPayOrders: 0,
      unreadMessages: 0,
      unreadSessions: 0,
      onlineItems: 1,
      totalItems: 1,
      lastMessageAt: 0,
      lastOrderAt: 0,
      itemsSyncedAt: 0,
      ordersSyncedAt: 0,
    },
  ],
  queue: [],
  share: {
    day: { range: 'day', label: '今日成交', totalAmount: 0, totalOrders: 0, segments: [] },
    week: { range: 'week', label: '7 天成交', totalAmount: 0, totalOrders: 0, segments: [] },
    total: { range: 'total', label: '总成交', totalAmount: 0, totalOrders: 0, segments: [] },
  },
  trend: {
    week: { range: 'week', label: '7 天成交', totalAmount: 0, totalOrders: 0, points: [] },
    month: { range: 'month', label: '30 天成交', totalAmount: 0, totalOrders: 0, points: [] },
    total: { range: 'total', label: '总成交', totalAmount: 0, totalOrders: 0, points: [] },
  },
  sellerTrend: {
    week: { available: true, range: 'week', label: '近 7 天', totalAmount: 0, totalOrders: 0, points: [], syncedAt: 1779645600 },
    halfMonth: { available: false, range: 'halfMonth', label: '近 15 天', totalAmount: 0, totalOrders: 0, points: [], syncedAt: 0 },
    month: { available: true, range: 'month', label: '近 30 天', totalAmount: 0, totalOrders: 0, points: [], syncedAt: 1779645600 },
  },
  sync: {
    status: 'ok',
    subtitle: '本地同步正常',
    sqliteOk: true,
    liveOnline: 1,
    liveTotal: 1,
    staleAccounts: 0,
    signServiceOk: true,
    signServiceStatusCode: 200,
    licenseServiceOk: true,
    licenseServiceStatusCode: 200,
    dataCompass: {
      status: 'ok',
      accountCount: 1,
      successCount: 1,
      failedCount: 0,
      lastSuccessAt: 1779645600,
      errorSummary: '',
    },
  },
})

describe('Dashboard realtime refresh', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
    getDataCenterStatsMock.mockReset()
    liveTransportControllers.length = 0
    window.scrollTo = vi.fn()
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      value: 'visible',
    })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('does not refresh the whole data center on a timer', async () => {
    getDataCenterStatsMock
      .mockResolvedValueOnce(makeStats('账号甲'))
      .mockResolvedValueOnce(makeStats('账号乙'))

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    await act(async () => {
      await Promise.resolve()
    })

    expect(screen.getByRole('heading', { name: '经营总览' })).toBeTruthy()
    expect((screen.getByRole('combobox', { name: '账号范围' }) as HTMLSelectElement).value).toBe('all')
    expect(screen.getByText('全部账号 · 近 7 天成交')).toBeTruthy()
    expect(screen.getAllByText('暂无成交').length).toBeGreaterThan(0)
    expect(screen.queryByText('等待数据罗盘同步')).toBeNull()
    expect(screen.getByTestId('ring-order-handling').textContent).toContain('数据暂不可用')

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })

    expect(screen.getByText('全部账号 · 近 7 天成交')).toBeTruthy()
    expect(screen.queryByText('账号乙')).toBeNull()
    expect(getDataCenterStatsMock).toHaveBeenCalledTimes(1)
  })

  it('keeps the last successful stats visible when manual refresh fails', async () => {
    getDataCenterStatsMock
      .mockResolvedValueOnce(makeStats('账号甲'))
      .mockRejectedValueOnce(new Error('network down'))

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })

    await act(async () => {
      screen.getByRole('button', { name: /刷新数据/ }).click()
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(screen.getByText('全部账号 · 近 7 天成交')).toBeTruthy()
    expect(screen.queryByText('本地同步正常')).toBeNull()
    expect(screen.getByTestId('dashboard-load-error').textContent).toContain('network down')
    expect(screen.getByText('当前客户端暂未返回自动化运行汇总')).toBeTruthy()
    expect(getDataCenterStatsMock).toHaveBeenNthCalledWith(2, { trendUid: undefined, refresh: true })
  })

  it('does not render EMPTY_STATS zeroes when the initial load fails', async () => {
    getDataCenterStatsMock.mockRejectedValueOnce(new Error('initial down'))
    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve(); await Promise.resolve() })

    expect(screen.getByTestId('dashboard-initial-state').textContent).toBe('经营数据暂不可用')
    expect(screen.getByTestId('dashboard-load-error').textContent).toContain('initial down')
    expect(screen.queryByText('暂无成交')).toBeNull()
    expect(screen.queryByTestId('todo-wait-send')).toBeNull()
  })

  it('links real metrics, ranges, trends and queue tabs in the redesigned analysis overview', async () => {
    const stats = makeStats('账号甲')
    stats.totals.waitSendOrders = 3
    stats.totals.overdueReplySessions = 2
    stats.totals.aftersaleOrders = 1
    stats.totals.sessionErrorAccounts = 1
    stats.totals.todayRevenue = 18
    stats.totals.todayOrders = 1
    stats.totals.unreadMessages = 4
    stats.totals.accountTotal = 1
    stats.sync.staleAccounts = 1
    stats.accounts[0].waitSendOrders = 3
    stats.accounts[0].unreadMessages = 4
    stats.automation = {
      delivery: { enabled: 3, running: null, pending: 2, errors: null },
      scheduledTasks: { enabled: 5, running: 2, pending: 1, errors: 1 },
      replies: { enabled: 4, running: null, pending: null, errors: null },
    }
    stats.sellerTrend.week = { available: true, range: 'week', label: '近 7 天', totalAmount: 88, totalOrders: 5, syncedAt: 1779645600, points: [
      { date: '2026-07-28', label: '7/28', amount: 20, orders: 2, messages: 6 },
      { date: '2026-07-29', label: '7/29', amount: 68, orders: 3, messages: 9 },
    ] }
    stats.share.week = { range: 'week', label: '7 天成交', totalAmount: 88, totalOrders: 5, segments: [
      { uid: 'u1', nick: '账号甲', amount: 88, orders: 5, percent: 100 },
    ] }
    stats.queue = [{
      id: 'q1',
      kind: 'order',
      uid: 'u1',
      accountNick: '账号甲',
      title: '苹果音乐会员兑换码',
      subtitle: '订单号 20260729001',
      customer: '买家甲',
      amount: 18,
      status: 'WAIT_SELLER_SEND_GOODS',
      statusLabel: '待发货',
      actionLabel: '去发货',
      targetPath: '/orders',
      ts: 1779645600,
    }, {
      id: 'q2', kind: 'message', uid: 'u1', accountNick: '账号甲', title: '买家咨询库存', subtitle: '', customer: '买家乙', amount: 0,
      status: 'unread', statusLabel: '未回复', actionLabel: '去回复', targetPath: '/messages', ts: 1779645600,
    }, {
      id: 'q3', kind: 'message', uid: 'u1', accountNick: '账号甲', title: '已经回复的咨询', subtitle: '', customer: '买家丙', amount: 0,
      status: 'replied', statusLabel: '已回复', actionLabel: '详情', targetPath: '/messages', ts: 1779645600,
    }]
    getDataCenterStatsMock.mockResolvedValue(stats)

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    await act(async () => {
      await Promise.resolve()
    })

    expect(screen.getByRole('heading', { name: '经营总览' })).toBeTruthy()
    expect((screen.getByRole('combobox', { name: '账号范围' }) as HTMLSelectElement).value).toBe('all')
    expect(screen.getByRole('option', { name: '全部账号（1）' })).toBeTruthy()
    expect(screen.getByRole('option', { name: '账号甲' })).toBeTruthy()
    expect(screen.queryByText('本地同步正常')).toBeNull()
    expect(screen.getByText('全部账号 · 近 7 天成交')).toBeTruthy()
    expect(screen.getByRole('button', { name: '近 7 天' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '近 15 天' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '近 30 天' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: '累计' })).toBeNull()
    expect(screen.getByTestId('ring-account-share').textContent).toContain('账号成交占比')
    expect(screen.getByTestId('ring-order-handling').textContent).toContain('订单处理')
    expect(screen.getByTestId('ring-order-handling').textContent).toContain('数据暂不可用')
    expect(screen.getByTestId('ring-automation-coverage').textContent).toContain('数据暂不可用')
    expect(screen.getByRole('heading', { name: '待办与异常' })).toBeTruthy()
    expect(screen.queryByRole('link', { name: '查看全部' })).toBeNull()
    expect(screen.getByText('自动化运行')).toBeTruthy()
    expect(screen.getByText('自动发货')).toBeTruthy()
    expect(screen.getByText('定时任务')).toBeTruthy()
    expect(screen.getByText('回复自动化')).toBeTruthy()
    expect(screen.getByTestId('automation-delivery').textContent).toContain('自动发货3启用账号2启用规则')
    expect(screen.getByTestId('automation-scheduled-tasks').textContent).toContain('定时任务1已安排1最近失败')
    expect(screen.getByTestId('automation-replies').textContent).toContain('回复自动化4启用账号')
    expect(screen.getByTestId('todo-wait-send').getAttribute('href')).toBe('/orders')
    expect(screen.getByTestId('todo-aftersale').getAttribute('href')).toBe('/orders')
    expect(screen.getByTestId('todo-messages').getAttribute('href')).toBe('/messages')
    expect(screen.getByTestId('todo-connections').getAttribute('href')).toBe('/accounts')
    expect(screen.getByTestId('todo-risk').getAttribute('href')).toBe('/accounts')
    expect(screen.getByTestId('todo-automation').getAttribute('href')).toBe('/other-automation')
    expect(screen.getByTestId('automation-delivery').getAttribute('href')).toBe('/delivery')
    expect(screen.getByTestId('automation-replies').getAttribute('href')).toBe('/keywords')
    expect(screen.getByTestId('automation-scheduled-tasks').getAttribute('href')).toBe('/other-automation')
    expect(screen.getByRole('link', { name: /管理自动化/ }).getAttribute('href')).toBe('/keywords')
    expect(screen.queryByText('账号运行')).toBeNull()
    expect(screen.queryByText('待发货分布')).toBeNull()
    expect(screen.queryByText('未读消息分布')).toBeNull()
    expect(screen.queryByText('在售商品分布')).toBeNull()
    expect(screen.queryByText('成交贡献')).toBeNull()
    expect(screen.queryByText('本地数据')).toBeNull()
    expect(screen.queryByText('快捷入口')).toBeNull()
    expect(screen.queryByText('苹果音乐会员兑换码')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: '近 15 天' }))
    expect(screen.getByText('全部账号 · 近 15 天成交')).toBeTruthy()
    expect(screen.getByTestId('dashboard-trend').textContent).toContain('成交趋势暂不可用')
    fireEvent.click(screen.getByRole('button', { name: '近 30 天' }))
    expect(screen.getByText('全部账号 · 近 30 天成交')).toBeTruthy()
    expect(screen.getByTestId('ring-account-share').textContent).toContain('数据暂不可用')

    fireEvent.change(screen.getByRole('combobox', { name: '账号范围' }), { target: { value: 'u1' } })
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(screen.getByText('账号甲 · 近 30 天成交')).toBeTruthy()
    expect(getDataCenterStatsMock).toHaveBeenLastCalledWith({ trendUid: 'u1', refresh: undefined })
  })

  it('thins month axis labels while retaining every trend point and tooltip', async () => {
    const stats = makeStats('账号甲')
    stats.sellerTrend.month.points = Array.from({ length: 30 }, (_, index) => ({
      date: `2026-07-${String(index + 1).padStart(2, '0')}`,
      label: `7/${index + 1}`,
      amount: index + 1,
      orders: index,
      messages: index * 2,
    }))
    stats.sellerTrend.month.totalAmount = 465
    stats.sellerTrend.month.totalOrders = 435
    getDataCenterStatsMock.mockResolvedValueOnce(stats)

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })
    fireEvent.click(screen.getByRole('button', { name: '近 30 天' }))

    const chart = screen.getByRole('img', { name: '近 30 天成交金额趋势图' })
    const bars = chart.querySelectorAll('.dashboard-v7-bar')
    expect(bars).toHaveLength(30)
    expect(chart.querySelectorAll('.dashboard-v7-bar span')).toHaveLength(30)
    const labelledBars = [...bars].filter((bar) => bar.getAttribute('data-day'))
    expect(labelledBars).toHaveLength(6)
    expect(labelledBars[0]?.getAttribute('data-day')).toBe('7/1')
    expect(labelledBars.at(-1)?.getAttribute('data-day')).toBe('7/30')
    expect(labelledBars[0]?.classList.contains('is-first-label')).toBe(true)
    expect(labelledBars.at(-1)?.classList.contains('is-last-label')).toBe(true)
  })

  it('renders persisted 15-day seller trend when it is available', async () => {
    const stats = makeStats('账号甲')
    stats.sellerTrend.halfMonth = {
      available: true,
      range: 'halfMonth',
      label: '近 15 天',
      totalAmount: 150,
      totalOrders: 15,
      syncedAt: 1779645600,
      points: Array.from({ length: 15 }, (_, index) => ({
        date: `2026-07-${String(index + 1).padStart(2, '0')}`,
        label: `7/${index + 1}`,
        amount: index + 1,
        orders: 1,
        messages: 0,
      })),
    }
    getDataCenterStatsMock.mockResolvedValueOnce(stats)

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })
    fireEvent.click(screen.getByRole('button', { name: '近 15 天' }))

    expect(screen.getByText('¥150')).toBeTruthy()
    expect(screen.getByRole('img', { name: '近 15 天成交金额趋势图' }).querySelectorAll('.dashboard-v7-bar')).toHaveLength(15)
  })

  it('reloads and renders the selected account seller trend by uid', async () => {
    const aggregate = makeStats('账号甲')
    const selected = makeStats('账号甲')
    selected.sellerTrend.week = {
      available: true,
      range: 'week',
      label: '近 7 天',
      totalAmount: 77,
      totalOrders: 7,
      syncedAt: 1779645600,
      points: Array.from({ length: 7 }, (_, index) => ({
        date: `2026-07-${String(index + 20).padStart(2, '0')}`,
        label: `7/${index + 20}`,
        amount: index + 5,
        orders: 1,
        messages: 0,
      })),
    }
    getDataCenterStatsMock.mockResolvedValueOnce(aggregate).mockResolvedValueOnce(selected)

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })
    await act(async () => {
      fireEvent.change(screen.getByRole('combobox', { name: '账号范围' }), { target: { value: 'u1' } })
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(getDataCenterStatsMock).toHaveBeenNthCalledWith(2, { trendUid: 'u1', refresh: undefined })
    expect(screen.getByText('账号甲 · 近 7 天成交')).toBeTruthy()
    expect(screen.getByText('¥77')).toBeTruthy()
    expect(screen.getByRole('img', { name: '近 7 天成交金额趋势图' }).querySelectorAll('.dashboard-v7-bar')).toHaveLength(7)
  })

  it('counts account errors across all accounts without restoring the removed account ranking', async () => {
    const stats = makeStats('账号甲')
    stats.totals.accountTotal = 13
    stats.accounts = Array.from({ length: 13 }, (_, index) => ({
      ...stats.accounts[0],
      uid: `u${index + 1}`,
      nick: index === 12 ? '错误账号' : `普通账号${index + 1}`,
      lastError: index === 12 ? '签名失效 https://sign.gooofish.shop/private/path' : '',
      liveStatus: index === 12 ? 'error' : 'ok',
      waitSendOrders: index < 12 ? 100 - index : 0,
    }))
    getDataCenterStatsMock.mockResolvedValueOnce(stats)

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })

    expect(screen.getByTestId('todo-risk').textContent).toContain('1 个账号存在需要查看的风险记录')
    expect(screen.getByTestId('todo-risk').getAttribute('href')).toBe('/accounts')
    expect(document.querySelector('.dashboard-account-main')).toBeNull()
  })

  it('orders account choices by error, connection issue, then pending workload', async () => {
    const stats = makeStats('普通账号')
    const base = stats.accounts[0]
    stats.accounts = [
      { ...base, uid: 'pending', nick: '待处理账号', waitSendOrders: 9 },
      { ...base, uid: 'normal', nick: '普通账号' },
      { ...base, uid: 'offline', nick: '离线账号', liveStatus: 'reconnecting' },
      { ...base, uid: 'error', nick: '错误账号', liveStatus: 'ok', lastError: '历史错误' },
    ]
    stats.totals.accountTotal = stats.accounts.length
    getDataCenterStatsMock.mockResolvedValueOnce(stats)

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })

    const options = [...screen.getByRole('combobox', { name: '账号范围' }).querySelectorAll('option')].map((option) => option.textContent)
    expect(options).toEqual(['全部账号（4）', '错误账号', '离线账号', '待处理账号', '普通账号'])
    expect(screen.getByTestId('todo-risk').textContent).toContain('未发现账号错误或会话风险')
  })

  it('shows data unavailable for an automation card whose fields are all unavailable', async () => {
    const stats = makeStats('账号甲')
    stats.automation = {
      delivery: { enabled: null, running: null, pending: null, errors: null },
      scheduledTasks: { enabled: 0, running: 0, pending: 0, errors: 0 },
      replies: { enabled: undefined, running: undefined, pending: undefined, errors: undefined },
    }
    getDataCenterStatsMock.mockResolvedValueOnce(stats)

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })

    expect(screen.getByTestId('automation-delivery').textContent).toContain('自动发货数据暂不可用运行状态暂不可用')
    expect(screen.getByTestId('automation-replies').textContent).toContain('回复自动化数据暂不可用运行状态暂不可用')
    expect(screen.getByTestId('automation-scheduled-tasks').textContent).toContain('0已安排0最近失败当前未启用')
  })

  it('treats empty live status as unsynced and ignores stale errors on healthy accounts', async () => {
    const stats = makeStats('账号甲')
    stats.accounts[0].liveStatus = 'ok'
    stats.accounts[0].lastError = '历史错误，不应计入当前风险'
    stats.accounts.push({ ...stats.accounts[0], uid: 'u2', nick: '账号乙', liveStatus: '', lastError: '' })
    stats.totals.accountTotal = 2
    getDataCenterStatsMock.mockResolvedValueOnce(stats)

    render(<MemoryRouter><Dashboard /></MemoryRouter>)
    await act(async () => { await Promise.resolve() })

    expect(screen.getByTestId('todo-risk').textContent).toContain('未发现账号错误或会话风险')
    expect(screen.getByTestId('todo-connections').textContent).toContain('1 个账号连接状态未同步')
    expect(screen.getByTestId('todo-connections').textContent).not.toContain('全部账号连接正常')
  })

  it('updates account IM status from live connection events without refreshing other stats', async () => {
    getDataCenterStatsMock
      .mockResolvedValueOnce(makeStats('账号甲'))
      .mockImplementationOnce(() => new Promise(() => {}))

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    await act(async () => {
      await Promise.resolve()
    })

    expect(screen.getByTestId('todo-connections').textContent).toContain('全部账号连接正常')
    expect(liveTransportControllers).toHaveLength(1)
    expect(liveTransportControllers[0].subscribe).toHaveBeenCalledWith(['u1'])

    await act(async () => {
      liveTransportControllers[0].options.onEvent?.({
        type: 'connection.reconnecting',
        uid: 'u1',
        reason: 'upstream reconnecting',
      })
      vi.advanceTimersByTime(100)
    })

    expect(screen.queryByText('正在刷新')).toBeNull()
    expect((screen.getByRole('button', { name: /刷新数据/ }) as HTMLButtonElement).disabled).toBe(false)
    expect(screen.getByTestId('todo-connections').textContent).toContain('1 个账号离线或重连')
    expect(screen.queryByText('账号乙')).toBeNull()
    expect(getDataCenterStatsMock).toHaveBeenCalledTimes(1)
  })

  it('coalesces realtime account status bursts before updating the dashboard', async () => {
    getDataCenterStatsMock.mockResolvedValueOnce(makeStats('账号甲'))

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    await act(async () => {
      await Promise.resolve()
    })

    expect(screen.getByTestId('todo-connections').textContent).toContain('全部账号连接正常')

    await act(async () => {
      liveTransportControllers[0].options.onEvent?.({
        type: 'connection.reconnecting',
        uid: 'u1',
        reason: 'upstream reconnecting',
      })
    })

    expect(screen.getByTestId('todo-connections').textContent).toContain('全部账号连接正常')

    await act(async () => {
      vi.advanceTimersByTime(100)
    })

    expect(screen.getByTestId('todo-connections').textContent).toContain('1 个账号离线或重连')
    expect(getDataCenterStatsMock).toHaveBeenCalledTimes(1)
  })

  it('subscribes loaded account uids when the realtime socket connects after stats load', async () => {
    getDataCenterStatsMock.mockResolvedValueOnce(makeStats('账号甲'))

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    await act(async () => {
      await Promise.resolve()
    })

    expect(screen.getByText('全部账号 · 近 7 天成交')).toBeTruthy()
    liveTransportControllers[0].subscribe.mockClear()

    await act(async () => {
      liveTransportControllers[0].options.onStateChange?.('ws')
    })

    expect(liveTransportControllers[0].subscribe).toHaveBeenCalledWith(['u1'])
  })

  it('does not resubscribe when only account realtime status changes', async () => {
    getDataCenterStatsMock.mockResolvedValueOnce(makeStats('账号甲'))

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    await act(async () => {
      await Promise.resolve()
    })

    expect(liveTransportControllers[0].subscribe).toHaveBeenCalledWith(['u1'])
    liveTransportControllers[0].subscribe.mockClear()

    await act(async () => {
      liveTransportControllers[0].options.onEvent?.({
        type: 'connection.reconnecting',
        uid: 'u1',
        reason: 'upstream reconnecting',
      })
      vi.advanceTimersByTime(100)
    })

    expect(screen.getByTestId('todo-connections').textContent).toContain('1 个账号离线或重连')
    expect(liveTransportControllers[0].subscribe).not.toHaveBeenCalled()
  })

  it('refreshes whole data only from sync events or manual refresh', async () => {
    getDataCenterStatsMock
      .mockResolvedValueOnce(makeStats('账号甲'))
      .mockRejectedValueOnce(new Error('network down'))

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    await act(async () => {
      await Promise.resolve()
    })

    expect(screen.getByText('全部账号 · 近 7 天成交')).toBeTruthy()

    await act(async () => {
      vi.advanceTimersByTime(5_000)
      await Promise.resolve()
    })

    expect(screen.getByText('全部账号 · 近 7 天成交')).toBeTruthy()
    expect(screen.queryByText('数据中心加载异常')).toBeNull()
  })
})
