import { useState } from 'react'
import { Link } from 'react-router-dom'
import {
  AlertTriangle,
  Bot,
  Clock3,
  MessageCircle,
  PackageCheck,
  RefreshCw,
  RotateCw,
  Send,
  ShieldAlert,
} from 'lucide-react'
import type { DataCenterAccountRow, DataCenterShareRange, DataCenterStats, DataCenterTrendPoint } from '../../api/types'
import { sanitizeServiceText } from '../../utils/errorText'
import './dashboard.css'

type RangeKey = 'week' | 'halfMonth' | 'month'
type RingSegment = { label: string; value: number; color: string }

export function DashboardPresentation({ stats, loading, loadError, scopeUid, trendReady, onScopeChange, onRefresh }: {
  stats: DataCenterStats | null
  loading: boolean
  loadError: string
  scopeUid: string
  trendReady: boolean
  onScopeChange: (uid: string) => void
  onRefresh: () => void
}) {
  const [range, setRange] = useState<RangeKey>('week')
  const effectiveScopeUid = scopeUid === 'all' || stats?.accounts.some((account) => account.uid === scopeUid) ? scopeUid : 'all'
  const selectedAccount = effectiveScopeUid === 'all' ? null : stats?.accounts.find((account) => account.uid === effectiveScopeUid) || null
  const sortedAccounts = stats ? [...stats.accounts].sort(accountRiskSort) : []

  return (
    <div className="dashboard-v7 desktop-page-viewport">
      <main className="dashboard-v7__inner">
        <header className="dashboard-v7-head">
          <div>
            <span className="dashboard-v7-eyebrow"><i />经营与自动化</span>
            <h1>经营总览</h1>
            <p>成交数据按账号和时间范围查看；所有需要处理的事项集中在同一个待办列表。</p>
            {loadError ? <p className="dashboard-v7-error" role="alert" data-testid="dashboard-load-error">刷新失败：{sanitizeServiceText(loadError, '请稍后重试')}</p> : null}
          </div>
          <div className="dashboard-v7-actions">
            <select aria-label="账号范围" value={effectiveScopeUid} onChange={(event) => onScopeChange(event.target.value)} disabled={!stats || loading}>
              <option value="all">{stats ? `全部账号（${stats.totals.accountTotal}）` : '全部账号'}</option>
              {sortedAccounts.map((account) => <option value={account.uid} key={account.uid}>{safeAccountName(account.nick)}</option>)}
            </select>
            <button type="button" className="dashboard-v7-refresh" onClick={onRefresh} disabled={loading}>
              <RefreshCw size={15} className={loading ? 'spin' : ''} />{loading ? '刷新中' : '刷新数据'}
            </button>
          </div>
        </header>

        {stats ? <LoadedDashboard stats={stats} range={range} setRange={setRange} selectedAccount={selectedAccount} trendReady={trendReady} /> : <UnavailableDashboard loading={loading} />}
      </main>
    </div>
  )
}

function LoadedDashboard({ stats, range, setRange, selectedAccount, trendReady }: {
  stats: DataCenterStats
  range: RangeKey
  setRange: (range: RangeKey) => void
  selectedAccount: DataCenterAccountRow | null
  trendReady: boolean
}) {
  const scopeName = selectedAccount ? safeAccountName(selectedAccount.nick) : '全部账号'
  const sellerTrend = stats.sellerTrend[range]
  const trendAvailable = trendReady && sellerTrend.available
  const summary = trendAvailable ? { amount: sellerTrend.totalAmount, orders: sellerTrend.totalOrders } : null
  const points = trendAvailable ? sellerTrend.points : null
  const trendUnavailable = trendAvailable ? null : dataCompassTrendUnavailable(stats)

  return <>
    <section className="dashboard-v7-panel dashboard-v7-overview" aria-label="成交汇总">
      <div className="dashboard-v7-panel-head">
        <span>{scopeName} · {rangeLabel(range)}成交</span>
        <div className="dashboard-v7-tabs" aria-label="成交范围">
          {([['week', '近 7 天'], ['halfMonth', '近 15 天'], ['month', '近 30 天']] as const).map(([key, label]) => (
            <button key={key} type="button" aria-pressed={range === key} onClick={() => setRange(key)}>{label}</button>
          ))}
        </div>
      </div>
      <div className="dashboard-v7-overview-body">
        <div className="dashboard-v7-summary">
          <span>成交金额</span>
          <strong className={summary ? '' : 'is-unavailable'}>{summary ? (summary.amount === 0 ? '暂无成交' : money(summary.amount)) : trendUnavailable?.title}</strong>
          <span className="dashboard-v7-period-change">{trendUnavailable?.detail || '较上一周期：数据暂不可用'}</span>
          <div className="dashboard-v7-metrics">
            <SummaryMetric label="成交订单" value={summary?.orders} />
            <SummaryMetric label="待发货" value={selectedAccount ? selectedAccount.waitSendOrders : stats.totals.waitSendOrders} />
            <SummaryMetric label="售后退款" value={selectedAccount ? null : stats.totals.aftersaleOrders} />
            <SummaryMetric label="未读会话" value={selectedAccount ? selectedAccount.unreadSessions : stats.totals.unreadSessions} />
          </div>
        </div>
        <div className="dashboard-v7-rings">
          <AccountShareRing stats={stats} range={range} selectedAccount={selectedAccount} />
          <RingCard title="订单处理" description="待发货和售后退款占比。" segments={[]} state="unavailable" testId="ring-order-handling" />
          <RingCard title="自动化覆盖" description="已有自动化配置的账号比例。" segments={[]} state="unavailable" testId="ring-automation-coverage" />
        </div>
      </div>
      <AmountTrend points={points} range={range} unavailable={trendUnavailable} />
    </section>
    <TodoAndRisks stats={stats} />
    <AutomationRuntime automation={stats.automation} />
    <footer className="dashboard-v7-foot">
      <span><i />{stats.generatedAt ? `本地业务数据已更新于 ${relativeTime(stats.generatedAt)}` : '等待首次同步'}</span>
      <span>数值只接入已有真实字段</span>
    </footer>
  </>
}

