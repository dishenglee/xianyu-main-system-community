import { describe, expect, it } from 'vitest'
import { isRedFlowerEligibleOrder, orderStageDisplayLabel, orderStageForOrder } from './orderStatus'

describe('orderStageForOrder', () => {
  it.each([
    ['close_by_buyer', '交易关闭', 'closed'],
    ['close_by_out', '交易关闭', 'closed'],
    ['close_by_seller', '交易关闭', 'closed'],
    ['refund_success', '交易关闭，有退款', 'closed'],
    ['refund_by_buyer', '买家申请退款,需要卖家处理', 'aftersale'],
    ['aftersale_pending', '售后处理中', 'aftersale'],
    ['seller_to_send', '等待卖家发货', 'tosend'],
    ['trade_success', '交易成功', 'done'],
    ['trade_success', '交易成功，有退款', 'done'],
  ])('maps current local status %s / %s to %s', (status, statusText, expected) => {
    expect(orderStageForOrder({ status, statusText })).toBe(expected)
  })

  it.each([
    ['WAIT_BUYER_PAY', '等待买家付款', 'pending'],
    ['buyer_to_pay', '等待买家付款', 'pending'],
    ['WAIT_SELLER_SEND_GOODS', '待发货', 'tosend'],
    ['WAIT_BUYER_CONFIRM_GOODS', '等待买家确认收货', 'sent'],
    ['buyer_to_confirm', '等待见面交易', 'sent'],
  ])('maps expected upstream status %s / %s to %s', (status, statusText, expected) => {
    expect(orderStageForOrder({ status, statusText })).toBe(expected)
  })

  it('does not silently classify unknown statuses as pending receipt', () => {
    expect(orderStageForOrder({ status: 'SOME_NEW_STATUS', statusText: '新状态' })).toBe('unknown')
  })

  it('shows the real upstream status text for unmapped states', () => {
    expect(orderStageDisplayLabel({
      status: 'some_new_state',
      statusText: '平台处理中',
    })).toBe('平台处理中')
  })

  it('shows the real upstream status text for aftersale states', () => {
    expect(orderStageDisplayLabel({
      status: 'refund_by_buyer',
      statusText: '买家申请退款,需要卖家处理',
    })).toBe('买家申请退款,需要卖家处理')
  })

  it.each([
    ['trade_success', '交易成功', true],
    ['trade_success', '交易成功，有退款', false],
    ['refund_success', '交易关闭，有退款', false],
    ['close_by_buyer', '交易关闭', false],
  ])('only treats completed non-refund orders as red flower eligible', (status, statusText, expected) => {
    expect(isRedFlowerEligibleOrder({ status, statusText })).toBe(expected)
  })
})
