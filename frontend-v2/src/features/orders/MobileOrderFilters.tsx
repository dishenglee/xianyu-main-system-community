// @ts-nocheck
import { Filter, Search, Users, X } from 'lucide-react'
import { BottomSheet } from '../../components/Modal'
import {
  KANBAN_META,
  colorForAccount,
  formatSyncedAt,
} from './orderUtils'
import { SensitiveValue } from '../../components/SensitiveValue'

// 顶部 sticky 工具条 —— 搜索 + 状态 + 账号筛选入口
export function MobileOrdersToolbar({
  searchQuery,
  setSearchQuery,
  statusFilter,
  setStatusFilter,
  acctFilter,
  totalOrders,
  overdueCount,
  pendingCount,
  tosendCount,
  sentCount,
  aftersaleCount,
  doneCount,
  closedCount,
  onOpenAccountDrawer,
  accountStats,
  surfaceBackground,
}) {
  const acct = acctFilter ? accountStats.find((a) => a.uid === acctFilter) : null

  return (
    <div
      className="sticky top-0 z-10 px-4 py-2 space-y-2"
      style={{
        background: surfaceBackground || 'var(--mobile-glass-surface-strong, var(--mobile-glass-surface))',
        borderBottom: '1px solid var(--glass-border)',
        backdropFilter: 'saturate(180%) blur(20px)',
        WebkitBackdropFilter: 'saturate(180%) blur(20px)',
      }}
    >
      {/* 搜索 + 账号筛选触发 */}
      <div className="flex items-center gap-2">
        <div
          className="flex-1 flex items-center gap-1.5 rounded-[10px] px-3 py-[7px]"
          style={{ border: '1px solid var(--glass-border)', background: 'var(--surface-card)' }}
        >
          <Search className="w-4 h-4" style={{ color: 'var(--text-muted)' }} />
          <input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="搜索商品 / 订单号 / 买家"
            className="flex-1 bg-transparent outline-none text-[13px]"
          />
          {searchQuery ? (
            <button
              type="button"
              aria-label="清除搜索"
              onClick={() => setSearchQuery('')}
              className="w-5 h-5 rounded-full flex items-center justify-center"
              style={{ color: 'var(--text-muted)' }}
            >
              <X className="w-3.5 h-3.5" />
            </button>
          ) : null}
        </div>

        <button
          type="button"
          onClick={onOpenAccountDrawer}
          className="h-9 px-3 rounded-[10px] flex items-center gap-1 cursor-pointer"
          style={{
            border: '1px solid var(--glass-border)',
            background: acct ? `color-mix(in srgb, ${acct.color} 10%, var(--surface-card))` : 'var(--surface-card)',
            color: acct ? acct.color : 'var(--text-main)',
          }}
          aria-label="选择账号筛选"
        >
          <Users className="w-3.5 h-3.5" />
          <span className="text-[12.5px] font-medium truncate max-w-[80px]">
            {acct ? <SensitiveValue kind="account" value={acct.nick} /> : '全部账号'}
          </span>
        </button>
      </div>

      {/* 水平滚动状态 chip 行 */}
      <div className="flex items-center gap-1.5 overflow-x-auto -mx-4 px-4 pb-1" style={{ scrollbarWidth: 'none' }}>
        <StatusChip label="全部" value="all" count={totalOrders} active={statusFilter === 'all'} onChange={setStatusFilter} />
        <StatusChip label="待付款" value="pending" count={pendingCount} active={statusFilter === 'pending'} onChange={setStatusFilter} tone={KANBAN_META.pending.tone} />
        <StatusChip label="待发货" value="tosend" count={tosendCount} active={statusFilter === 'tosend'} onChange={setStatusFilter} tone={KANBAN_META.tosend.tone} alert={overdueCount > 0} />
        <StatusChip label="待收货" value="sent" count={sentCount} active={statusFilter === 'sent'} onChange={setStatusFilter} tone={KANBAN_META.sent.tone} />
        <StatusChip label="售后" value="aftersale" count={aftersaleCount} active={statusFilter === 'aftersale'} onChange={setStatusFilter} tone={KANBAN_META.aftersale.tone} />
        <StatusChip label="已完成" value="done" count={doneCount} active={statusFilter === 'done'} onChange={setStatusFilter} tone={KANBAN_META.done.tone} />
        <StatusChip label="关闭" value="closed" count={closedCount} active={statusFilter === 'closed'} onChange={setStatusFilter} tone={KANBAN_META.closed.tone} />
      </div>

      {overdueCount > 0 && statusFilter !== 'tosend' ? (
        <button
          type="button"
          onClick={() => setStatusFilter('tosend')}
          className="w-full flex items-center justify-center gap-1.5 px-3 py-1.5 rounded-[10px] text-[12px] font-semibold"
          style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
        >
          <Filter className="w-3.5 h-3.5" />
          ⚠ {overdueCount} 单超 24 小时未发货，点击查看
        </button>
      ) : null}
    </div>
  )
}

