import type { Order, Pagination, RequestOptions } from './types'
import { request } from './client'

// ==================== 交易订单 ====================

export const tradeSold = (uid: string, page = 1, status = 'all', { force = false } = {}) =>
  request<{ orders: Order[]; pagination: Pagination }>(
    'GET', `/api/trade/sold?uid=${uid}&page=${page}&status=${status}${force ? '&force=true' : ''}`,
  )
export const tradeOrders = (uid: string, page = 1, seller = true) =>
  request('GET', `/api/trade/orders?uid=${uid}&page=${page}&seller=${seller}`)
export const tradeBill = (uid: string) => request('GET', `/api/trade/bill?uid=${uid}`)

// ==================== 订单操作 ====================

export const orderModifyPriceRender = (uid: string, bizOrderId: string) =>
  request('POST', '/api/order/modify_price/render', { uid, biz_order_id: bizOrderId })
export const orderAdjustPrice = (uid: string, bizOrderId: string, modifyFee: number) =>
  request('POST', '/api/order/adjust_price', { uid, biz_order_id: bizOrderId, modify_fee: modifyFee })
export const tradeClose = (uid: string, orderId: string, reason = '不想卖了') =>
  request('POST', '/api/trade/close', { uid, order_id: orderId, reason })
export const tradeAdjustPrice = (uid: string, orderId: string, newPrice: string | number, newTransportFee: string | number = '0') =>
  request('POST', '/api/trade/adjust_price', {
    uid, order_id: orderId,
    new_price: String(newPrice), new_transport_fee: String(newTransportFee),
  })
export const orderDelete = (uid: string, bizOrderId: string) =>
  request('POST', '/api/order/delete', { uid, biz_order_id: bizOrderId })
export const orderCloseReason = (uid: string, bizOrderId: string) =>
  request('GET', `/api/order/close_reason?uid=${uid}&biz_order_id=${bizOrderId}`)
export const orderClose = (uid: string, bizOrderId: string, closeReason = '') =>
  request('POST', '/api/order/close', { uid, biz_order_id: bizOrderId, close_reason: closeReason })
export const orderConsignRender = (uid: string, bizOrderId: string) =>
  request('POST', '/api/order/consign/render', { uid, biz_order_id: bizOrderId })
export const orderConsign = (uid: string, bizOrderId: string, companyCode: string, companyName: string, mailNo: string, addressId = '') =>
  request('POST', '/api/order/consign', {
    uid, biz_order_id: bizOrderId,
    company_code: companyCode, company_name: companyName, mail_no: mailNo, address_id: addressId,
  })
export const orderConsignDummy = (uid: string, bizOrderId: string) =>
  request('POST', '/api/order/consign_dummy', { uid, biz_order_id: bizOrderId })
export const requestRedFlower = (uid: string, bizOrderId: string) =>
  request('POST', '/api/order/red-flower', { uid, biz_order_id: bizOrderId })

// ==================== 闲鱼币 ====================

export const coinDeductionOpen = (uid: string, itemIds: string[], ratio: string) =>
  request('POST', '/api/coin/deduction/open', { uid, item_ids: itemIds, ratio })
export const coinDeductionClose = (uid: string, itemIds: string[]) =>
  request('POST', '/api/coin/deduction/close', { uid, item_ids: itemIds })
export const coinInfo = (uid: string) => request('GET', `/api/coin/info?uid=${uid}`)
export const coinAccount = (uid: string) => request('GET', `/api/coin/account?uid=${uid}`)
export const coinDeductionRatio = (uid: string, price = '100', itemId = '') =>
  request(
    'GET',
    `/api/coin/deduction/ratio?uid=${encodeURIComponent(uid)}&price=${encodeURIComponent(price)}${itemId ? `&item_id=${encodeURIComponent(itemId)}` : ''}`,
  )

// ==================== S04 aliases ====================

export const syncOrders = (uid: string, seller = true) =>
  request<{ ok: boolean; total: number; synced_at: number; detail?: string }>(
    'POST', '/api/orders/sync', { uid, seller },
  )
