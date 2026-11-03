// @ts-nocheck
import { useState, useEffect, useRef } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import {
  MessageSquare, ShoppingCart, Package, AlertTriangle,
  Search, ListChecks, CircleDollarSign, ChevronRight, Clock3, Sparkles,
  Megaphone, BellRing, Database,
  TimerReset,
} from 'lucide-react'
import { useAccounts } from '../../hooks/useAccounts'
import { getDashboardStats } from '../../api/index'
import { getAuthToken } from '../../api/client'
import { useAuthScope } from '../../hooks/useAuthScope'
import { useUserUnreadStore, selectUserUnreadTotal } from '../../store/userUnreadStore'
import { filterBusinessAccounts, isDeleteOnlyAccount } from '../../utils/accountAccess'

const mutedSurfaceStyle = {
  border: '1px solid var(--glass-border)',
  background: 'var(--surface-soft)',
}

function SectionHeader({ eyebrow, title, description, action }) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="min-w-0">
        <h2
          className="text-[16px] leading-[1.2] font-semibold"
          style={{ color: 'var(--text-main)' }}
        >
          {title}
        </h2>
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  )
}

function WarmCard({ children, className = '', style = {} }) {
  return (
    <div className={`mobile-surface-card ${className}`} style={style}>
      {children}
    </div>
  )
}

