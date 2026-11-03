// @ts-nocheck
import { StatusBadge } from '../../components/StatusBadge'
import {
  isClosedOrder as isClosed,
  isFinishedOrder as isFinished,
  isWaitPayOrder as isWaitPay,
  isWaitReceiveOrder as isWaitReceive,
  isWaitSendOrder as isWaitSend,
  orderStatusDisplayLabel,
} from './orderStatus'

export const ACCOUNT_COLORS = ['#FF5000', '#0EBE7C', '#A24DFF', '#0095FF', '#FFC900', '#FF2E63', '#5E5CE6', '#14B8A6']

export const KANBAN_META = {
  pending: { key: 'pending', label: '待付款', tone: 'var(--warning)', bg: 'var(--status-warning-bg)' },
  tosend: { key: 'tosend', label: '待发货', tone: 'var(--warning)', bg: 'var(--status-warning-bg)' },
  sent: { key: 'sent', label: '待收货', tone: 'var(--primary)', bg: 'var(--primary-light)' },
  aftersale: { key: 'aftersale', label: '售后', tone: 'var(--danger)', bg: 'var(--status-danger-bg)' },
  done: { key: 'done', label: '已完成', tone: 'var(--success)', bg: 'var(--status-success-bg)' },
  closed: { key: 'closed', label: '交易关闭', tone: '#8E8E93', bg: 'var(--surface-soft)' },
  unknown: { key: 'unknown', label: '未知', tone: '#8E8E93', bg: 'var(--surface-soft)' },
}

export const expressList = [
  { code: 'SF', name: '顺丰速运' },
  { code: 'YTO', name: '圆通快递' },
  { code: 'ZTO', name: '中通快递' },
  { code: 'STO', name: '申通快递' },
  { code: 'YUNDA', name: '韵达快递' },
  { code: 'JD', name: '京东物流' },
  { code: 'EMS', name: 'EMS' },
  { code: 'JT', name: '极兔快递' },
]

export const lineClamp2 = {
  display: '-webkit-box',
  WebkitBoxOrient: 'vertical',
  WebkitLineClamp: 2,
  overflow: 'hidden',
}

export function extractOrders(r) {
  if (Array.isArray(r?.orders)) return r.orders
  if (Array.isArray(r?.data?.orderList)) return r.data.orderList
  if (Array.isArray(r?.data)) return r.data
  return []
}

export function statusBadge(o) {
  if (isWaitPay(o)) return <StatusBadge status="warning" label="待付款" />
  if (isWaitSend(o)) return <StatusBadge status="info" label="待发货" />
  if (isWaitReceive(o)) return <StatusBadge status="info" label="待收货" />
  if (isClosed(o)) return <StatusBadge status="neutral" label="交易关闭" />
  if (isFinished(o)) return <StatusBadge status="success" label="已完成" />
  return <StatusBadge status="neutral" label={orderStatusDisplayLabel(o, '未知')} />
}

export function colorForAccount(uid) {
  if (!uid) return ACCOUNT_COLORS[0]
  let seed = 0
  for (const ch of String(uid)) seed = (seed * 31 + ch.charCodeAt(0)) % ACCOUNT_COLORS.length
  return ACCOUNT_COLORS[seed]
}

export function parseMaybeTime(raw) {
  if (!raw && raw !== 0) return 0
  if (raw instanceof Date) return raw.getTime()
  if (typeof raw === 'number') return raw > 1e12 ? raw : raw * 1000
  const text = String(raw).trim()
  if (!text) return 0
  if (/^\d+$/.test(text)) {
    const num = Number(text)
    return num > 1e12 ? num : num * 1000
  }
  const parsed = Date.parse(text.replace(/-/g, '/'))
  return Number.isFinite(parsed) ? parsed : 0
}

export function getOrderTimeMs(order) {
  return parseMaybeTime(
    order.createTime
    ?? order.orderCreated
    ?? order.gmtCreate
    ?? order.createdAt
    ?? order.created_at
    ?? order.updateTime
    ?? 0,
  )
}

export function getRelativeTimeLabel(order) {
  const ts = getOrderTimeMs(order)
  if (!ts) return '时间未知'
  const diff = Math.max(0, Date.now() - ts)
  const hour = 3600_000
  const day = 24 * hour
  if (diff < 60_000) return '刚刚'
  if (diff < hour) return `${Math.max(1, Math.floor(diff / 60_000))}分钟前`
  if (diff < day) return `${Math.floor(diff / hour)}小时前`
  if (diff < 7 * day) return `${Math.floor(diff / day)}天前`
  const date = new Date(ts)
  return `${date.getMonth() + 1}/${date.getDate()}`
}

export function getOverdueHours(order) {
  if (!isWaitSend(order)) return 0
  const createdAt = getOrderTimeMs(order)
  if (!createdAt) return 0
  const hours = Math.floor((Date.now() - createdAt) / 3600_000)
  return hours > 24 ? hours : 0
}

export function getOrderTitle(order) {
  return order.title || order.itemTitle || order.item_title || '未命名商品'
}

export function getOrderBuyer(order) {
  return order.buyerNick || order.buyer_nick || order.buyerId || order.buyer_uid || '买家未知'
}

export function getOrderPrice(order) {
  return parseFloat(order.price || order.actualFee || order.totalFee || 0) || 0
}

export function getOrderImage(order) {
  return order.itemPic || order.picUrl || order.itemImage || order.item_image || order.itemCover || order.item_cover || ''
}

export function formatMoney(amount) {
  if (!Number.isFinite(amount)) return '¥0'
  return `¥${amount.toFixed(amount % 1 === 0 ? 0 : 2)}`
}

export function formatSyncedAt(ts) {
  if (!ts || ts <= 0) return '从未同步'
  const diffSec = Math.max(0, Math.floor(Date.now() / 1000 - ts))
  if (diffSec < 60) return '刚刚同步'
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)} 分钟前同步`
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} 小时前同步`
  const d = new Date(ts * 1000)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')} 同步`
}

export function formatAddressCopyText(address) {
  if (!address) return ''
  const fullName = address.fullName || address.name || ''
  const mobile = address.mobile || address.phone || ''
  const fullAddress = address.formatFullAddress || address.fullAddress || address.address || address.addressDetail || ''
  return address.copyText || [fullName, mobile, fullAddress].filter(Boolean).join(' ')
}