function UnavailableDashboard({ loading }: { loading: boolean }) {
  return <section className="dashboard-v7-panel dashboard-v7-overview dashboard-v7-initial" aria-label="成交汇总">
    <div className="dashboard-v7-panel-head"><span>全部账号 · 近 7 天成交</span></div>
    <div data-testid="dashboard-initial-state">{loading ? '正在加载经营数据…' : '经营数据暂不可用'}</div>
  </section>
}

function SummaryMetric({ label, value }: { label: string; value: number | null | undefined }) {
  return <div><span>{label}</span><b>{typeof value === 'number' ? value.toLocaleString('zh-CN') : '暂不可用'}</b></div>
}

function AccountShareRing({ stats, range, selectedAccount }: { stats: DataCenterStats; range: RangeKey; selectedAccount: DataCenterAccountRow | null }) {
  if (range !== 'week') return <RingCard title={selectedAccount ? '该账号成交占比' : '账号成交占比'} description={selectedAccount ? '所选账号在全部账号成交额中的比例。' : '按成交额统计各账号在总成交中的比例。'} segments={[]} state="unavailable" testId="ring-account-share" />
  const share = stats.share.week
  if (share.totalAmount === 0) return <RingCard title={selectedAccount ? '该账号成交占比' : '账号成交占比'} description="当前范围暂无成交。" segments={[]} state="empty" testId="ring-account-share" />
  const segments = selectedAccount ? selectedShareSegments(share, selectedAccount) : accountShareSegments(share)
  return <RingCard title={selectedAccount ? '该账号成交占比' : '账号成交占比'} description={selectedAccount ? '所选账号在全部账号成交额中的比例。' : '按成交额统计各账号在总成交中的比例。'} segments={segments} state={segments.length ? 'ready' : 'unavailable'} testId="ring-account-share" />
}

function RingCard({ title, description, segments, state, testId }: { title: string; description: string; segments: RingSegment[]; state: 'ready' | 'empty' | 'unavailable'; testId: string }) {
  const total = segments.reduce((sum, item) => sum + item.value, 0)
  const background = total > 0 ? conicGradient(segments, total) : 'conic-gradient(#e8edf3 0 100%)'
  return <div className={`dashboard-v7-ring-card is-${state}`} data-testid={testId}>
    <i className="dashboard-v7-donut" style={{ background }} />
    <div><b>{title}</b><p>{description}</p>{state === 'unavailable' ? <span className="dashboard-v7-unavailable">数据暂不可用</span> : state === 'empty' ? <span className="dashboard-v7-unavailable">暂无成交</span> : <div className="dashboard-v7-ring-key">{segments.slice(0, 4).map((item) => <span key={item.label}><i style={{ background: item.color }} />{item.label}</span>)}</div>}</div>
  </div>
}

