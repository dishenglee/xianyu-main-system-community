// @ts-nocheck
import React from 'react'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const tradeSoldMock = vi.fn()
const orderConsignRenderMock = vi.fn()
const orderAdjustPriceMock = vi.fn()
const orderDeleteMock = vi.fn()
const requestRedFlowerMock = vi.fn()
const syncOrdersMock = vi.fn()
const accountStoreMock = {
  accounts: [{ uid: '90000000002', nick: '测试账号', status: 'active' }],
  loadAccounts: vi.fn().mockResolvedValue(undefined),
}

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../api/index', () => ({
  tradeSold: (...args) => tradeSoldMock(...args),
  orderConsignRender: (...args) => orderConsignRenderMock(...args),
  orderAdjustPrice: (...args) => orderAdjustPriceMock(...args),
  orderDelete: (...args) => orderDeleteMock(...args),
  requestRedFlower: (...args) => requestRedFlowerMock(...args),
  orderClose: vi.fn(),
  orderConsign: vi.fn(),
  orderConsignDummy: vi.fn(),
  syncOrders: (...args) => syncOrdersMock(...args),
  isSuccess: () => true,
  retMsg: () => '',
}))

import { NotifyProvider } from '../../components/Notify'
import { AppDialogProvider } from '../../components/AppDialog'
import { DesktopOrders } from './DesktopOrders'

function findRowForText(text) {
  const node = screen.getByText(text)
  return node.closest('[data-testid="order-card"]') || node
}

function renderPage() {
  return render(
    <NotifyProvider>
      <AppDialogProvider>
        <MemoryRouter>
          <DesktopOrders />
        </MemoryRouter>
      </AppDialogProvider>
    </NotifyProvider>,
  )
}