function StatusChip({ children, tone = 'neutral' }) {
  const toneMap = {
    neutral: { background: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' },
    info: { background: 'var(--status-info-bg)', color: 'var(--status-info-text)' },
    success: { background: 'var(--status-success-bg)', color: 'var(--status-success-text)' },
    warning: { background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' },
    danger: { background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' },
  }
  const currentTone = toneMap[tone] || toneMap.neutral
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-3 py-1 text-[11px] font-semibold"
      style={currentTone}
    >
      {children}
    </span>
  )
}

function StatCard({ icon: Icon, label, value, color, helper, onClick }) {
  return (
    <button
      onClick={onClick}
      className="flex min-h-[122px] flex-col items-start gap-3 rounded-[24px] p-4 text-left transition-transform active:scale-[0.98]"
      style={mutedSurfaceStyle}
    >
      <div className="flex w-full items-start justify-between gap-3">
        <div className="flex h-11 w-11 items-center justify-center rounded-[18px]" style={{ backgroundColor: `color-mix(in srgb, ${color} 12%, transparent)` }}>
          <Icon className="w-5 h-5" style={{ color }} />
        </div>
        <ChevronRight className="mt-1 h-4 w-4 shrink-0" style={{ color: 'var(--text-tertiary)' }} />
      </div>
      <div className="space-y-1">
        <p className="text-[24px] font-semibold leading-none" style={{ color: 'var(--text-main)' }}>{value ?? '—'}</p>
        <p className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{label}</p>
        {helper ? (
          <p className="text-[11px] leading-4" style={{ color: 'var(--text-secondary)' }}>
            {helper}
          </p>
        ) : null}
      </div>
    </button>
  )
}

function formatMoney(value) {
  const n = Number(value)
  if (!Number.isFinite(n)) return '¥0'
  return `¥${n.toLocaleString('zh-CN', { maximumFractionDigits: 2 })}`
}

export default function MobileOverview() {
  const navigate = useNavigate()
  const { accounts, loadAccounts } = useAccounts()
  const [loading, setLoading] = useState(false)
  const [stats, setStats] = useState(null)
  const { authToken } = useAuthScope(() => {
    setStats(null)
    setLoading(false)
  })
  const unreadMessages = useUserUnreadStore(selectUserUnreadTotal)

  const activeAccounts = filterBusinessAccounts(accounts)
  const activeAccountKey = activeAccounts.map((a) => a.uid).join('|')
  const activeAccountKeyRef = useRef(activeAccountKey)
  activeAccountKeyRef.current = activeAccountKey
  const restrictedAccounts = accounts.filter((a) => !isDeleteOnlyAccount(a) && (a.status === 'restricted' || a.status === 'inactive'))
  const abnormalAccounts = (stats?.restrictedAccounts ?? 0) + (stats?.inactiveAccounts ?? 0) + (stats?.sessionErrorAccounts ?? 0) || restrictedAccounts.length
  const heroChips = [
    { label: '活跃账号', value: stats?.activeAccounts ?? activeAccounts.length, tone: 'success' },
    { label: '异常账号', value: abnormalAccounts, tone: abnormalAccounts ? 'danger' : 'info' },
  ]

  async function loadAll() {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKey
    setLoading(true)
    try {
      const [s] = await Promise.allSettled([getDashboardStats()])
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      if (s.status === 'fulfilled') setStats(s.value)
    } finally {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) setLoading(false)
    }
  }

  useEffect(() => {
    loadAccounts().then(() => loadAll())
  }, [authToken])

  return (
    <div className="h-full flex flex-col mobile-page-shell" style={{ backgroundColor: 'var(--bg-page)' }}>
      <div className="flex-1 overflow-y-auto mobile-page-body">
        <div className="mobile-page-stack pb-6">
          <WarmCard className="p-5">
            <SectionHeader
              title="运营总览"
              action={(
                <StatusChip tone={loading ? 'warning' : 'info'}>
                  <Clock3 className="h-3.5 w-3.5" />
                  {loading ? '同步中' : '概览'}
                </StatusChip>
              )}
            />

            <div className="mt-4 flex flex-wrap gap-2">
              {heroChips.map((chip) => (
                <StatusChip key={chip.label} tone={chip.tone}>
                  {chip.label} {chip.value}
                </StatusChip>
              ))}
            </div>

            <div className="mt-4 grid grid-cols-2 gap-3">
              <StatCard
                icon={CircleDollarSign}
                label="成交金额"
                value={formatMoney(stats?.todayRevenue ?? 0)}
                helper=""
                color="var(--primary)"
                onClick={() => navigate('/orders')}
              />
              <StatCard
                icon={ShoppingCart}
                label="今日订单"
                value={stats?.todayOrders ?? 0}
                helper=""
                color="var(--success)"
                onClick={() => navigate('/orders')}
              />
              <StatCard
                icon={MessageSquare}
                label="未读消息"
                value={unreadMessages}
                helper=""
                color="var(--warning)"
                onClick={() => navigate('/messages')}
              />
              <StatCard
                icon={AlertTriangle}
                label="账号异常"
                value={abnormalAccounts}
                helper=""
                color={abnormalAccounts ? 'var(--danger)' : 'var(--text-secondary)'}
                onClick={() => navigate('/accounts')}
              />
            </div>
          </WarmCard>

          <WarmCard className="p-5">
            <SectionHeader
              title="业务入口"
            />

            <div className="mt-3 grid grid-cols-2 gap-3">
              {[
                { title: '账号管理', to: '/accounts', icon: Database, tone: 'success' },
                { title: '账号导入导出', to: '/bundle', icon: Database, tone: 'success' },
                { title: '同行监控', to: '/peer-monitor', icon: Search, tone: 'primary' },
                { title: '超级擦亮', to: '/super-polish', icon: Sparkles, tone: 'primary' },
                { title: '推广', to: '/growth', icon: Megaphone, tone: 'warning' },
                { title: '通知中心', to: '/notifications', icon: BellRing, tone: 'warning' },
                { title: '商品回复', to: '/item-replies', icon: MessageSquare, tone: 'primary' },
                { title: '不在线回复', to: '/offline-reply', icon: Clock3, tone: 'warning' },
                { title: 'AI 回复', to: '/ai-replies', icon: Sparkles, tone: 'primary' },
                { title: '评论任务', to: '/tasks', icon: ListChecks, tone: 'primary' },
                { title: '关键词规则', to: '/keywords', icon: MessageSquare, tone: 'primary' },
                { title: '自动发货', to: '/delivery', icon: Package, tone: 'success' },
                { title: '定时任务', to: '/other-automation', icon: TimerReset, tone: 'primary' },
              ].map((item) => (
                <QuickLinkTile key={item.to} {...item} />
              ))}
            </div>
          </WarmCard>
        </div>
      </div>
    </div>
  )
}

function QuickLinkTile({ to, icon: Icon, title, tone = 'primary' }) {
  const colorMap = {
    primary: 'var(--primary)',
    success: 'var(--success)',
    warning: 'var(--warning)',
  }
  const color = colorMap[tone] || colorMap.primary
  return (
    <Link
      to={to}
      className="flex min-h-[78px] items-center gap-3 rounded-[18px] px-3.5 py-3 text-left transition-transform active:scale-[0.98]"
      style={{
        border: '1px solid var(--control-border)',
        background: 'var(--surface-control)',
      }}
    >
      <div
        className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[12px]"
        style={{
          background: `color-mix(in srgb, ${color} 13%, transparent)`,
          color,
        }}
      >
        <Icon className="h-[18px] w-[18px]" />
      </div>
      <div className="min-w-0 flex-1">
        <span className="block truncate text-[14px] font-semibold leading-5" style={{ color: 'var(--text-main)' }}>{title}</span>
      </div>
      <ChevronRight className="h-4 w-4 shrink-0" style={{ color: 'var(--text-tertiary)' }} />
    </Link>
  )
}
