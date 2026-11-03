// @ts-nocheck
// 移动端账号管理。保留账号数据、操作、sheet、modal 逻辑，
// 只重构移动端视觉结构，让 KPI、列表、状态与操作更接近预览语言。
import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  Users, Plus, RefreshCw, AlertTriangle, ShieldCheck, Wallet, Clock3, Database,
} from 'lucide-react'
import { EmptyState } from '../../components/EmptyState'
import { SensitiveVisibilityToggle } from '../../components/SensitiveValue'
import { PublishLocationModal } from './PublishLocationModal'
import { AccountQrLoginModal } from './AccountQrLoginModal'
import { useAccountsPageState, accountRuntimeStatus } from './hooks/useAccountsPageState'
import { MobileAccountCard } from './mobile/MobileAccountCard'
import { MobileAccountSheet } from './mobile/MobileAccountSheet'
import { accountProfileFallback } from '../../hooks/useAccountProfiles'
import { hasFishShop } from './FishShopBadge'

function SurfaceCard({ children, className = '', style = {} }: any) {
  return (
    <div
      className={`mobile-surface-card ${className}`}
      style={style}
    >
      {children}
    </div>
  )
}
function KpiChip({ label, value, sub, tone = 'var(--text-main)', background = 'var(--surface-control)' }: any) {
  return (
    <div
      className="rounded-[18px] px-3 py-3 min-w-[116px] shrink-0"
      style={{ background, border: '1px solid var(--control-border)' }}
    >
      <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{label}</div>
      <div className="mt-1 text-[16px] font-semibold leading-none" style={{ color: tone }}>{value}</div>
      {sub ? (
        <div className="mt-1 text-[10px]" style={{ color: tone === 'var(--text-main)' ? 'var(--text-muted)' : tone }}>
          {sub}
        </div>
      ) : null}
    </div>
  )
}
function InlineBadge({ children, tone = 'var(--primary)', active = false }: any) {
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-3 py-1.5 text-[11px] font-medium whitespace-nowrap"
      style={{
        color: tone,
        background: active ? 'var(--primary-light)' : 'var(--surface-control)',
        border: `1px solid ${active ? 'color-mix(in srgb, var(--primary) 18%, transparent)' : 'var(--control-border)'}`,
      }}
    >
      {children}
    </span>
  )
}

function BundleEntryButton({ onClick }: any) {
  return (
    <button
      onClick={onClick}
      className="shrink-0 inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[11px] font-medium cursor-pointer"
      style={{ background: 'var(--surface-control)', color: 'var(--text-main)', border: '1px solid var(--control-border)' }}
      aria-label="账号数据导入导出"
      title="账号数据导入导出"
    >
      <Database className="w-3.5 h-3.5" />
      导入/导出
    </button>
  )
}