describe('DesktopOrders status classification', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  beforeEach(() => {
    tradeSoldMock.mockReset()
    orderConsignRenderMock.mockReset()
    orderAdjustPriceMock.mockReset()
    orderDeleteMock.mockReset()
    requestRedFlowerMock.mockReset()
    syncOrdersMock.mockReset()
    accountStoreMock.accounts = [{ uid: '90000000002', nick: '测试账号', status: 'active' }]
    accountStoreMock.loadAccounts.mockClear()
    orderAdjustPriceMock.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    orderDeleteMock.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    requestRedFlowerMock.mockResolvedValue({ success: true })
    syncOrdersMock.mockResolvedValue({ ok: true })
    orderConsignRenderMock.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        idleLogisticTypes: ['OFFLINE_CONSIGN', 'DUMMY_CONSIGN'],
        senderAddressInfo: {
          addressId: '16696353765',
          fullName: '小六子',
          mobile: '15580695130',
          formatFullAddress: '湖南省长沙市岳麓区望城坡街道水木兰亭西门菜鸟驿站',
        },
        receiverAddressInfo: {
          fullName: '李大叔',
          mobile: '18817139918',
          formatFullAddress: '湖南省长沙市岳麓区望城坡街道商学院附近的沙里坪小区',
        },
      },
    })
    window.matchMedia = vi.fn().mockImplementation((query) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }))
  })

  it('keeps desktop order KPIs and sync in the order header without a redundant hub tab', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: 'ORDER-HEADER-KPI',
          bizOrderId: 'ORDER-HEADER-KPI',
          title: '订单头部指标商品',
          status: 'seller_to_send',
          statusText: '等待卖家发货',
          buyerNick: '测试买家',
          price: '88',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    })

    renderPage()

    const heading = screen.getByRole('heading', { name: '订单' })
    const headerSection = heading.closest('section')
    expect(screen.queryByRole('link', { name: '我的订单' })).toBeNull()
    expect(screen.queryByText('我的订单')).toBeNull()
    expect(headerSection).toBeTruthy()

    const header = within(headerSection!)
    const kpiLabels = ['订单总数', '待付款', '待发货', '售后', '已完成', '交易关闭', '累计金额']
    for (const label of kpiLabels) {
      expect(await header.findByText(label)).toBeTruthy()
    }

    const syncButton = header.getByRole('button', { name: '同步全部订单' })
    const visibilityButton = header.getByRole('button', { name: /隐藏敏感信息|显示敏感信息/ })
    const kpiGrid = header.getByText('订单总数').closest('.grid')
    expect(headerSection!.contains(syncButton)).toBe(true)
    expect(headerSection!.contains(visibilityButton)).toBe(true)
    expect(kpiGrid).toBeTruthy()
    expect(headerSection!.contains(kpiGrid)).toBe(true)
  })

  it('filters desktop orders by one or multiple accounts and restores all accounts', async () => {
    accountStoreMock.accounts = [
      { uid: 'account-a', nick: '账号甲', status: 'active' },
      { uid: 'account-b', nick: '账号乙', status: 'active' },
      { uid: 'account-c', nick: '账号丙', status: 'active' },
    ]
    tradeSoldMock.mockImplementation(async (uid) => ({
      _last_synced_at: 0,
      orders: [
        {
          orderId: `ORDER-${uid}`,
          bizOrderId: `ORDER-${uid}`,
          title: `${uid} 的订单`,
          status: 'seller_to_send',
          statusText: '等待卖家发货',
          buyerNick: `${uid} 买家`,
          price: '10',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    }))

    renderPage()

    await screen.findByText('account-a 的订单')
    expect(screen.getByText('account-b 的订单')).toBeTruthy()
    expect(screen.getByText('account-c 的订单')).toBeTruthy()
    expect(screen.getByText('订单总数').parentElement?.textContent).toContain('3')
    expect(screen.getByRole('button', { name: /待发货3/ })).toBeTruthy()
    expect(screen.getByTitle('跳到待发货').textContent).toContain('3 单超 24h')

    let accountButton = screen.getByRole('button', { name: '账号选择：全部账号' })
    expect(accountButton.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(accountButton)
    expect(accountButton.getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByRole('checkbox', { name: '全部账号' }).checked).toBe(true)

    fireEvent.keyDown(document, { key: 'Escape' })
    expect(accountButton.getAttribute('aria-expanded')).toBe('false')
    expect(document.activeElement).toBe(accountButton)

    fireEvent.click(accountButton)
    fireEvent.mouseDown(document.body)
    expect(accountButton.getAttribute('aria-expanded')).toBe('false')

    fireEvent.click(accountButton)
    fireEvent.click(screen.getByRole('checkbox', { name: '选择账号 账号甲' }))
    await waitFor(() => {
      expect(screen.getByText('account-a 的订单')).toBeTruthy()
      expect(screen.queryByText('account-b 的订单')).toBeNull()
      expect(screen.queryByText('account-c 的订单')).toBeNull()
      expect(screen.getByText('订单总数').parentElement?.textContent).toContain('1')
      expect(screen.getByRole('button', { name: /待发货1/ })).toBeTruthy()
      expect(screen.getByTitle('跳到待发货').textContent).toContain('1 单超 24h')
    })
    expect(screen.getByRole('button', { name: '账号选择：账号甲' })).toBeTruthy()

    fireEvent.click(screen.getByRole('checkbox', { name: '选择账号 账号甲' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '账号选择：全部账号' })).toBeTruthy()
      expect(screen.getByText('account-a 的订单')).toBeTruthy()
      expect(screen.getByText('account-b 的订单')).toBeTruthy()
      expect(screen.getByText('account-c 的订单')).toBeTruthy()
    })

    fireEvent.click(screen.getByRole('checkbox', { name: '选择账号 账号甲' }))
    fireEvent.click(screen.getByRole('checkbox', { name: '选择账号 账号乙' }))
    await waitFor(() => {
      expect(screen.getByText('account-a 的订单')).toBeTruthy()
      expect(screen.getByText('account-b 的订单')).toBeTruthy()
      expect(screen.queryByText('account-c 的订单')).toBeNull()
      expect(screen.getByText('订单总数').parentElement?.textContent).toContain('2')
      expect(screen.getByRole('button', { name: /待发货2/ })).toBeTruthy()
      expect(screen.getByTitle('跳到待发货').textContent).toContain('2 单超 24h')
    })
    expect(screen.getByRole('button', { name: '账号选择：已选 2 个账号' })).toBeTruthy()

    fireEvent.click(screen.getByRole('checkbox', { name: '全部账号' }))
    await waitFor(() => {
      expect(screen.getByText('account-a 的订单')).toBeTruthy()
      expect(screen.getByText('account-b 的订单')).toBeTruthy()
      expect(screen.getByText('account-c 的订单')).toBeTruthy()
      expect(screen.getByText('订单总数').parentElement?.textContent).toContain('3')
      expect(screen.getByRole('button', { name: /待发货3/ })).toBeTruthy()
      expect(screen.getByTitle('跳到待发货').textContent).toContain('3 单超 24h')
    })
    accountButton = screen.getByRole('button', { name: '账号选择：全部账号' })
    expect(accountButton).toBeTruthy()
  })

  it('resets stale account selection and constrains long account summaries', async () => {
    const longNick = '这是一个用于验证不会挤压搜索框的特别特别长账号昵称'
    accountStoreMock.accounts = [
      { uid: 'account-long', nick: longNick, status: 'active' },
      { uid: 'account-peer', nick: '同行账号', status: 'active' },
    ]
    tradeSoldMock.mockImplementation(async (uid) => ({
      _last_synced_at: 0,
      orders: [
        {
          orderId: `ORDER-${uid}`,
          bizOrderId: `ORDER-${uid}`,
          title: `${uid} 的订单`,
          status: 'seller_to_send',
          statusText: '等待卖家发货',
          buyerNick: `${uid} 买家`,
          price: '10',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    }))

    const { rerender } = renderPage()
    await screen.findByText('account-long 的订单')

    fireEvent.click(screen.getByRole('button', { name: '账号选择：全部账号' }))
    fireEvent.click(screen.getByRole('checkbox', { name: `选择账号 ${longNick}` }))

    const longSummaryButton = await screen.findByRole('button', { name: `账号选择：${longNick}` })
    expect(longSummaryButton.className).toContain('w-[168px]')
    expect(longSummaryButton.className).toContain('max-w-[168px]')
    expect(longSummaryButton.getAttribute('title')).toBe(longNick)
    expect(screen.queryByText('account-peer 的订单')).toBeNull()

    accountStoreMock.accounts = [{ uid: 'account-new', nick: '新账号', status: 'active' }]
    rerender(
      <NotifyProvider>
        <AppDialogProvider>
          <MemoryRouter>
            <DesktopOrders />
          </MemoryRouter>
        </AppDialogProvider>
      </NotifyProvider>,
    )

    await screen.findByText('account-new 的订单')
    expect(screen.getByRole('button', { name: '账号选择：全部账号' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: /已选 0 个账号/ })).toBeNull()
    expect(screen.queryByText('account-long 的订单')).toBeNull()
    expect(screen.getByText('订单总数').parentElement?.textContent).toContain('1')
  })

  it('renders close_by_buyer orders as transaction closed instead of completed', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: '4700963197372937303',
          bizOrderId: '4700963197372937303',
          title: 'iPadPro11寸2022版本m2。仅限自提。价格可以商量',
          status: 'close_by_buyer',
          statusText: '交易关闭',
          buyerNick: '小鹏同学',
          price: '1000',
          orderCreated: '2026-05-02 10:00:00',
        },
      ],
    })

    renderPage()

    await screen.findByText('4700963197372937303')
    const row = findRowForText('4700963197372937303')

    expect(row.textContent).toContain('交易关闭')
    expect(row.textContent).not.toContain('已完成')
  })

  it('renders shipped orders waiting for buyer confirmation as pending receipt', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: '4502276319054045503',
          bizOrderId: '4502276319054045503',
          title: '罗技G PRO 60无线机械键盘，全新正品，支持无线连接，手',
          status: 'buyer_to_confirm',
          statusText: '等待见面交易',
          buyerNick: '测试买家',
          price: '0.01',
          orderCreated: '2026-05-02 11:00:00',
        },
      ],
    })

    renderPage()

    await screen.findByText('4502276319054045503')
    const row = findRowForText('4502276319054045503')

    expect(row.textContent).toContain('待收货')
    expect(row.textContent).not.toContain('已发货')
  })

  it('renders seller_to_send orders as waiting for shipment instead of pending receipt', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: '4502276319054045503',
          bizOrderId: '4502276319054045503',
          title: '罗技G PRO 60无线机械键盘，全新正品，支持无线连接，手',
          status: 'seller_to_send',
          statusText: '等待卖家发货',
          buyerNick: '糯叽叽的墨染呀',
          price: '0.01',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    })

    renderPage()

    await screen.findByText('4502276319054045503')
    const row = findRowForText('4502276319054045503')

    expect(row.textContent).toContain('待发货')
    expect(row.textContent).not.toContain('待收货')
  })

  it('uses compact two-column desktop rows and keeps refund handling risk readable', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: 'REFUND-NEEDS-HANDLING',
          bizOrderId: 'REFUND-NEEDS-HANDLING',
          title: '退款待处理商品',
          status: 'refund_by_buyer',
          statusText: '买家申请退款,需要卖家处理',
          buyerNick: '申请退款的买家',
          price: '88',
          orderCreated: '2026-05-02 11:30:00',
        },
        {
          orderId: 'COMPACT-NORMAL-ORDER',
          bizOrderId: 'COMPACT-NORMAL-ORDER',
          title: '用于验证桌面紧凑布局的超长普通商品标题',
          status: 'seller_to_send',
          statusText: '等待卖家发货',
          buyerNick: '普通订单买家',
          price: '128.00',
          orderCreated: '2026-05-02 11:35:00',
        },
      ],
    })

    const { container } = renderPage()

    await screen.findByText('REFUND-NEEDS-HANDLING')
    const grid = container.querySelector('[data-testid="desktop-order-grid"]')
    const cards = screen.getAllByTestId('order-card')
    const normalCard = findRowForText('COMPACT-NORMAL-ORDER')
    const refundCard = findRowForText('REFUND-NEEDS-HANDLING')

    expect(grid?.className).toContain('grid-cols-2')
    expect(cards).toHaveLength(2)
    expect(cards.every((card) => card.className.includes('min-h-[116px]'))).toBe(true)
    expect(normalCard.querySelector('[data-testid="order-card-thumb"]')?.className).toContain('h-12')
    expect(normalCard.querySelector('[data-testid="order-card-top"]')?.textContent).toContain('待发货')
    expect(normalCard.querySelector('[data-testid="order-card-top"]')?.textContent).toContain('¥128.00')
    expect(normalCard.querySelector('[data-testid="order-card-meta"]')?.textContent).toContain('普通订单买家')
    expect(normalCard.querySelector('[data-testid="order-card-footer"]')?.textContent).toContain('COMPACT-NORMAL-ORDER')

    const risk = refundCard.querySelector('[data-testid="order-card-risk"]')
    expect(refundCard.querySelector('[data-testid="order-card-top"]')?.textContent).toContain('退款待处理')
    expect(risk?.textContent).toContain('买家申请退款')
    expect(risk?.textContent).toContain('需卖家处理')
    expect(screen.queryByRole('button', { name: /处理退款/ })).toBeNull()
  })

  it('opens a shipment modal with logistics/no-logistics choices and copyable addresses', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: '4502276319054045503',
          bizOrderId: '4502276319054045503',
          title: '罗技G PRO 60无线机械键盘，全新正品，支持无线连接，手',
          status: 'seller_to_send',
          statusText: '等待卖家发货',
          buyerNick: '糯叽叽的墨染呀',
          price: '0.01',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    })

    renderPage()

    await screen.findByText('4502276319054045503')
    fireEvent.click(screen.getByRole('button', { name: '发货' }))

    await screen.findByText('物流发货')
    expect(screen.getByText('无需物流')).toBeTruthy()
    expect(screen.getByText('寄件地址')).toBeTruthy()
    expect(screen.getByText('收货地址')).toBeTruthy()
    expect(await screen.findByText(/小六子/)).toBeTruthy()
    expect(await screen.findByText(/李大叔/)).toBeTruthy()
    expect(screen.getAllByRole('button', { name: '复制地址' }).length).toBeGreaterThanOrEqual(2)
    expect(screen.queryByText('必要时先改价')).toBeNull()
    expect(screen.queryByText('保持原业务能力，改价成功后会自动刷新订单。')).toBeNull()

    await waitFor(() => {
      expect(orderConsignRenderMock).toHaveBeenCalledWith('90000000002', '4502276319054045503')
    })
  })

  it('keeps price adjustment available for pending payment orders and refreshes afterward', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: 'WAITPAY123',
          bizOrderId: 'WAITPAY123',
          title: '待付款可改价商品',
          status: 'WAIT_BUYER_PAY',
          statusText: '等待买家付款',
          buyerNick: '测试买家',
          price: '99',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    })
    renderPage()

    await screen.findByText('WAITPAY123')
    fireEvent.click(screen.getByRole('button', { name: '改价' }))
    const priceInput = await screen.findByPlaceholderText('例如 12.34')
    fireEvent.change(priceInput, { target: { value: '88.50' } })
    fireEvent.click(screen.getByRole('button', { name: '确认改价' }))

    await waitFor(() => {
      expect(orderAdjustPriceMock).toHaveBeenCalledWith('90000000002', 'WAITPAY123', 88.5)
      expect(tradeSoldMock).toHaveBeenCalledWith('90000000002', 1, 'all', { force: false })
    })
  })

  it('removes the deleted order from the list immediately without waiting for sync', async () => {
    syncOrdersMock.mockImplementation(() => new Promise(() => {}))
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: 'CLOSED-DELETE-1',
          bizOrderId: 'CLOSED-DELETE-1',
          title: '待删除订单',
          status: 'close_by_buyer',
          statusText: '交易关闭',
          buyerNick: '测试买家',
          price: '10',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    })

    renderPage()

    await screen.findByText('CLOSED-DELETE-1')
    fireEvent.click(screen.getByTitle('删除订单'))
    fireEvent.click(await screen.findByRole('button', { name: '删除' }))

    await screen.findByText('已删除订单')
    await waitFor(() => {
      expect(screen.queryByText('CLOSED-DELETE-1')).toBeNull()
    })
  })

  it('only offers eligible completed orders a confirmation-protected red flower request', async () => {
    tradeSoldMock.mockResolvedValue({
      _last_synced_at: 0,
      orders: [
        {
          orderId: 'DONE-RED-FLOWER-1',
          bizOrderId: 'DONE-RED-FLOWER-1',
          title: '已完成订单',
          status: 'trade_success',
          statusText: '交易成功',
          buyerNick: '测试买家',
          price: '10',
          orderCreated: '2026-05-02 11:30:00',
        },
        {
          orderId: 'REFUND-NO-RED-FLOWER-1',
          bizOrderId: 'REFUND-NO-RED-FLOWER-1',
          title: '已完成但退款订单',
          status: 'trade_success',
          statusText: '交易成功，有退款',
          buyerNick: '测试买家',
          price: '10',
          orderCreated: '2026-05-02 11:30:00',
        },
      ],
    })

    renderPage()

    await screen.findByText('DONE-RED-FLOWER-1')
    expect(screen.getAllByRole('button', { name: '求小红花' })).toHaveLength(1)
    fireEvent.click(screen.getByRole('button', { name: '求小红花' }))
    expect(await screen.findByText('将向买家发起求小红花请求，是否继续？')).toBeTruthy()
    expect(requestRedFlowerMock).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: '确认请求' }))
    await waitFor(() => {
      expect(requestRedFlowerMock).toHaveBeenCalledWith('90000000002', 'DONE-RED-FLOWER-1')
    })
  })
})