function AmountTrend({ points, range, unavailable }: { points: DataCenterTrendPoint[] | null; range: RangeKey; unavailable: TrendUnavailable | null }) {
  if (points === null) return <div className="dashboard-v7-chart dashboard-v7-chart-empty" data-testid="dashboard-trend"><span>{unavailable?.title}</span>{unavailable?.detail ? <span className="dashboard-v7-unavailable">{unavailable.detail}</span> : null}</div>
  if (!points.length || points.every((point) => point.amount === 0)) return <div className="dashboard-v7-chart dashboard-v7-chart-empty" data-testid="dashboard-trend">当前范围暂无成交</div>
  const max = Math.max(...points.map((point) => point.amount), 1)
  const labelIndexes = new Set(trendLabelPoints(points, range === 'month' ? 6 : 7).map(({ index }) => index))
  const firstLabel = Math.min(...labelIndexes)
  const lastLabel = Math.max(...labelIndexes)
  return <div className="dashboard-v7-chart" data-testid="dashboard-trend" role="img" aria-label={`${rangeLabel(range)}成交金额趋势图`}>
    {points.map((point, index) => <i key={`${point.date}-${index}`} className={`dashboard-v7-bar${index === firstLabel ? ' is-first-label' : ''}${index === lastLabel ? ' is-last-label' : ''}`} data-day={labelIndexes.has(index) ? (point.label || shortDate(point.date)) : ''} style={{ height: `${Math.max(4, point.amount / max * 100)}%` }}><span>{`${point.label || point.date}: ${money(point.amount)}`}</span></i>)}
  </div>
}

type TrendUnavailable = { title: string; detail: string }

function dataCompassTrendUnavailable(stats: DataCenterStats): TrendUnavailable {
  const dataCompass = stats.sync.dataCompass
  if (dataCompass.status === 'failed') {
    const details = []
    if (dataCompass.failedCount > 0) details.push(`失败账号 ${dataCompass.failedCount} 个`)
    const errorSummary = sanitizeServiceText(dataCompass.errorSummary, '')
    if (errorSummary) details.push(errorSummary)
    return { title: '数据罗盘同步失败', detail: details.join(' · ') }
  }
  if (dataCompass.status === 'pending') return { title: '等待数据罗盘同步', detail: '' }
  return { title: '成交趋势暂不可用', detail: '' }
}

function TodoAndRisks({ stats }: { stats: DataCenterStats }) {
  const statusCounts = connectionStatusCounts(stats.accounts)
  const connectionAttention = statusCounts.issue + statusCounts.unsynced + stats.sync.staleAccounts
  const accountErrors = stats.accounts.filter((account) => hasAccountRisk(account)).length
  const riskCount = Math.max(stats.totals.sessionErrorAccounts, accountErrors)
  const scheduled = stats.automation?.scheduledTasks
  const automationKnown = [scheduled?.pending, scheduled?.errors].some((value) => typeof value === 'number')
  const automationErrors = scheduled?.errors
  const automationTone = typeof automationErrors === 'number' && automationErrors > 0 ? 'risk' : automationKnown ? 'ok' : 'info'
  const items = [
    { key: 'wait-send', to: '/orders', icon: PackageCheck, tone: 'warn', title: '待发货订单', description: stats.totals.waitSendOrders ? `${stats.totals.waitSendOrders} 笔订单等待发货${stats.totals.waitSendEarliestAt ? `，最早更新于 ${queueTime(stats.totals.waitSendEarliestAt)}` : ''}。` : '当前没有待发货订单。', badge: String(stats.totals.waitSendOrders) },
    { key: 'aftersale', to: '/orders', icon: AlertTriangle, tone: 'risk', title: '售后退款待处理', description: stats.totals.aftersaleOrders ? `${stats.totals.aftersaleOrders} 笔买家申请退款，需要卖家处理。` : '当前没有售后退款待处理。', badge: String(stats.totals.aftersaleOrders) },
    { key: 'messages', to: '/messages', icon: MessageCircle, tone: 'info', title: '未读与超时回复', description: `${stats.totals.unreadMessages} 条未读消息，其中 ${stats.totals.overdueReplySessions} 个会话需优先回复。`, badge: String(stats.totals.overdueReplySessions || stats.totals.unreadMessages) },
    { key: 'connections', to: '/accounts', icon: RotateCw, tone: connectionAttention ? 'warn' : 'ok', title: '账号连接与同步', description: connectionAttention ? `${statusCounts.issue} 个账号离线或重连，${statusCounts.unsynced} 个账号连接状态未同步，${stats.sync.staleAccounts} 个账号业务数据可能滞后。` : '全部账号连接正常，未发现同步滞后。', badge: connectionAttention ? String(connectionAttention) : '正常' },
    { key: 'risk', to: '/accounts', icon: ShieldAlert, tone: riskCount ? 'risk' : 'ok', title: '账号风险', description: riskCount ? `${riskCount} 个账号存在需要查看的风险记录。` : '未发现账号错误或会话风险。', badge: String(riskCount) },
    { key: 'automation', to: '/other-automation', icon: Clock3, tone: automationTone, title: '自动化运行', description: automationKnown ? `${formatOptional(scheduled?.pending, '已安排')}，${formatOptional(scheduled?.errors, '最近失败')}。` : '自动化运行数据暂不可用。', badge: automationKnown ? (automationErrors ? `${automationErrors} 失败` : '正常') : '暂不可用' },
  ] as const
  return <section className="dashboard-v7-panel dashboard-v7-todo" aria-label="待办与异常">
    <div className="dashboard-v7-section-head"><div><h2>待办与异常</h2><p>订单、消息、账号、自动化的所有需处理事项都在这里。</p></div></div>
    <div className="dashboard-v7-reminders">{items.map(({ key, to, icon: Icon, tone, title, description, badge }) => <Link key={key} to={to} className="dashboard-v7-reminder" data-testid={`todo-${key}`}><i className={`dashboard-v7-reminder-icon is-${tone}`}><Icon size={15} /></i><div><b>{title}</b><p>{description}</p></div><span className={`dashboard-v7-pill is-${tone}`}>{badge}</span></Link>)}</div>
  </section>
}

