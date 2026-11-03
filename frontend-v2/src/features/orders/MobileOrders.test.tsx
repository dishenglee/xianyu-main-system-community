import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { MobileOrders } from './MobileOrders'

const ordersState = vi.hoisted(() => ({ current: null as any }))

vi.mock('./hooks/useOrdersPageState', () => ({
  useOrdersPageState: () => ordersState.current,
}))

vi.mock('./MobileShippingSheet', () => ({
  MobileShippingSheet: () => null,
}))

function makeOrder(index: number) {
  return {
    _uid: 'uid-1',
    _nick: '测试账号',
    orderId: `ORDER-${index}`,
    bizOrderId: `ORDER-${index}`,
    title: `测试商品 ${index}`,
    status: index % 2 === 0 ? 'DONE' : 'WAIT_BUYER_PAY',
    statusText: index % 2 === 0 ? '已完成' : '等待买家付款',
    buyerNick: `买家 ${index}`,
    price: `${index}`,
    orderCreated: '2026-07-05 11:47:00',
  }
}

function makeOrdersState() {
  const allOrders = Array.from({ length: 8 }, (_, index) => makeOrder(index + 1))
  return {
    activeAccounts: [{ uid: 'uid-1', nick: '测试账号', status: 'active' }],
    loading: false,
    groups: [],
    syncingUids: {},
    requestingRedFlowerIds: {},
    allOrders,
    accountStats: [{ uid: 'uid-1', nick: '测试账号', total: allOrders.length, waitSendCount: 0, overdueCount: 0, lastSyncedAt: 0 }],
    totalWaitSend: 0,
    totalOverdue: 0,
    shipModal: null,
    setShipModal: vi.fn(),
    shipForm: {},
    setShipForm: vi.fn(),
    shipMode: 'offline',
    setShipMode: vi.fn(),
    shipContextLoading: false,
    shipContextError: '',
    senderAddress: null,
    receiverAddress: null,
    copiedAddress: '',
    shipping: false,
    logisticsDisabled: false,
    loadAllOrders: vi.fn(),
    syncOne: vi.fn(),
    syncAll: vi.fn(),
    handleDelete: vi.fn(),
    handleClose: vi.fn(),
    handleAdjustPrice: vi.fn(),
    handleRequestRedFlower: vi.fn(),
    handleShipmentSubmit: vi.fn(),
    copyAddress: vi.fn(),
  }
}

describe('MobileOrders layout', () => {
  beforeEach(() => {
    ordersState.current = makeOrdersState()
  })

  it('keeps order cards inside a bounded scroll viewport below the filters', () => {
    const { container } = render(<MobileOrders />)

    const listCard = screen.getByText('订单列表').closest('section')
    expect(listCard).not.toBeNull()
    expect(listCard!.classList.contains('flex')).toBe(true)
    expect(listCard!.classList.contains('flex-col')).toBe(true)

    const scrollViewport = container.querySelector('.overflow-y-auto.h-full')
    expect(scrollViewport).not.toBeNull()
    expect(scrollViewport!.parentElement!.classList.contains('flex-1')).toBe(true)
    expect(scrollViewport!.parentElement!.classList.contains('overflow-hidden')).toBe(true)
    expect(screen.getByText('ORDER-8')).toBeTruthy()
  })

  it('only exposes 求小红花 for completed mobile orders', () => {
    render(<MobileOrders />)

    expect(screen.getAllByRole('button', { name: '求小红花' })).toHaveLength(4)
  })
})