export default function MobileAccounts() {
  const s = useAccountsPageState()
  const {
    accounts, sortedAccounts, profiles, kpi, enabledAccountCount, loadAccounts,
    showQr, qrUrl, qrStatusText, qrScanned, qrConflict, unbindApplying, handleAddAccount, handleReloginAccount, handleApplyAccountUnbind, refreshQr, closeQr,
    refreshingAll, handleRefreshAll,
    handleRefreshAccount, handleDeleteAccount,
    savingMap, handleToggle,
    locationModal, openLocationModal, closeLocationModal,
  } = s

  const navigate = useNavigate()
  const [actionAccount, setActionAccount] = useState<any>(null)
  const accountCountLabel = `本机账号 ${enabledAccountCount} 个`

  const accountSummary = useMemo(() => {
    const sessionIssueCount = sortedAccounts.filter((account: any) => {
      const key = accountRuntimeStatus(account).key
      return key === 'session_error' || key === 'expired' || key === 'offline'
    }).length
    const restrictedCount = sortedAccounts.filter((account: any) => accountRuntimeStatus(account).key === 'restricted').length
    const fishShopCount = sortedAccounts.filter((account: any) => hasFishShop({
      ...accountProfileFallback(account),
      ...(profiles[account.uid] || {}),
    })).length
    return { sessionIssueCount, restrictedCount, fishShopCount }
  }, [sortedAccounts, profiles])

  return (
    <div
      className="h-full flex flex-col mobile-page-shell"
      style={{ backgroundColor: 'var(--bg-page)' }}
    >
      <div
        className="flex-1 overflow-y-auto mobile-page-body pt-3"
        style={{ paddingBottom: 'calc(env(safe-area-inset-bottom) + 104px)' }}
      >
        <div className="mobile-page-stack">
          <SurfaceCard className="p-4">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="mt-1 text-[18px] font-semibold leading-none" style={{ color: 'var(--text-main)' }}>
                  账号
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <SensitiveVisibilityToggle compact />
                <InlineBadge>
                  <Clock3 className="w-3.5 h-3.5" />
                  {accountCountLabel}
                </InlineBadge>
              </div>
            </div>

            {!accounts.length ? (
              <div className="mt-4 flex justify-end">
                <BundleEntryButton onClick={() => navigate('/bundle')} />
              </div>
            ) : null}

            {accounts.length ? (
              <>
                <div className="mt-4 grid grid-cols-3 gap-2">
                  <KpiChip
                    label="会话可用"
                    value={`${kpi.online} / ${kpi.total}`}
                    sub={kpi.sessionErrors ? `${kpi.sessionErrors} 个异常` : '当前可用会话'}
                    tone={kpi.sessionErrors ? 'var(--danger)' : 'var(--text-main)'}
                    background={kpi.sessionErrors ? 'var(--status-danger-bg)' : 'var(--surface-control)'}
                  />
                  <KpiChip
                    label="累计赚"
                    value={`¥${kpi.totalEarned.toLocaleString()}`}
                    sub={`${kpi.sold.toLocaleString()} 单成交`}
                    tone="var(--warning)"
                  />
                  <KpiChip
                    label="闲鱼币"
                    value={kpi.coin.toLocaleString()}
                    sub={`鱼力值 ${kpi.fp.toLocaleString()}`}
                    tone="var(--primary)"
                  />
                </div>

                <div className="mt-3 flex items-center gap-2">
                  <div className="min-w-0 flex-1 flex gap-2 overflow-x-auto pb-0.5" style={{ scrollbarWidth: 'none' }}>
                    <InlineBadge tone={accountSummary.sessionIssueCount > 0 ? 'var(--danger)' : 'var(--primary)'} active={accountSummary.sessionIssueCount > 0}>
                      <AlertTriangle className="w-3.5 h-3.5" />
                      {accountSummary.sessionIssueCount > 0 ? `${accountSummary.sessionIssueCount} 个需处理` : '会话稳定'}
                    </InlineBadge>
                    <InlineBadge tone={accountSummary.restrictedCount > 0 ? 'var(--status-warning-text)' : 'var(--primary)'} active={accountSummary.restrictedCount > 0}>
                      <ShieldCheck className="w-3.5 h-3.5" />
                      {accountSummary.restrictedCount > 0 ? `${accountSummary.restrictedCount} 个受限` : '无受限账号'}
                    </InlineBadge>
                    <InlineBadge>
                      <Wallet className="w-3.5 h-3.5" />
                      鱼小铺 {accountSummary.fishShopCount} 个
                    </InlineBadge>
                  </div>
                  <BundleEntryButton onClick={() => navigate('/bundle')} />
                  <button
                    onClick={handleRefreshAll}
                    disabled={refreshingAll || !accounts.length}
                    className="shrink-0 inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[11px] font-medium cursor-pointer disabled:opacity-40"
                    style={{ background: 'var(--primary-light)', color: 'var(--primary)' }}
                    aria-label="全部刷新"
                    title="全部刷新 cookie"
                  >
                    <RefreshCw className={`w-3.5 h-3.5 ${refreshingAll ? 'animate-spin' : ''}`} />
                    {refreshingAll ? '刷新中' : '全部刷新'}
                  </button>
                </div>
              </>
            ) : null}
          </SurfaceCard>

          {!accounts.length ? (
            <SurfaceCard className="p-4">
              <EmptyState
                icon={Users}
                title="暂无账号"
                description="点击右下角「+」扫码添加账号"
              />
            </SurfaceCard>
          ) : (
            <div className="flex flex-col gap-2.5">
              {sortedAccounts.map((account: any) => (
                <MobileAccountCard
                  key={account.uid}
                  account={account}
                  profile={profiles[account.uid]}
                  onTap={() => setActionAccount(account)}
                  onRelogin={handleReloginAccount}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      <button
        onClick={handleAddAccount}
        className="fixed right-4 z-40 w-14 h-14 rounded-full flex items-center justify-center cursor-pointer active:scale-95 transition-transform liquid-fab disabled:opacity-40"
        style={{
          bottom: 'calc(env(safe-area-inset-bottom) + 80px)',
        }}
        aria-label="扫码添加账号"
      >
        <Plus className="w-6 h-6" />
      </button>

      <MobileAccountSheet
        open={!!actionAccount}
        account={actionAccount}
        saving={!!(actionAccount && savingMap[actionAccount.uid])}
        onClose={() => setActionAccount(null)}
        onOpenLocation={openLocationModal}
        onOpenDetail={(uid) => navigate(`/accounts/${uid}`)}
        onRefreshAccount={handleRefreshAccount}
        onDeleteAccount={handleDeleteAccount}
        onRelogin={handleReloginAccount}
        onToggle={handleToggle}
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

      {locationModal ? (
        <PublishLocationModal
          uid={locationModal.uid}
          nick={locationModal.nick}
          open
          onClose={closeLocationModal}
          onSaved={() => loadAccounts()}
        />
      ) : null}
    </div>
  )
}