function AutomationRuntime({ automation }: { automation: DataCenterStats['automation'] }) {
  const cards = automation ? [
    { key: 'delivery', to: '/delivery', label: '自动发货', icon: Send, metrics: [['启用账号', automation.delivery.enabled], ['启用规则', automation.delivery.pending]], status: automationStatus(automation.delivery.enabled, undefined, '配置已启用') },
    { key: 'replies', to: '/keywords', label: '回复自动化', icon: Bot, metrics: [['启用账号', automation.replies.enabled]], status: automationStatus(automation.replies.enabled, undefined, '回复配置已启用') },
    { key: 'scheduled-tasks', to: '/other-automation', label: '定时任务', icon: Clock3, metrics: [['已安排', automation.scheduledTasks.pending], ['最近失败', automation.scheduledTasks.errors]], status: automationStatus(automation.scheduledTasks.pending, automation.scheduledTasks.errors, '按计划运行') },
  ] : []
  return <section id="automation" className="dashboard-v7-panel dashboard-v7-automation" aria-label="自动化配置与运行">
    <div className="dashboard-v7-section-head"><div><h2>自动化配置与运行</h2><p>只展示可核对的真实配置和调度状态；异常同时进入上方“待办与异常”。</p></div><Link to="/keywords">管理自动化 →</Link></div>
    {cards.length ? <div className="dashboard-v7-auto-grid">{cards.map(({ key, to, label, icon: Icon, metrics, status }) => {
      const available = metrics.filter(([, value]) => typeof value === 'number')
      return <Link to={to} key={key} className="dashboard-v7-auto-card" data-testid={`automation-${key}`}><div><b>{label}</b><i><Icon size={15} /></i></div>{available.length ? <dl>{available.map(([metricLabel, value]) => <div key={String(metricLabel)} className={metricLabel === '最近失败' && value ? 'is-error' : ''}><dd>{value}</dd><dt>{metricLabel}</dt></div>)}</dl> : <p className="dashboard-v7-unavailable">数据暂不可用</p>}<span className={status.tone === 'risk' ? 'is-error' : status.tone === 'ok' ? 'is-ok' : ''}>{status.label}</span></Link>
    })}</div> : <div className="dashboard-v7-empty">当前客户端暂未返回自动化运行汇总</div>}
  </section>
}

function automationStatus(primary: number | null | undefined, errors: number | null | undefined, success: string) {
  if (typeof primary !== 'number' && typeof errors !== 'number') return { tone: 'unknown', label: '运行状态暂不可用' } as const
  if (typeof errors === 'number' && errors > 0) return { tone: 'risk', label: '存在最近失败任务' } as const
  if (typeof primary === 'number' && primary === 0) return { tone: 'neutral', label: '当前未启用' } as const
  return { tone: 'ok', label: success } as const
}

function connectionStatusCounts(accounts: DataCenterAccountRow[]) {
  return accounts.reduce((counts, account) => {
    const status = normalizeLiveStatus(account.liveStatus)
    if (!status) counts.unsynced += 1
    else if (!isHealthyLiveStatus(status)) counts.issue += 1
    return counts
  }, { issue: 0, unsynced: 0 })
}

function hasAccountRisk(account: DataCenterAccountRow) {
  const status = normalizeLiveStatus(account.liveStatus)
  return Boolean(account.lastError && status && !isHealthyLiveStatus(status))
}

