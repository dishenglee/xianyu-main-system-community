// @ts-nocheck
// 桌面端账号管理：保留原 AccountsPage 的横行密集排版 —
// 头像 + 状态点 / 名字 + UID + chip / 核心指标 / 自动化状态 / 操作按钮 / 右侧详情抽屉。
// 所有业务逻辑都来自 useAccountsPageState，本组件只渲染。
import {
  Users, Plus, RefreshCw, Power, ChevronDown, ChevronUp, Pencil, Database, Check, Circle, MapPin,
} from 'lucide-react'
import { useCallback, useRef, useState } from 'react'
import {
  getSellerMetric,
  SELLER_METRIC_ALIASES,
  accountProfileFallback,
} from '../../hooks/useAccountProfiles'
import { EmptyState } from '../../components/EmptyState'
import { SensitiveValue, SensitiveVisibilityToggle, useSensitiveAvatarInitial } from '../../components/SensitiveValue'
import { AccountDetailDrawer } from './AccountDetailDrawer'
import { AccountProfileEditor } from './AccountProfileEditor'
import { AccountQrLoginModal } from './AccountQrLoginModal'
import { FishShopBadge, hasFishShop } from './FishShopBadge'
import { PublishLocationModal } from './PublishLocationModal'
import { composeAccountLocation } from '../items/itemUtils'
import {
  useAccountsPageState,
  accountRuntimeStatus,
  accountRuntimeReason,
  formatCurrencyMetric,
  formatDateTime,
  formatLiveConnectionLabel,
  formatRefreshedAt,
} from './hooks/useAccountsPageState'
import { AccountOnlineAge } from './AccountOnlineAge'
import { getAccountActions, getAccountAutoReplyStatus, getAccountAutoShipStatus, getAccountRestrictionState } from './accountUi'

function AvatarFallback({ nick, uid }: { nick?: string; uid?: string }) {
  const initial = useSensitiveAvatarInitial(nick || uid || '?')
  return (
    <div className="liquid-avatar w-[52px] h-[52px] rounded-full flex items-center justify-center text-[20px] font-semibold">
      {initial}
    </div>
  )
}

function InlineEditProfileButton({ onClick, disabled = false }: { onClick: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      aria-label="编辑资料"
      title="编辑资料"
      onClick={onClick}
      disabled={disabled}
      className="inline-flex h-7 w-7 items-center justify-center rounded-full text-[11px] font-medium transition-colors hover:bg-black/[0.05] disabled:opacity-40 disabled:cursor-not-allowed"
      style={{
        color: 'var(--text-secondary)',
        border: '1px solid var(--control-border)',
        background: 'var(--surface-card-strong)',
      }}
    >
      <Pencil className="h-3 w-3" />
    </button>
  )
}

function openBundleTransferPage() {
  window.history.pushState({}, '', '/bundle')
  window.dispatchEvent(new Event('popstate'))
}

