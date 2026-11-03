// @ts-nocheck
import {
  AlertTriangle,
  PencilLine,
  RefreshCw,
  RotateCcw,
  ShoppingCart,
  Truck,
  XCircle,
  Trash2,
  Flower2,
} from 'lucide-react'
import {
  KANBAN_META,
  colorForAccount,
  getOverdueHours,
} from './orderUtils'
import {
  isWaitPayOrder as isWaitPay,
  isWaitSendOrder as isWaitSend,
  isEndedOrder as isEnded,
  isRedFlowerEligibleOrder,
  orderStageDisplayLabel,
  orderStageForOrder as kanbanStatusForOrder,
} from './orderStatus'
import { SensitiveValue, useSensitiveAvatarInitial, useSensitiveText } from '../../components/SensitiveValue'

export function filterOrdersForList(orders, statusFilter, searchQuery) {
  let list = orders
  if (statusFilter !== 'all') {
    list = list.filter((o) => kanbanStatusForOrder(o) === statusFilter)
  }
  if (searchQuery && searchQuery.trim()) {
    const q = searchQuery.trim().toLowerCase()
    list = list.filter((o) => {
      const title = String(o.title || o.itemTitle || '').toLowerCase()
      const oid = String(o.orderId || o.bizOrderId || '').toLowerCase()
      const buyer = String(o.buyerNick || o.buyerId || '').toLowerCase()
      return title.includes(q) || oid.includes(q) || buyer.includes(q)
    })
  }
  // 时间倒序
  return [...list].sort((a, b) => {
    const ra = String(a.orderCreated || a.createTime || a.gmtCreate || '')
    const rb = String(b.orderCreated || b.createTime || b.gmtCreate || '')
    return rb.localeCompare(ra)
  })
}

