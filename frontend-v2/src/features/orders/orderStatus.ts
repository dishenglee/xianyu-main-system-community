export type OrderStage = 'pending' | 'tosend' | 'sent' | 'aftersale' | 'done' | 'closed' | 'unknown'

type OrderLike = {
  status?: unknown
  statusText?: unknown
  orderStatusText?: unknown
  statusStr?: unknown
}

function statusCode(order: OrderLike) {
  return String(order?.status ?? '').trim().toUpperCase()
}

function statusText(order: OrderLike) {
  return String(order?.statusText ?? order?.orderStatusText ?? order?.statusStr ?? '').trim()
}

export function orderStatusDisplayLabel(order: OrderLike, fallback = '未知') {
  return statusText(order) || String(order?.status ?? '').trim() || fallback
}

export function orderStageDisplayLabel(order: OrderLike, fallback = '未知') {
  if (isWaitPayOrder(order)) return '待付款'
  if (isAftersaleOrder(order)) return orderStatusDisplayLabel(order, fallback)
  if (isWaitSendOrder(order)) return '待发货'
  if (isWaitReceiveOrder(order)) return '待收货'
  if (isClosedOrder(order)) return '交易关闭'
  if (isFinishedOrder(order)) return '已完成'
  return orderStatusDisplayLabel(order, fallback)
}

export function isWaitPayOrder(order: OrderLike) {
  const s = statusCode(order)
  const t = statusText(order)
  return (
    s.includes('WAIT_BUYER_PAY')
    || s.includes('BUYER_TO_PAY')
    || s === 'WAIT_PAY'
    || t.includes('待付款')
    || t.includes('等待付款')
    || t.includes('等待买家付款')
  )
}

export function isWaitSendOrder(order: OrderLike) {
  const s = statusCode(order)
  const t = statusText(order)
  return (
    s.includes('WAIT_SELLER_SEND')
    || s.includes('WAIT_SEND')
    || s.includes('SELLER_TO_SEND')
    || t.includes('待发货')
    || t.includes('等待卖家发货')
  )
}

export function isWaitReceiveOrder(order: OrderLike) {
  const s = statusCode(order)
  const t = statusText(order)
  return (
    s.includes('WAIT_BUYER_CONFIRM')
    || s.includes('BUYER_TO_CONFIRM')
    || s.includes('BUYER_CONFIRM_GOODS')
    || s.includes('WAIT_RECEIVE')
    || s.includes('CONSIGNED')
    || t.includes('待收货')
    || t.includes('确认收货')
    || t.includes('已发货')
    || t.includes('等待见面交易')
  )
}

export function isClosedOrder(order: OrderLike) {
  const s = statusCode(order)
  const t = statusText(order)
  return s.includes('CLOSE') || s.includes('CANCEL') || t.includes('关闭') || t.includes('取消')
}

export function isAftersaleOrder(order: OrderLike) {
  if (isClosedOrder(order) || isFinishedOrder(order)) return false
  const s = statusCode(order)
  const t = statusText(order)
  return (
    s.includes('REFUND')
    || s.includes('AFTERSALE')
    || s.includes('AFTER_SALE')
    || t.includes('退款')
    || t.includes('售后')
  )
}

export function isFinishedOrder(order: OrderLike) {
  if (isClosedOrder(order)) return false
  const s = statusCode(order)
  const t = statusText(order)
  return s.includes('FINISH') || s.includes('SUCCESS') || s === 'DONE' || t.includes('成功') || t.includes('完成')
}

export function isRedFlowerEligibleOrder(order: OrderLike) {
  if (!isFinishedOrder(order)) return false
  const s = statusCode(order)
  const t = statusText(order)
  return !(
    s.includes('REFUND')
    || s.includes('CLOSE')
    || s.includes('CANCEL')
    || t.includes('退款')
    || t.includes('关闭')
    || t.includes('取消')
  )
}

export function isEndedOrder(order: OrderLike) {
  return isClosedOrder(order) || isFinishedOrder(order)
}

export function orderStageForOrder(order: OrderLike): OrderStage {
  if (isWaitPayOrder(order)) return 'pending'
  if (isAftersaleOrder(order)) return 'aftersale'
  if (isWaitSendOrder(order)) return 'tosend'
  if (isWaitReceiveOrder(order)) return 'sent'
  if (isClosedOrder(order)) return 'closed'
  if (isFinishedOrder(order)) return 'done'
  return 'unknown'
}