export default function DesktopAccounts() {
  const s = useAccountsPageState()
  const {
    accounts, sortedAccounts, profiles, kpi, enabledAccountCount, loadAccounts,
    showQr, qrUrl, qrStatusText, qrScanned, qrConflict, unbindApplying, handleAddAccount, handleReloginAccount, handleApplyAccountUnbind, refreshQr, closeQr,
    refreshingAll, handleRefreshAll,
    expandedUid, toggleExpand,
    handleRefreshAccount, handleDeleteAccount,
    savingMap,
    locationModal, openLocationModal, closeLocationModal,
  } = s
  const [profileEditorTarget, setProfileEditorTarget] = useState<any | null>(null)
  const latestDetailTriggerRef = useRef<HTMLButtonElement | null>(null)
  const accountCountLabel = `本机账号 ${enabledAccountCount} 个`
  const drawerAccount = sortedAccounts.find((account: any) => account.uid === expandedUid) || null
  const drawerProfile = drawerAccount ? profiles[drawerAccount.uid] || null : null
  const closeDetailDrawer = useCallback(() => {
    if (expandedUid) toggleExpand(expandedUid)
  }, [expandedUid, toggleExpand])

  return (
    <>

      <div className="desktop-scroll-hidden flex-1 overflow-y-auto px-4 py-4 md:px-6">
        <section
          className="surface-card mb-4 rounded-[24px] px-5 py-4"
        >
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-[20px] font-semibold tracking-[-0.02em]" style={{ color: 'var(--text-main)' }}>
                账号
              </h1>
              <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                {accountCountLabel}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <SensitiveVisibilityToggle compact />
              <button onClick={openBundleTransferPage} className="btn-secondary btn-sm">
                <Database className="w-3.5 h-3.5" /> 导入/导出
              </button>
              <button onClick={handleRefreshAll} disabled={refreshingAll || !accounts.length} className="btn-ghost btn-sm disabled:opacity-40">
                <RefreshCw className={`w-3.5 h-3.5 ${refreshingAll ? 'animate-spin' : ''}`} /> 刷新
              </button>
              <button onClick={handleAddAccount} className="btn-primary btn-sm">
                <Plus className="w-3.5 h-3.5" /> 添加
              </button>
            </div>
          </div>
        </section>

        {/* KPI 条 — 极简灰白，仅 danger 时红 */}
        {accounts.length ? (
          <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-7 gap-2.5 mb-4 max-md:min-w-0">
            <KpiPill label="会话可用"   value={`${kpi.online} / ${kpi.total}`} sub={kpi.sessionErrors ? `${kpi.sessionErrors} 个异常` : undefined} danger={kpi.sessionErrors > 0} />
            <KpiPill label="闲鱼币"     value={<SensitiveValue kind="money" value={kpi.coin.toLocaleString()} />} />
            <KpiPill label="平均安全分" value={kpi.avgSec || '—'} sub={kpi.lowSec ? `${kpi.lowSec} 个 <60` : undefined} danger={kpi.lowSec > 0} />
            <KpiPill label="鱼力值"     value={kpi.fp.toLocaleString()} />
            <KpiPill label="累计违规"   value={kpi.vio} danger={kpi.vio > 0} />
            <KpiPill label="30 天销量"  value={kpi.sold} />
            <KpiPill label="累计赚" value={<SensitiveValue kind="money" value={`¥${kpi.totalEarned.toLocaleString()}`} />} />
          </div>
        ) : null}

        {!accounts.length ? (
          <EmptyState icon={Users} title="暂无账号" description='点击右上角「扫码登录」添加账号' />
        ) : (
          <div className={`account-list-grid surface-card rounded-[18px] overflow-hidden${drawerAccount ? ' is-detail-drawer-open' : ''}`}>
            {sortedAccounts.map((a: any) => {
              const expanded = expandedUid === a.uid
              const p = {
                ...accountProfileFallback(a),
                ...(profiles[a.uid] || {}),
              }
              const score = Number(p.securityScore?.totalScore ?? 0)
              const coinV = Number(p.coinBalance ?? 0)
              const fpV = Number(p.fishPower ?? 0)
              const vioV = Number(p.violations?.totalCount ?? 0)
              const soldV = getSellerMetric(p.seller30d, SELLER_METRIC_ALIASES.soldCount)
              const totalEarnedV = Number(p.totalEarned ?? 0)
              const runtimeStatus = accountRuntimeStatus(a)
              const restriction = getAccountRestrictionState(a)
              const sessionReason = restriction.deleteOnly ? '' : accountRuntimeReason(a)
              const publishLocation = composeAccountLocation(a.location)
              const fishShopVisible = hasFishShop(p)
              const autoReplyStatus = getAccountAutoReplyStatus(a)
              const autoShipStatus = getAccountAutoShipStatus(a)
              const accountActions = getAccountActions({
                account: a,
                onRefreshAccount: handleRefreshAccount,
                onOpenLocation: openLocationModal,
                onOpenDetail: toggleExpand,
                onDeleteAccount: handleDeleteAccount,
              })
              return (
                <div className={`account-list-item${expanded ? ' is-detail-active' : ''}`} key={a.uid}>
                  {/* 横行主内容 */}
                  <div className="account-row-main px-5 py-3 transition-colors hover:bg-black/[0.015]">
                    {/* 左：头像 + 状态点 */}
                    <div className="account-row-avatar relative shrink-0">
                      {a.avatar
                        ? <img src={a.avatar} className="w-11 h-11 rounded-full object-cover" style={{ border: '1px solid var(--glass-border)' }} alt="" />
                        : <AvatarFallback nick={a.nick} uid={a.uid} />}
                      <span
                        className="absolute -right-0.5 -bottom-0.5 w-3 h-3 rounded-full"
                        style={{ background: runtimeStatus.tone, boxShadow: '0 0 0 2px var(--surface-card-strong)' }}
                        title={runtimeStatus.label}
                      />
                    </div>

                    {/* 名字 + UID + 添加时间 + Cookie 刷新 */}
                    <div className="account-row-info min-w-0">
                      <div className="flex items-center gap-1.5">
                        <SensitiveValue kind="account" className="account-row-nick text-[14px] font-semibold truncate" value={a.nick || '未知'} />
                        <span
                          className="inline-flex shrink-0 items-center whitespace-nowrap rounded px-1.5 py-[1px] text-[10px] leading-none"
                          style={{ background: runtimeStatus.chipBg, color: runtimeStatus.chipText }}
                          title={sessionReason || runtimeStatus.label}
                        >
                          {runtimeStatus.label}
                        </span>
                        {fishShopVisible ? (
                          <FishShopBadge level={p.fishShopLevel} />
                        ) : null}
                        <InlineEditProfileButton
                          onClick={() => {
                            if (restriction.deleteOnly) return
                            setProfileEditorTarget({ uid: a.uid, account: a, profile: p })
                          }}
                          disabled={restriction.deleteOnly}
                        />
                        {runtimeStatus.key === 'restricted' && a.restriction_end ? (
                          <span
                            className="inline-flex shrink-0 items-center whitespace-nowrap rounded px-1.5 py-[1px] text-[10px] leading-none"
                            style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}
                            title={`处罚结束时间：${formatDateTime(a.restriction_end)}`}
                          >
                            至 {formatDateTime(a.restriction_end)}
                          </span>
                        ) : null}
                      </div>
                      <div className="mt-0.5 flex min-w-0 items-center gap-2">
                        <SensitiveValue kind="uid" className="shrink-0 text-[11.5px] font-mono" value={a.uid} />
                        <button
                          type="button"
                          onClick={() => openLocationModal(a.uid, a.nick)}
                          disabled={restriction.deleteOnly}
                          className="inline-flex min-w-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10.5px] transition-colors hover:bg-black/[0.05] disabled:cursor-not-allowed disabled:opacity-40"
                          style={{ color: publishLocation ? 'var(--primary)' : 'var(--text-muted)' }}
                          title={publishLocation ? `发布地：${publishLocation}，点击修改` : '发布地未设置，点击设置'}
                        >
                          <MapPin className="h-3 w-3 shrink-0" />
                          <span className="truncate">{publishLocation || '设置发布地'}</span>
                        </button>
                      </div>
                      <div className="account-row-meta text-[11px] mt-1 flex items-center gap-2" style={{ color: 'var(--text-secondary)' }}>
                        <AccountOnlineAge raw={a.added_at ?? a.addedAt ?? a.bound_at ?? a.boundAt} />
                        <span aria-hidden style={{ color: 'var(--text-tertiary)' }}>·</span>
                        <span title="最近授权刷新">🍪 {formatRefreshedAt(a.cookie_refreshed_at ?? a.cookieRefreshedAt)}</span>
                      </div>
                      <div className="text-[11px] mt-1 truncate" style={{ color: a.liveStatus === 'connected' || a.live_status === 'connected' ? 'var(--success)' : 'var(--text-muted)' }}>
                        {formatLiveConnectionLabel(a)}
                      </div>
                      {restriction.deleteOnly ? (
                        <div
                          className="text-[11px] mt-1 truncate"
                          style={{ color: 'var(--status-danger-text)' }}
                          title={restriction.restrictionMessage}
                        >
                          {restriction.restrictionMessage}
                        </div>
                      ) : null}
                      {sessionReason ? (
                        runtimeStatus.key === 'session_error' ? (
                          <button
                            type="button"
                            onClick={() => handleReloginAccount(a.uid, a.nick)}
                            disabled={restriction.deleteOnly}
                            className="text-[11px] mt-1 inline-flex items-center gap-1 cursor-pointer hover:underline disabled:opacity-40 disabled:cursor-not-allowed"
                            style={{ color: 'var(--status-danger-text)' }}
                            title={sessionReason}
                          >
                            <Power className="w-3 h-3" />
                            <span>{sessionReason}</span>
                          </button>
                        ) : (
                          <div
                            className="text-[11px] mt-1 truncate"
                            style={{ color: 'var(--text-muted)' }}
                            title={sessionReason}
                          >
                            {sessionReason}
                          </div>
                        )
                      ) : null}
                    </div>

                    {/* 核心指标 */}
                    <div
                      className="account-row-metrics hidden lg:grid min-w-0"
                      aria-label="账号核心指标"
                    >
                      <Metric label="闲鱼币" v={<SensitiveValue kind="money" value={coinV.toLocaleString()} />} />
                      <Metric label="安全分" v={score || '—'} danger={score > 0 && score < 60} warn={score >= 60 && score < 85} />
                      <Metric label="鱼力值" v={fpV.toLocaleString()} />
                      <Metric label="违规"  v={vioV} danger={vioV > 0} />
                      <Metric label="30天销" v={soldV} />
                      <Metric label="累计赚" v={<SensitiveValue kind="money" value={formatCurrencyMetric(totalEarnedV)} />} tone="accent" />
                    </div>

                    {/* 自动回复与自动发货只读配置状态 */}
                    <div className="account-row-capabilities shrink-0 border-l" style={{ borderColor: 'var(--control-border)' }}>
                      <ConfigurationStatusChip label="自动回复" status={autoReplyStatus} />
                      <AutoShipStatusChip status={autoShipStatus} />
                    </div>

                    {/* 操作按钮 */}
                    <div className="account-row-actions flex items-center gap-1 shrink-0 ml-auto">
                      {accountActions.filter((action) => action.key !== 'location').map((action) => {
                        const Icon = action.key === 'detail'
                          ? (expanded ? ChevronUp : ChevronDown)
                          : action.icon
                        return (
                          <button
                            key={action.key}
                            onClick={(event) => {
                              if (action.key === 'detail') latestDetailTriggerRef.current = event.currentTarget
                              action.onClick()
                            }}
                            disabled={action.disabled}
                            className={`h-8 px-2.5 rounded-md text-[12px] flex items-center gap-1 transition-colors disabled:opacity-40 disabled:cursor-not-allowed ${
                              action.key === 'detail' ? 'account-detail-trigger ' : ''
                            }${
                              action.danger ? 'hover:bg-red-50' : 'hover:bg-black/[0.06]'
                            }`}
                            style={action.danger ? { color: 'var(--danger)' } : { color: 'var(--text-secondary)' }}
                            title={action.label}
                          >
                            <Icon className={`w-3.5 h-3.5 ${action.key === 'refresh' && savingMap[a.uid] ? 'animate-spin' : ''}`} />
                            {action.shortLabel}
                          </button>
                        )
                      })}
                    </div>
                  </div>

                </div>
              )
            })}
          </div>
        )}
      </div>

      <AccountDetailDrawer
        open={Boolean(drawerAccount)}
        account={drawerAccount}
        profile={drawerProfile}
        returnFocusRef={latestDetailTriggerRef}
        onClose={closeDetailDrawer}
        onAccountUpdated={loadAccounts}
      />

      <AccountProfileEditor
        uid={profileEditorTarget?.uid || ''}
        account={profileEditorTarget?.account || null}
        profile={profileEditorTarget?.profile || null}
        open={!!profileEditorTarget}
        onClose={() => setProfileEditorTarget(null)}
        onProfileUpdated={() => {
          void Promise.resolve(loadAccounts?.()).catch(() => {})
        }}
      />

      <AccountQrLoginModal
        open={showQr}
        qrUrl={qrUrl}
        qrStatusText={qrStatusText}
        qrScanned={qrScanned}
        qrConflict={qrConflict}
        unbindApplying={unbindApplying}
        onClose={closeQr}
        onRefreshQr={refreshQr}
        onApplyAccountUnbind={handleApplyAccountUnbind}
      />

      {/* 发布地（publish_locations）配置 */}
      {locationModal ? (
        <PublishLocationModal
          uid={locationModal.uid}
          nick={locationModal.nick}
          open
          onClose={closeLocationModal}
          onSaved={() => loadAccounts()}
        />
      ) : null}
    </>
  )
}