export function OrderList({ orders, onShip, onAdjustPrice, onClose, onDelete, onRequestRedFlower, onSync, syncingUids, requestingRedFlowerIds = {} }) {
  if (!orders.length) {
    return (
      <div className="surface-card rounded-[14px] py-14 text-center text-[13px]" style={{ color: 'var(--text-secondary)' }}>
        暂无匹配的订单
      </div>
    )
  }
  // 按天分组 sticky
  const groupedByDay = (() => {
    const now = new Date()
    const map = new Map()
    for (const o of orders) {
      const raw = o.orderCreated || o.createTime || o.gmtCreate || ''
      const d = new Date(raw)
      let bucket = '更早'
      if (!isNaN(d.getTime())) {
        const dateDay = d.toDateString()
        const today = now.toDateString()
        const yesterday = new Date(now.getTime() - 86400_000).toDateString()
        if (dateDay === today) bucket = '今天'
        else if (dateDay === yesterday) bucket = '昨天'
        else if (now.getTime() - d.getTime() < 7 * 86400_000) bucket = '本周'
      }
      if (!map.has(bucket)) map.set(bucket, [])
      map.get(bucket).push(o)
    }
    const order = ['今天', '昨天', '本周', '更早']
    return order.filter((k) => map.has(k)).map((k) => [k, map.get(k)])
  })()

  return (
    <div className="surface-card rounded-[14px] overflow-hidden">
      {groupedByDay.map(([day, list]) => (
        <div key={day}>
          <div
            className="px-5 py-1.5 text-[11px] font-semibold tracking-wider flex items-center gap-2 sticky"
            style={{ background: 'rgba(250,250,252,0.78)', backdropFilter: 'blur(12px)', color: 'var(--text-secondary)', borderBottom: '1px solid var(--control-border)', top: 0 }}
          >
            {day}
            <span className="h-px flex-1" style={{ background: 'var(--control-border)' }} />
            <span className="font-mono normal-case opacity-70">{list.length} 单</span>
          </div>
          <div data-testid="desktop-order-grid" className="grid grid-cols-2 items-stretch gap-2.5 p-2.5">
            {list.map((o, idx) => (
              <OrderRow
                key={`${o._uid}-${o.orderId || o.bizOrderId || idx}`}
                o={o}
                onShip={onShip}
                onAdjustPrice={onAdjustPrice}
                onClose={onClose}
                onDelete={onDelete}
                onRequestRedFlower={onRequestRedFlower}
                onSync={onSync}
                syncing={!!syncingUids[o._uid]}
                requestingRedFlower={!!requestingRedFlowerIds[`${o._uid}:${o.bizOrderId || o.orderId}`]}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

function OrderRow({ o, onShip, onAdjustPrice, onClose, onDelete, onRequestRedFlower, onSync, syncing, requestingRedFlower }) {
  const maskedAccountNick = useSensitiveText('account', o._nick || '订单')
  const accountAvatarInitial = useSensitiveAvatarInitial(o._nick || '?')
  const kStatus = kanbanStatusForOrder(o)
  const meta = KANBAN_META[kStatus] || KANBAN_META.unknown
  const statusLabel = orderStageDisplayLabel(o)
  const overdueHours = getOverdueHours(o)
  const overdue = overdueHours > 0
  const color = colorForAccount(o._uid)
  const rawStatus = String(o.statusText || o.orderStatusText || o.statusStr || '')
  const refundPending = kStatus === 'aftersale'
    && rawStatus.includes('买家申请退款')
    && rawStatus.includes('需要卖家处理')
  const fullTs = (() => {
    const raw = o.orderCreated || o.createTime || o.gmtCreate || ''
    const d = new Date(raw)
    if (isNaN(d.getTime())) return ''
    const y = d.getFullYear()
    const mo = String(d.getMonth() + 1).padStart(2, '0')
    const da = String(d.getDate()).padStart(2, '0')
    const hh = String(d.getHours()).padStart(2, '0')
    const mm = String(d.getMinutes()).padStart(2, '0')
    return `${y}-${mo}-${da} ${hh}:${mm}`
  })()
  const amount = String(o.price || o.totalFee || '—')
  const iconButtonClass = 'flex h-7 w-7 shrink-0 items-center justify-center rounded-[7px] cursor-pointer transition-colors hover:bg-black/[0.06] disabled:opacity-40'
  return (
    <div
      data-testid="order-card"
      className="flex h-full min-h-[116px] min-w-0 gap-2.5 rounded-[10px] border p-3 transition-colors hover:bg-black/[0.015]"
      style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card-strong)' }}
    >
      <div data-testid="order-card-thumb" className="relative h-12 w-12 shrink-0">
        <div className="flex h-12 w-12 items-center justify-center overflow-hidden rounded-[8px]" style={{ background: 'var(--surface-sunken)' }}>
          {o.picUrl || o.itemPicUrl || o.itemPic ? (
            <img src={o.picUrl || o.itemPicUrl || o.itemPic} alt="" className="h-full w-full object-cover" />
          ) : (
            <ShoppingCart className="h-4 w-4" style={{ color: 'var(--text-tertiary)' }} />
          )}
        </div>
        {o._avatar ? (
          <img
            src={o._avatar}
            alt=""
            className="absolute -bottom-1 -right-1 h-4 w-4 rounded-full object-cover"
            style={{ boxShadow: `0 0 0 1.5px ${color}, 0 0 0 3px var(--surface-card-strong)` }}
            title={maskedAccountNick}
          />
        ) : (
          <div
            className="absolute -bottom-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full text-[8px] font-bold text-white"
            style={{ background: color, boxShadow: '0 0 0 2px var(--surface-card-strong)' }}
            title={maskedAccountNick}
          >
            {accountAvatarInitial}
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col">
        <div data-testid="order-card-top" className="flex min-w-0 items-center gap-1.5">
          <SensitiveValue kind="item-title" className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-5" value={o.title || o.itemTitle || '—'} />
          {refundPending ? (
            <span className="inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-[2px] text-[10px] font-semibold" style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}>
              <RotateCcw className="h-3 w-3" />退款待处理
            </span>
          ) : (
            <span className="inline-flex max-w-[76px] shrink-0 items-center rounded px-1.5 py-[2px] text-[10px] font-semibold" style={{ background: meta.bg, color: meta.tone }} title={statusLabel}>
              <span className="truncate">{statusLabel}</span>
            </span>
          )}
          {overdue ? (
            <span className="inline-flex shrink-0 rounded px-1 py-[1px] text-[9px] font-semibold" style={{ background: 'var(--danger)', color: 'var(--liquid-highlight)' }}>
              超 {overdueHours}h
            </span>
          ) : null}
          <div className="max-w-[104px] shrink-0 truncate text-right font-mono text-[14px] font-semibold leading-5" style={{ color: 'var(--text-main)' }}>
            ¥<SensitiveValue kind="money" value={amount} />
          </div>
        </div>

        <div data-testid="order-card-meta" className="mt-1 flex min-w-0 items-center gap-1.5 text-[10.5px] leading-4" style={{ color: 'var(--text-muted)' }}>
          <span className="max-w-[104px] shrink-0 truncate rounded px-1.5 py-px" style={{ background: `${color}15`, color }} title={maskedAccountNick}>
            <SensitiveValue kind="account" value={o._nick || '账号'} />
          </span>
          <span className="truncate">买家：<SensitiveValue kind="buyer" value={o.buyerNick || o.buyerId || '—'} /></span>
        </div>

        {refundPending ? (
          <div data-testid="order-card-risk" className="mt-1.5 flex min-w-0 items-center justify-between gap-2 rounded-[6px] px-2 py-0.5 text-[10px] leading-4" style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}>
            <span className="inline-flex min-w-0 items-center gap-1 font-medium"><AlertTriangle className="h-3 w-3 shrink-0" /><span className="truncate">买家申请退款</span></span>
            <span className="shrink-0 font-semibold">需卖家处理</span>
          </div>
        ) : null}

        <div data-testid="order-card-footer" className="mt-auto flex min-w-0 items-center justify-between gap-2 border-t pt-2" style={{ borderColor: 'var(--control-border)' }}>
          <div className="min-w-0 truncate font-mono text-[9.5px] leading-4" style={{ color: 'var(--text-muted)' }}>
            <SensitiveValue kind="order-id" value={o.orderId || o.bizOrderId || '—'} />{fullTs ? ` · ${fullTs}` : ''}
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {isWaitSend(o) ? (
              <button onClick={() => onShip(o._uid, o)} className={iconButtonClass} style={{ background: 'var(--primary)', color: '#fff' }} title="发货" aria-label="发货">
                <Truck className="h-3.5 w-3.5" />
              </button>
            ) : null}
            {isWaitPay(o) ? (
              <button onClick={() => onAdjustPrice(o._uid, o)} className={iconButtonClass} style={{ background: 'var(--status-warning-bg)', color: 'var(--warning)' }} title="改价" aria-label="改价">
                <PencilLine className="h-3.5 w-3.5" />
              </button>
            ) : null}
            {isWaitPay(o) || isWaitSend(o) ? (
              <button onClick={() => onClose(o._uid, o)} className={iconButtonClass} title="关闭订单" aria-label="关闭订单">
                <XCircle className="h-3.5 w-3.5" style={{ color: 'var(--text-secondary)' }} />
              </button>
            ) : null}
            {isEnded(o) ? (
              <button onClick={() => onDelete(o._uid, o)} className={iconButtonClass} title="删除订单" aria-label="删除订单">
                <Trash2 className="h-3.5 w-3.5" style={{ color: 'var(--text-secondary)' }} />
              </button>
            ) : null}
            {isRedFlowerEligibleOrder(o) ? (
              <button
                onClick={() => onRequestRedFlower(o._uid, o)}
                disabled={requestingRedFlower}
                className={iconButtonClass}
                style={{ background: 'var(--status-success-bg)', color: 'var(--success)' }}
                title="求小红花"
                aria-label="求小红花"
              >
                <Flower2 className={`h-3.5 w-3.5 ${requestingRedFlower ? 'animate-pulse' : ''}`} />
              </button>
            ) : null}
            <button
              onClick={() => onSync(o._uid)}
              disabled={syncing}
              title={`同步 ${maskedAccountNick}`}
              aria-label={`同步 ${maskedAccountNick}`}
              className={iconButtonClass}
            >
              <RefreshCw className={`h-3.5 w-3.5 ${syncing ? 'animate-spin' : ''}`} style={{ color: 'var(--text-secondary)' }} />
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