function accountRiskSort(a: DataCenterAccountRow, b: DataCenterAccountRow) {
  const errorDiff = Number(Boolean(b.lastError)) - Number(Boolean(a.lastError))
  if (errorDiff) return errorDiff
  const connectionDiff = Number(hasConnectionIssue(b)) - Number(hasConnectionIssue(a))
  if (connectionDiff) return connectionDiff
  const pendingDiff = accountPendingLoad(b) - accountPendingLoad(a)
  return pendingDiff || safeAccountName(a.nick).localeCompare(safeAccountName(b.nick), 'zh-CN')
}

function hasConnectionIssue(account: DataCenterAccountRow) {
  const status = normalizeLiveStatus(account.liveStatus)
  return Boolean(status && !isHealthyLiveStatus(status))
}

function accountPendingLoad(account: DataCenterAccountRow) {
  return account.waitSendOrders + account.pendingPayOrders + account.unreadMessages + account.unreadSessions
}

function normalizeLiveStatus(value: string | undefined) { return String(value || '').trim().toLowerCase() }
function isHealthyLiveStatus(value: string) { return value === 'connected' || value === 'ok' || value === 'online' }

function accountShareSegments(share: DataCenterShareRange): RingSegment[] {
  const valid = share.segments.filter((segment) => segment.amount > 0)
  const total = valid.reduce((sum, segment) => sum + segment.amount, 0)
  if (!total) return []
  const colors = ['#1769e8', '#4b9ddf', '#9bc0ff', '#d7e5ff']
  const head = valid.slice(0, 3).map((segment, index) => ({ label: `${safeAccountName(segment.nick)} ${formatPercent(segment.amount / total * 100)}`, value: segment.amount, color: colors[index] }))
  const other = valid.slice(3).reduce((sum, segment) => sum + segment.amount, 0)
  return other > 0 ? [...head, { label: `其他 ${formatPercent(other / total * 100)}`, value: other, color: colors[3] }] : head
}

function selectedShareSegments(share: DataCenterShareRange, account: DataCenterAccountRow): RingSegment[] {
  const selected = share.segments.find((segment) => segment.uid === account.uid)?.amount
  if (typeof selected !== 'number' || selected < 0 || share.totalAmount <= 0) return []
  const remainder = Math.max(0, share.totalAmount - selected)
  return [{ label: `${safeAccountName(account.nick)} ${formatPercent(selected / share.totalAmount * 100)}`, value: selected, color: '#1769e8' }, { label: `其他 ${formatPercent(remainder / share.totalAmount * 100)}`, value: remainder, color: '#d7e5ff' }].filter((item) => item.value > 0)
}

function conicGradient(segments: RingSegment[], total: number) { let cursor = 0; return `conic-gradient(${segments.map((segment) => { const start = cursor; cursor += segment.value / total * 100; return `${segment.color} ${start}% ${cursor}%` }).join(',')})` }
function trendLabelPoints(points: DataCenterTrendPoint[], maxLabels = 7) { if (points.length <= maxLabels) return points.map((point, index) => ({ point, index })); return Array.from({ length: maxLabels }, (_, slot) => { const index = Math.round(slot * (points.length - 1) / (maxLabels - 1)); return { point: points[index], index } }) }
function formatOptional(value: number | null | undefined, label: string) { return typeof value === 'number' ? `${value} 个${label}` : `${label}数据暂不可用` }
function formatPercent(value: number) { return `${Number(value || 0).toLocaleString('zh-CN', { maximumFractionDigits: 1 })}%` }
function safeAccountName(value: string) { return String(value || '').trim() || '未识别账号' }
function rangeLabel(range: RangeKey) { return range === 'week' ? '近 7 天' : range === 'halfMonth' ? '近 15 天' : '近 30 天' }
function money(value: number) { return `¥${Number(value || 0).toLocaleString('zh-CN', { maximumFractionDigits: 2 })}` }
function shortDate(value: string) { const p = value.split('-'); return p.length === 3 ? `${Number(p[1])}/${Number(p[2])}` : value }
function clock(seconds: number) { const d = new Date(seconds * 1000); return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}` }
function localShortDate(date: Date) { return `${date.getMonth() + 1}/${date.getDate()}` }
function relativeTime(seconds: number) { const diff = Date.now() - seconds * 1000; if (diff < 60_000) return '刚刚'; if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`; if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`; return localShortDate(new Date(seconds * 1000)) }
function queueTime(seconds: number) { if (!seconds) return '--'; return `${localShortDate(new Date(seconds * 1000))} ${clock(seconds)}` }