// ---------------------------------------------------------------------------
// KPI pill — compact card for the top-of-page KPI strip.
// ---------------------------------------------------------------------------
function KpiPill({ label, value, sub, danger = false }: { label: string; value: any; sub?: string; danger?: boolean }) {
  return (
    <div className="surface-card rounded-[14px] px-3 py-2.5">
      <div className="text-[10px]" style={{ color: 'var(--text-muted)' }}>{label}</div>
      <div className="text-[17px] font-semibold font-mono leading-tight mt-0.5" style={{ color: danger ? 'var(--status-danger-text)' : 'var(--text-main)', letterSpacing: '-0.01em' }}>{value}</div>
      {sub ? <div className="text-[10px] mt-0.5 truncate" style={{ color: danger ? 'var(--status-danger-text)' : 'var(--text-muted)' }}>{sub}</div> : null}
    </div>
  )
}

// 每行列表里的紧凑指标
function Metric({ label, v, danger = false, warn = false, tone }: any) {
  const color = tone === 'accent' ? 'var(--primary)'
    : danger ? 'var(--status-danger-text)'
    : warn ? 'var(--status-warning-text)'
    : 'var(--text-main)'
  return (
    <div className="account-row-metric text-center min-w-0">
      <div className="account-row-metric-label">{label}</div>
      <div className="account-row-metric-value" style={{ color }}>{v}</div>
    </div>
  )
}

function AutoShipStatusChip({ status }: { status: { enabled: boolean; shortLabel: string; label: string; mismatchCount?: number } }) {
  return (
    <div className="min-w-0">
      <ConfigurationStatusChip label="自动发货" status={status} />
      {status.mismatchCount ? (
        <a
          href="/delivery"
          className="mt-1 inline-flex max-w-full items-center rounded px-1.5 py-0.5 text-[10px] font-medium"
          style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
          title="前往自动发货处理待发订单商品未匹配的配置"
        >
          商品不匹配（{status.mismatchCount}笔）· 去处理
        </a>
      ) : null}
    </div>
  )
}

function ConfigurationStatusChip({ label, status }: { label: string; status: { enabled: boolean } }) {
  const StatusIcon = status.enabled ? Check : Circle
  const accessibleLabel = `${label}：${status.enabled ? '已配置' : '未配置'}`

  return (
    <div
      className="account-row-capability-item"
      title={accessibleLabel}
      aria-label={accessibleLabel}
      style={{ color: status.enabled ? 'var(--status-info-text)' : 'var(--text-muted)' }}
    >
      <StatusIcon className="account-row-capability-icon" aria-hidden="true" />
      <span>{label}</span>
    </div>
  )
}
