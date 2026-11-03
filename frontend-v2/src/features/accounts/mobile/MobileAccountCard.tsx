// @ts-nocheck
// 移动端账号卡 — 单列、竖排，最大化触控面积。
// 点击卡片整体 → 打开 MobileAccountSheet；卡内不再放小按钮，避免误触。
import {
  Coins, Shield, AlertTriangle, ShoppingBag, TrendingUp, Wallet,
} from 'lucide-react'
import {
  getSellerMetric,
  SELLER_METRIC_ALIASES,
  accountProfileFallback,
} from '../../../hooks/useAccountProfiles'
import {
  accountRuntimeStatus,
  accountRuntimeReason,
  formatCurrencyMetric,
  formatDateTime,
  formatLiveConnectionLabel,
  formatRefreshedAt,
} from '../hooks/useAccountsPageState'
import { AccountOnlineAge } from '../AccountOnlineAge'
import { composeAccountLocation } from '../../items/itemUtils'
import { FishShopBadge, hasFishShop } from '../FishShopBadge'
import { getAccountRestrictionState } from '../accountUi'
import { SensitiveValue, useSensitiveAvatarInitial } from '../../../components/SensitiveValue'

interface Props {
  account: any
  profile: any
  onTap: () => void
  onRelogin: (uid: string, nick?: string) => void
}