function StatusChip({ label, value, count, active, onChange, tone, alert }) {
  return (
    <button
      onClick={() => onChange(value)}
      className="shrink-0 h-8 px-3 rounded-full text-[12.5px] font-medium flex items-center gap-1.5 cursor-pointer transition-colors"
      style={{
        background: active ? 'var(--text-main)' : 'var(--surface-card)',
        color: active ? 'var(--liquid-highlight)' : 'var(--text-main)',
        border: `1px solid ${active ? 'var(--text-main)' : 'var(--glass-border)'}`,
      }}
    >
      {tone ? <span className="w-1.5 h-1.5 rounded-full" style={{ background: active ? 'var(--liquid-highlight)' : tone }} /> : null}
      <span>{label}</span>
      <span
        className="font-mono text-[11px] px-1.5 rounded-full"
        style={{
          background: active ? 'rgba(255,255,255,0.18)' : 'var(--surface-sunken)',
          color: active ? 'var(--liquid-highlight)' : 'var(--text-muted)',
        }}
      >
        {count}
      </span>
      {alert && !active ? <span className="w-1.5 h-1.5 rounded-full" style={{ background: 'var(--danger)' }} /> : null}
    </button>
  )
}

// 账号筛选 Drawer —— 从底部弹出的 BottomSheet
export function MobileAccountDrawer({
  open,
  onClose,
  acctFilter,
  setAcctFilter,
  accountStats,
  totalOrders,
  totalWaitSend,
  totalOverdue,
}) {
  return (
    <BottomSheet open={open} onClose={onClose} title="按账号筛选">
      <div className="space-y-2">
        <AcctRow
          active={acctFilter == null}
          color="var(--text-main)"
          name="全部账号"
          subtitle={`${totalOrders} 订单 · 待发 ${totalWaitSend}${totalOverdue ? ` · ⚠ ${totalOverdue} 超 24h` : ''}`}
          onClick={() => { setAcctFilter(null); onClose() }}
        />
        {accountStats.map((account) => (
          <AcctRow
            key={account.uid}
            active={acctFilter === account.uid}
            color={account.color}
            name={<SensitiveValue kind="account" value={account.nick} />}
            avatar={account.avatar}
            subtitle={`${account.total} 单 · 待发 ${account.waitSendCount}${account.overdueCount ? ` · ⚠${account.overdueCount}` : ''} · ${formatSyncedAt(account.lastSyncedAt)}`}
            onClick={() => { setAcctFilter(account.uid); onClose() }}
          />
        ))}
      </div>
    </BottomSheet>
  )
}

function AcctRow({ active, color, name, avatar, subtitle, onClick }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="w-full flex items-center gap-3 px-3 py-2.5 rounded-[12px] cursor-pointer text-left"
      style={{
        background: active ? `color-mix(in srgb, ${color} 10%, var(--surface-card))` : 'var(--surface-card)',
        border: `1px solid ${active ? color : 'var(--glass-border)'}`,
      }}
    >
      <div className="w-9 h-9 rounded-full flex items-center justify-center shrink-0" style={{ backgroundColor: 'var(--surface-card-strong)' }}>
        {avatar ? (
          <img src={avatar} alt="" className="w-8 h-8 rounded-full object-cover" style={{ boxShadow: `0 0 0 2px ${color}, 0 0 0 4px var(--surface-card-strong)` }} />
        ) : (
          <div className="w-8 h-8 rounded-full" style={{ background: `linear-gradient(135deg, ${color}, ${color}BB)`, boxShadow: `0 0 0 2px ${color}, 0 0 0 4px var(--surface-card-strong)` }} />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <div className="text-[14px] font-semibold truncate" style={{ color: 'var(--text-main)' }}>{name}</div>
        <div className="text-[12px] truncate" style={{ color: 'var(--text-muted)' }}>{subtitle}</div>
      </div>
    </button>
  )
}
