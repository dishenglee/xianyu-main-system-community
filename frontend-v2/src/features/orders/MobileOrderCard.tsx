// @ts-nocheck
import {
  PencilLine,
  RefreshCw,
  ShoppingCart,
  Trash2,
  Truck,
  XCircle,
  Flower2,
} from 'lucide-react'
import {
  KANBAN_META,
  colorForAccount,
  formatMoney,
  getOverdueHours,
  getOrderBuyer,
  getOrderImage,
  getOrderPrice,
  getOrderTitle,
} from './orderUtils'
import {
  isEndedOrder as isEnded,
  isRedFlowerEligibleOrder,
  orderStageDisplayLabel,
  isWaitPayOrder as isWaitPay,
  isWaitSendOrder as isWaitSend,
  orderStageForOrder as kanbanStatusForOrder,
} from './orderStatus'
import { SensitiveValue } from '../../components/SensitiveValue'
import { useSensitiveText } from '../../components/SensitiveValue'

function formatRowTime(raw) {
  // 跟 desktop OrderList.tsx 的 fullTs 一致：YYYY-MM-DD HH:MM
  const d = new Date(raw)
  if (isNaN(d.getTime())) return ''
  const y = d.getFullYear()
  const mo = String(d.getMonth() + 1).padStart(2, '0')
  const da = String(d.getDate()).padStart(2, '0')
  const hh = String(d.getHours()).padStart(2, '0')
  const mm = String(d.getMinutes()).padStart(2, '0')
  return `${y}-${mo}-${da} ${hh}:${mm}`
}

// 移动端订单卡：商品主体优先扫读；第二行左侧昵称、右侧操作保持同一行。
export function MobileOrderCard({ order, onShip, onAdjustPrice, onClose, onDelete, onRequestRedFlower, onSync, syncing, requestingRedFlower }) {
  const stage = kanbanStatusForOrder(order)
  const meta = KANBAN_META[stage] || KANBAN_META.unknown
  const overdueHours = getOverdueHours(order)
  const overdue = overdueHours > 0
  const color = colorForAccount(order._uid)
  const timeLabel = formatRowTime(order.orderCreated || order.createTime || order.gmtCreate || '')
  const title = getOrderTitle(order)
  const buyer = getOrderBuyer(order)
  const image = getOrderImage(order)
  const price = formatMoney(getOrderPrice(order))
  const orderId = order.orderId || order.bizOrderId || ''
  const maskedAccountNick = useSensitiveText('account', order._nick || '订单')
  const statusLabel = orderStageDisplayLabel(order)
  const actionButtonClass = 'flex h-7 w-7 items-center justify-center rounded-full cursor-pointer active:opacity-90 disabled:opacity-40'
  const actionStyle = { background: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' }

  return (
    <div
      className="mobile-surface-card mx-3 mb-3 px-4 py-4"
    >
      {(orderId || timeLabel) ? (
        <div className="mb-3 flex items-center justify-between gap-3">
          <div className="min-w-0 flex-1">
            {orderId ? (
              <div className="truncate font-mono text-[10.5px]" style={{ color: 'var(--text-muted)' }}>
                <SensitiveValue kind="order-id" value={orderId} />
              </div>
            ) : null}
          </div>
          {timeLabel ? (
            <div className="shrink-0 font-mono text-[10.5px]" style={{ color: 'var(--text-muted)' }}>
              {timeLabel}
            </div>
          ) : null}
        </div>
      ) : null}

      <div className="flex items-start gap-1">
        <div className="w-[52px] shrink-0">
          <div
            className="flex h-12 w-12 items-center justify-center overflow-hidden rounded-[16px]"
            style={{ background: 'var(--surface-sunken)' }}
          >
            {image ? (
              <img
                src={image}
                alt=""
                className="w-full h-full object-cover"
              />
            ) : (
              <ShoppingCart className="h-4 w-4" style={{ color: 'var(--text-tertiary)' }} />
            )}
          </div>

          <div className="mt-2 text-[16px] font-semibold leading-none" style={{ color: 'var(--primary)' }}>
            <SensitiveValue kind="money" value={price} />
          </div>
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0 flex-1">
              <p
                className="line-clamp-2 text-[15px] font-medium leading-snug"
                style={{ color: 'var(--text-main)' }}
              >
                <SensitiveValue kind="item-title" value={title} />
              </p>
            </div>
            <div className="flex shrink-0 flex-col items-end gap-1.5">
              <span
                className="mobile-state-badge"
                style={{ background: meta.bg, color: meta.tone }}
              >
                {statusLabel}
              </span>
            </div>
          </div>

          <div className="mt-2 flex items-center justify-between gap-2">
            <div className="min-w-0 flex flex-1 items-center gap-1.5 text-[11.5px]" style={{ color: 'var(--text-secondary)' }}>
              <span className="mobile-pill mobile-muted-pill max-w-[132px] truncate px-2.5 py-1 text-[10px]" style={{ color }}>
                <SensitiveValue kind="account" value={order._nick} />
              </span>
              <SensitiveValue kind="buyer" className="min-w-0 truncate" value={buyer} />
            </div>
            <div className="flex shrink-0 items-center gap-1">
              {isWaitSend(order) ? (
                <button
                  onClick={() => onShip(order._uid, order)}
                  className={actionButtonClass}
                  style={{ background: 'var(--primary)', color: '#fff' }}
                  title="发货"
                  aria-label="发货"
                >
                  <Truck className="w-3.5 h-3.5" />
                </button>
              ) : null}
              {isWaitPay(order) ? (
                <button
                  onClick={() => onAdjustPrice(order._uid, order)}
                  className={actionButtonClass}
                  style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}
                  title="改价"
                  aria-label="改价"
                >
                  <PencilLine className="w-3.5 h-3.5" />
                </button>
              ) : null}
              {(isWaitPay(order) || isWaitSend(order)) ? (
                <button
                  onClick={() => onClose(order._uid, order)}
                  className={actionButtonClass}
                  style={actionStyle}
                  title="关闭订单"
                  aria-label="关闭订单"
                >
                  <XCircle className="w-3.5 h-3.5" />
                </button>
              ) : null}
              {isEnded(order) ? (
                <button
                  onClick={() => onDelete(order._uid, order)}
                  className={actionButtonClass}
                  style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
                  title="删除订单"
                  aria-label="删除订单"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              ) : null}
              {isRedFlowerEligibleOrder(order) ? (
                <button
                  onClick={() => onRequestRedFlower(order._uid, order)}
                  disabled={requestingRedFlower}
                  className={actionButtonClass}
                  style={{ background: 'var(--status-success-bg)', color: 'var(--success)' }}
                  title="求小红花"
                  aria-label="求小红花"
                >
                  <Flower2 className={`w-3.5 h-3.5 ${requestingRedFlower ? 'animate-pulse' : ''}`} />
                </button>
              ) : null}
              <button
                onClick={() => onSync(order._uid)}
                disabled={syncing}
                title={`同步 ${maskedAccountNick}`}
                aria-label="同步订单"
                className={actionButtonClass}
                style={actionStyle}
              >
                <RefreshCw className={`h-3.5 w-3.5 ${syncing ? 'animate-spin' : ''}`} />
              </button>
            </div>
          </div>

          {overdue ? (
            <div className="mt-2">
              <span className="btn-danger rounded-full px-2 py-1 text-[10px] font-semibold">
                超 {overdueHours}h
              </span>
            </div>
          ) : null}
        </div>
      </div>

    </div>
  )
}