export function MobileAccountCard({ account, profile, onTap, onRelogin }: Props) {
  const a = account
  const p = {
    ...accountProfileFallback(a),
    ...(profile || {}),
  }
  const runtime = accountRuntimeStatus(a)
  const restriction = getAccountRestrictionState(a)
  const reason = restriction.deleteOnly ? '' : accountRuntimeReason(a)
  const score = Number(p.securityScore?.totalScore ?? 0)
  const coinV = Number(p.coinBalance ?? 0)
  const vioV = Number(p.violations?.totalCount ?? 0)
  const soldV = getSellerMetric(p.seller30d, SELLER_METRIC_ALIASES.soldCount)
  // 跟 desktop DesktopAccounts.tsx 行 90/93 一致：鱼力值 + 累计赚
  const fpV = Number(p.fishPower ?? 0)
  const totalEarnedV = Number(p.totalEarned ?? 0)
  const publishLocation = composeAccountLocation(a.location)
  const fishShopVisible = hasFishShop(p)
  const avatarInitial = useSensitiveAvatarInitial(a.nick || a.uid || '?')

  return (
    <button
      type="button"
      onClick={onTap}
      className="mobile-surface-card w-full text-left p-4 cursor-pointer active:opacity-90 transition-opacity"
    >
      <div className="flex items-start gap-3">
        <div className="relative shrink-0">
          {a.avatar ? (
            <img
              src={a.avatar}
              className="w-12 h-12 rounded-[16px] object-cover"
              style={{ border: '1px solid var(--control-border)' }}
              alt=""
            />
          ) : (
            <div className="liquid-avatar w-12 h-12 rounded-[16px] flex items-center justify-center text-[18px] font-semibold">
              {avatarInitial}
            </div>
          )}
          <span
            className="absolute -right-0.5 -bottom-0.5 w-3 h-3 rounded-full"
            style={{ background: runtime.tone, boxShadow: '0 0 0 2px rgba(255,255,255,0.90)' }}
          />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="flex items-center gap-1.5">
                <SensitiveValue kind="account" className="text-[16px] font-semibold truncate" value={a.nick || '未知'} />
                {fishShopVisible ? (
                  <FishShopBadge level={p.fishShopLevel} />
                ) : null}
              </div>
              <SensitiveValue kind="uid" className="text-[11px] mt-1 font-mono truncate" value={a.uid} />
            </div>
            <span
              className="mobile-state-badge shrink-0"
              style={{ background: runtime.chipBg, color: runtime.chipText }}
            >
              {runtime.label}
            </span>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <AccountOnlineAge
              raw={a.added_at ?? a.addedAt ?? a.bound_at ?? a.boundAt}
              className="mobile-pill mobile-muted-pill text-[11px]"
            />
            <span className="mobile-pill mobile-muted-pill text-[11px]" style={{ color: 'var(--text-secondary)' }}>
              授权 {formatRefreshedAt(a.cookie_refreshed_at ?? a.cookieRefreshedAt)}
            </span>
            <span className="mobile-pill mobile-muted-pill text-[11px]" style={{ color: a.liveStatus === 'connected' || a.live_status === 'connected' ? 'var(--success)' : 'var(--text-secondary)' }}>
              {formatLiveConnectionLabel(a)}
            </span>
          </div>
        </div>
      </div>

      {/* 受限到期 / 会话异常文案 */}
      {runtime.key === 'restricted' && a.restriction_end ? (
        <div
          className="mt-3 text-[11px] px-2.5 py-1.5 rounded-full inline-flex"
          style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}
        >
          受限至 {formatDateTime(a.restriction_end)}
        </div>
      ) : null}

      {/* session_error → 一行可点击的「重新扫码登录」入口；不要 prop 冒泡到卡片 onTap */}
      {reason ? (
        runtime.key === 'session_error' ? (
          <div
            role="button"
            tabIndex={0}
            onClick={(e) => {
              e.stopPropagation()
              onRelogin(a.uid, a.nick)
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.stopPropagation()
                onRelogin(a.uid, a.nick)
              }
            }}
            className="mt-3 inline-flex items-center gap-1 px-2.5 py-1.5 rounded-full text-[11.5px] cursor-pointer"
            style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
          >
            {reason} · 重新扫码
          </div>
        ) : (
          <div className="mt-3 text-[11.5px]" style={{ color: 'var(--text-secondary)' }}>
            {reason}
          </div>
        )
      ) : null}

      {/* 核心指标 — 跟 desktop DesktopAccounts.tsx 行 167-170 一致 6 项：
          闲鱼币 / 安全分 / 违规 / 30天销 / 鱼力值 / 累计赚。
          移动端 3 列 × 2 行布局保证不溢出。 */}
      <div className="grid grid-cols-3 gap-2 mt-4">
        <MetricCell icon={Coins}        label="闲鱼币" v={<SensitiveValue kind="money" value={coinV.toLocaleString()} />} tone="var(--warning)" />
        <MetricCell icon={Shield}       label="安全分" v={score || '—'}
          tone={score > 0 && score < 60 ? 'var(--danger)' : score >= 60 && score < 85 ? 'var(--warning)' : 'var(--success)'} />
        <MetricCell icon={AlertTriangle} label="违规"   v={vioV} tone={vioV > 0 ? 'var(--danger)' : 'var(--text-muted)'} />
        <MetricCell icon={ShoppingBag} label="30天销" v={soldV} tone="var(--primary)" />
        <MetricCell icon={TrendingUp}   label="鱼力值" v={fpV.toLocaleString()} tone="var(--primary)" />
        <MetricCell icon={Wallet}       label="累计赚" v={<SensitiveValue kind="money" value={formatCurrencyMetric(totalEarnedV)} />} tone="var(--danger)" />
      </div>

      <div className="mobile-soft-card mt-4 flex items-center justify-between gap-3 px-3 py-2.5">
        <div className="min-w-0">
          <div className="text-[10px] font-semibold uppercase tracking-[0.16em]" style={{ color: 'var(--primary)' }}>
            Publish
          </div>
          <div className="mt-1 text-[12px] truncate" style={{ color: publishLocation ? 'var(--primary)' : 'var(--text-muted)' }}>
            {publishLocation || '未设置发布地'}
          </div>
        </div>
        <div className="shrink-0 text-[11px] font-medium" style={{ color: 'var(--text-secondary)' }}>
          查看详情
        </div>
      </div>
    </button>
  )
}

function MetricCell({ icon: Icon, label, v, tone }: any) {
  return (
    <div className="mobile-soft-card py-2.5 px-2 text-center">
      <div className="flex items-center justify-center gap-1 text-[10px]" style={{ color: 'var(--text-muted)' }}>
        <Icon className="w-2.5 h-2.5" />
        {label}
      </div>
      <div className="text-[13px] font-semibold font-mono leading-tight mt-1" style={{ color: tone }}>
        {v}
      </div>
    </div>
  )
}
