// @ts-nocheck
import { useEffect, useMemo, useRef, useState } from 'react'
import { Coins, LineChart, Pencil, RefreshCw, Shield, Sparkles } from 'lucide-react'
import {
  accountCoinDetail,
  accountProfile,
  accountSellerData,
  accountSecurityScoreRecords,
  accountViolationsAll,
} from '../../api/index'
import { getAuthToken } from '../../api/client'
import {
  getSellerMetric,
  SELLER_METRIC_ALIASES,
  shouldForceAccountProfileRefresh,
} from '../../hooks/useAccountProfiles'
import {
  accountRuntimeReason,
  accountRuntimeStatus,
} from './hooks/useAccountsPageState'
import { FishShopBadge, hasFishShop } from './FishShopBadge'
import { useAuthStore } from '../../store/authStore'
import { FishShopDataCompassPanel } from './FishShopDataCompassPanel'
import { AccountProfileEditor } from './AccountProfileEditor'
import { getAccountRestrictionState } from './accountUi'
import { SensitiveValue, useSensitiveAvatarInitial } from '../../components/SensitiveValue'
import {
  asRecord,
  getNestedList,
  pickString,
} from '../promotions/promotionUtils'

type DetailMode = 'security' | 'coin' | 'datacompass'

export function buildExposureDetailQueryFromHistory(historyRow) {
  const containers = [
    historyRow,
    asRecord(historyRow?.detail),
    asRecord(historyRow?.extra),
    asRecord(historyRow?.ext),
    asRecord(historyRow?.bizExt),
    asRecord(historyRow?.itemInfo),
    asRecord(historyRow?.targetInfo),
  ]
  const query = {}

  for (const container of containers) {
    if (!query.instanceId) {
      const value = pickString(container, ['instanceId', 'instance_id'], '')
      if (value) query.instanceId = value
    }
    if (!query.bizCode) {
      const value = pickString(container, ['bizCode', 'biz_code'], '')
      if (value) query.bizCode = value
    }
    if (!query.itemId) {
      const value = pickString(container, ['itemId', 'item_id'], '')
      if (value) query.itemId = value
    }
    if (!query.taskInstanceId) {
      const value = pickString(container, ['taskInstanceId', 'task_id', 'taskId'], '')
      if (value) query.taskInstanceId = value
    }
    if (!query.date) {
      const value = pickString(container, ['date', 'ds'], '')
      if (value) query.date = value
    }
  }

  if (query.itemId || query.taskInstanceId || (query.instanceId && query.bizCode)) {
    return query
  }
  return null
}

export function findExposureDetailRequestFromHistory(historyRows) {
  const rows = Array.isArray(historyRows) ? historyRows : []
  for (const row of rows) {
    const historyRow = row && typeof row === 'object' ? row : {}
    const query = buildExposureDetailQueryFromHistory(historyRow)
    if (!query) continue
    return {
      label: pickString(historyRow, ['itemTitle', 'title', 'itemId'], '曝光明细'),
      query,
    }
  }
  return null
}

function extractExposureHistoryEntries(payload) {
  return getNestedList(payload, ['historyList', 'list', 'items'])
}

function extractExposureDetailEntries(payload) {
  return getNestedList(payload, ['detailList', 'list', 'items'])
}

function useIsMobileViewport() {
  const [isMobileViewport, setIsMobileViewport] = useState(() => (
    typeof window !== 'undefined' ? window.innerWidth < 768 : false
  ))

  useEffect(() => {
    if (typeof window === 'undefined') return undefined

    const media = typeof window.matchMedia === 'function'
      ? window.matchMedia('(max-width: 767px)')
      : null
    const sync = () => setIsMobileViewport(media ? media.matches : window.innerWidth < 768)

    sync()
    if (media?.addEventListener) {
      media.addEventListener('change', sync)
      return () => media.removeEventListener('change', sync)
    }

    window.addEventListener('resize', sync)
    return () => window.removeEventListener('resize', sync)
  }, [])

  return isMobileViewport
}

function gradient(score) {
  if (score >= 80) return 'linear-gradient(135deg, var(--success), color-mix(in srgb, var(--success) 72%, white))'
  if (score >= 60) return 'linear-gradient(135deg, var(--warning), color-mix(in srgb, var(--warning) 54%, white))'
  return 'linear-gradient(135deg, var(--danger), color-mix(in srgb, var(--danger) 68%, white))'
}

function parseTime(raw) {
  if (raw == null || raw === '') return 0
  if (typeof raw === 'number') return raw > 1_000_000_000_000 ? raw : raw * 1000
  const s = String(raw).trim()
  const n = Number(s)
  if (!Number.isNaN(n) && n > 0) return n > 1_000_000_000_000 ? n : n * 1000
  const d = new Date(s.replace(' ', 'T'))
  const t = d.getTime()
  return Number.isNaN(t) ? 0 : t
}

function formatDateTime(value) {
  if (!value) return ''
  const raw = String(value).trim()
  if (!raw) return ''
  let date
  if (/^\d+$/.test(raw)) {
    const n = Number(raw)
    date = new Date(n > 1e12 ? n : n * 1000)
  } else {
    const normalized = /^\d{4}-\d{2}-\d{2}$/.test(raw) ? `${raw} 23:59:59` : raw
    date = new Date(normalized.replace(' ', 'T'))
  }
  if (Number.isNaN(date.getTime())) return raw
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(date)
}

function formatNumber(value, digits = 0) {
  const num = Number(value)
  if (!Number.isFinite(num)) return '—'
  return num.toLocaleString(undefined, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  })
}

function formatCurrency(value) {
  const num = Number(value)
  if (!Number.isFinite(num)) return '—'
  return `¥${num.toLocaleString(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`
}

function extractCoinEntries(payload) {
  const list = payload?.data?.userScoreDetailDOList
    || payload?.userScoreDetailDOList
    || []
  return Array.isArray(list) ? list : []
}

function extractScoreRecords(payload) {
  const list = payload?.data?.data
    || payload?.data?.list
    || payload?.list
    || []
  return Array.isArray(list) ? list : []
}

function extractViolations(payload) {
  const list = payload?.data?.data
    || payload?.data?.list
    || payload?.list
    || []
  return Array.isArray(list) ? list : []
}

function extractSellerBanner(payload) {
  return payload?.data?.data?.graphBannerBenchData
    || payload?.data?.graphBannerBenchData
    || payload?.graphBannerBenchData
    || null
}

function extractSellerGraph(payload) {
  const banner = extractSellerBanner(payload)
  return Array.isArray(banner?.graphDataList) ? banner.graphDataList : []
}

function scoreReason(record, violations) {
  const type = String(record?.changeTypeName || '').trim()
  if (!type) return '安全分变动'
  if (!record?.decrease) return type
  const after = Number(record?.scoreAfter ?? 0)
  const before = Number(record?.scoreBefore ?? 0)
  const matched = violations.find((v) => {
    const punishTime = parseTime(v?.punishTime)
    const recordTime = parseTime(record?.gmtCreate)
    return (
      Math.abs(punishTime - recordTime) < 60_000
      || Number(v?.point) === Math.abs(before - after)
      || Number(v?.point) === Number(record?.realChangeScore ?? 0)
    )
  })
  const reason =
    matched?.viewConfig?.viewConfig?.punishReason
    || matched?.viewConfig?.viewConfig?.punishReasonFromPc
    || matched?.reason
    || matched?.punishTitle
    || matched?.viewConfig?.name
  return reason ? `${type} · ${reason}` : type
}

function buildSecurityTimeline(scoreRecords, violations) {
  const usedScoreIndexes = new Set()
  const usedViolationIndexes = new Set()
  const entries = []

  const violationReason = (violation) => (
    violation?.viewConfig?.viewConfig?.punishReason
    || violation?.viewConfig?.viewConfig?.punishReasonFromPc
    || violation?.reason
    || violation?.punishResult
    || ''
  )
  const violationTitleReason = (violation) => (
    violation?.viewConfig?.viewConfig?.punishReason
    || violation?.viewConfig?.viewConfig?.punishReasonFromPc
    || violation?.reason
    || violation?.punishTitle
    || violation?.viewConfig?.name
    || ''
  )
  const scoreDelta = (record) => {
    const value = Number(record?.realChangeScore ?? 0)
    return record?.decrease ? -value : value
  }

  violations.forEach((violation, violationIndex) => {
    const matchedScoreIndex = scoreRecords.findIndex((record, scoreIndex) => {
      if (usedScoreIndexes.has(scoreIndex) || !record?.decrease) return false
      const punishTime = parseTime(violation?.punishTime)
      const recordTime = parseTime(record?.gmtCreate)
      const after = Number(record?.scoreAfter ?? 0)
      const before = Number(record?.scoreBefore ?? 0)
      return (
        Math.abs(punishTime - recordTime) < 60_000
        || Number(violation?.point) === Math.abs(before - after)
        || Number(violation?.point) === Number(record?.realChangeScore ?? 0)
      )
    })
    if (matchedScoreIndex < 0) return

    usedScoreIndexes.add(matchedScoreIndex)
    usedViolationIndexes.add(violationIndex)
    const record = scoreRecords[matchedScoreIndex]
    const titleReason = violationTitleReason(violation)
    const type = String(record?.changeTypeName || '').trim()
    entries.push({
      key: `merged-${violationIndex}-${matchedScoreIndex}`,
      kind: 'merged',
      rawTimeMs: parseTime(record?.gmtCreate),
      timeText: formatDateTime(record?.gmtCreate),
      title: type && titleReason ? `${type} · ${titleReason}` : scoreReason(record, [violation]),
      reason: violationReason(violation),
      delta: scoreDelta(record),
      scoreBefore: record?.scoreBefore ?? null,
      scoreAfter: record?.scoreAfter ?? null,
      punishStart: formatDateTime(violation?.punishTime),
      punishEnd: formatDateTime(violation?.punishEndTime),
      active: Number(violation?.status) === 0,
    })
  })

  scoreRecords.forEach((record, index) => {
    if (usedScoreIndexes.has(index)) return
    const recordTime = record?.gmtCreate || record?.gmtModified
    entries.push({
      key: `score_only-${index}`,
      kind: 'score_only',
      rawTimeMs: parseTime(recordTime),
      timeText: formatDateTime(recordTime),
      title: record?.changeTypeName || (record?.decrease ? '安全分扣分' : '安全分恢复'),
      reason: '',
      delta: scoreDelta(record),
      scoreBefore: record?.scoreBefore ?? null,
      scoreAfter: record?.scoreAfter ?? null,
      punishStart: '',
      punishEnd: '',
      active: null,
    })
  })

  violations.forEach((violation, index) => {
    if (usedViolationIndexes.has(index)) return
    entries.push({
      key: `violation_only-${index}`,
      kind: 'violation_only',
      rawTimeMs: parseTime(violation?.punishTime),
      timeText: formatDateTime(violation?.punishTime),
      title: violation?.punishTitle || violation?.viewConfig?.name || '处罚记录',
      reason: violationReason(violation),
      delta: null,
      scoreBefore: null,
      scoreAfter: null,
      punishStart: formatDateTime(violation?.punishTime),
      punishEnd: formatDateTime(violation?.punishEndTime),
      active: Number(violation?.status) === 0,
    })
  })

  return entries.sort((left, right) => right.rawTimeMs - left.rawTimeMs)
}

function hasSellerShopMetrics(profile, sellerPayload) {
  return profile?.fishShopEnabled === true
}

function DetailRail({ tabs, value, onChange }) {
  return (
    <div className="account-inline-detail-tabs" role="tablist" aria-label="账号详情分类">
      {tabs.map((tab) => {
        const active = value === tab.key
        const Icon = tab.icon
        return (
          <button
            key={tab.key}
            type="button"
            role="tab"
            aria-selected={active}
            title={`${tab.label}：${tab.value}。${tab.hint}`}
            onClick={() => onChange(tab.key)}
            className={`account-inline-detail-tab${active ? ' is-active' : ''}`}
          >
            <span className="account-inline-detail-tab-icon" aria-hidden="true">
              <Icon />
            </span>
            <span className="account-inline-detail-tab-copy">
              <span className="account-inline-detail-tab-label">{tab.label}</span>
              <span className="account-inline-detail-tab-value">{tab.value}</span>
            </span>
          </button>
        )
      })}
    </div>
  )
}

function SegmentedTabs({ value, onChange, tabs, isMobile = false }) {
  return (
    <div
      className={isMobile ? 'mobile-soft-card flex gap-2 rounded-[18px] p-1.5' : 'flex gap-2 rounded-[12px] p-1'}
      style={isMobile ? undefined : { background: 'var(--surface-soft)' }}
    >
      {tabs.map((tab) => {
        const active = value === tab.key
        const Icon = tab.icon
        return (
          <button
            key={tab.key}
            type="button"
            onClick={() => onChange(tab.key)}
            className="flex-1 flex items-center justify-center gap-1.5 px-3 py-2 rounded-[10px] text-[12px] font-medium cursor-pointer transition-colors"
            style={{
              background: active
                ? 'linear-gradient(135deg, var(--primary-light), rgba(255, 255, 255, 0.82))'
                : 'transparent',
              color: active ? 'var(--text-main)' : 'var(--text-secondary)',
              border: active
                ? '1px solid color-mix(in srgb, var(--primary) 16%, transparent)'
                : '1px solid transparent',
            }}
          >
            <Icon className="w-3.5 h-3.5" />
            <span>{tab.label}</span>
          </button>
        )
      })}
    </div>
  )
}

function SummaryMetric({ label, value, accent, isMobile = false, sensitive = false }) {
  return (
    <div
      className={isMobile ? 'mobile-soft-card rounded-[18px] p-3' : 'rounded-[12px] p-3'}
      style={isMobile ? undefined : { background: 'var(--surface-soft)', border: '1px solid var(--control-border)' }}
    >
      <div className="text-[11px]" style={{ color: isMobile ? 'var(--text-secondary)' : 'var(--text-muted)' }}>{label}</div>
      <div className={`${isMobile ? 'text-[16px]' : 'text-[18px]'} font-semibold font-mono mt-1`} style={{ color: 'var(--text-main)' }}>{sensitive ? <SensitiveValue kind="money" value={value} /> : value}</div>
      <div className="mt-2 h-1 rounded-full overflow-hidden" style={{ background: 'var(--surface-sunken)' }}>
        <div className="h-full" style={{ width: '100%', background: accent }} />
      </div>
    </div>
  )
}

function EditProfileButton({ onClick, compact = false, disabled = false }) {
  return (
    <button
      type="button"
      aria-label="编辑资料"
      title="编辑资料"
      onClick={onClick}
      disabled={disabled}
      className={compact ? 'inline-flex h-7 w-7 items-center justify-center rounded-full text-[11px] font-medium cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed' : 'inline-flex h-8 w-8 items-center justify-center rounded-full text-[12px] font-medium cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed'}
      style={{
        background: 'var(--surface-soft)',
        color: 'var(--text-main)',
        border: '1px solid var(--control-border)',
      }}
    >
      <Pencil className={compact ? 'w-3 h-3' : 'w-3.5 h-3.5'} />
    </button>
  )
}

function RestrictionNotice({ message, isMobile = false }) {
  return (
    <div
      className={isMobile ? 'rounded-[16px] px-3 py-2 text-[11px]' : 'rounded-[12px] px-3 py-2 text-[11px]'}
      style={{
        background: 'var(--status-danger-bg)',
        color: 'var(--status-danger-text)',
        border: '1px solid color-mix(in srgb, var(--danger) 18%, transparent)',
      }}
    >
      {message}
    </div>
  )
}

function HeaderCard({ account, profile, isMobile = false, onEditProfile, restriction }) {
  const score = Number(profile?.securityScore?.totalScore ?? account?.security_score ?? 0)
  const totalEarned = Number(profile?.totalEarned ?? 0)
  const coin = Number(profile?.coinBalance ?? 0)
  const restrictionEnd = account?.restriction_end
  const runtime = accountRuntimeStatus(account)
  const runtimeReason = restriction?.deleteOnly ? '' : accountRuntimeReason(account)
  const fishShopVisible = hasFishShop(profile)
  const displayNick = String(
    profile?.snsNick
    || profile?.displayNick
    || profile?.nick
    || account?.nick
    || '未知账号',
  )
  const displayAvatar = String(
    profile?.avatar
    || profile?.avatarUrl
    || account?.avatar
    || '',
  )
  const avatarInitial = useSensitiveAvatarInitial(displayNick || account?.uid || '?')
  const profileActionGroup = (
    <div className="inline-flex items-center gap-2 shrink-0">
      {fishShopVisible ? (
        <FishShopBadge level={profile?.fishShopLevel} className="scale-[1.02] origin-left" />
      ) : null}
      <EditProfileButton
        onClick={() => {
          if (restriction?.deleteOnly) return
          onEditProfile()
        }}
        compact={isMobile}
        disabled={restriction?.deleteOnly}
      />
    </div>
  )
  if (isMobile) {
    return (
      <div
        className="mobile-surface-card overflow-hidden p-4"
        style={{
          background:
            'radial-gradient(circle at top right, color-mix(in srgb, var(--primary) 16%, transparent), transparent 34%), linear-gradient(180deg, rgba(255, 255, 255, 0.94), var(--surface-card-strong))',
        }}
      >
        <div className="flex items-start gap-3">
          {displayAvatar
            ? <img src={displayAvatar} className="w-14 h-14 rounded-[18px] object-cover" style={{ border: '1px solid var(--control-border)' }} alt="" />
            : <div className="liquid-avatar w-14 h-14 rounded-[18px] flex items-center justify-center text-[22px] font-semibold">{avatarInitial}</div>}
          <div className="min-w-0 flex-1">
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <div className="text-[11px] font-semibold uppercase tracking-[0.16em]" style={{ color: 'var(--primary)' }}>
                  Account profile
                </div>
                <div className="mt-1 text-[22px] leading-[1.02] font-semibold truncate" style={{ fontFamily: 'Iowan Old Style, Palatino Linotype, Book Antiqua, Georgia, serif', color: 'var(--text-main)' }}>
                  <SensitiveValue kind="account" value={displayNick} />
                </div>
                <div className="mt-1 text-[11px] font-mono truncate" style={{ color: 'var(--text-muted)' }}>
                  UID: <SensitiveValue kind="uid" value={account?.uid} />
                </div>
              </div>
              <span className="mobile-state-badge shrink-0 whitespace-nowrap" style={{ background: runtime.chipBg, color: runtime.chipText }}>
                {runtime.label}
              </span>
            </div>
            <div className="mt-3 flex flex-wrap items-center gap-2">
              {profileActionGroup}
              <span className="mobile-pill mobile-muted-pill text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                安全分 {score ? `${score}/100` : '—'}
              </span>
              <span className="mobile-pill mobile-muted-pill text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                闲鱼币 {formatNumber(coin)}
              </span>
            </div>
          </div>
        </div>

        {account?.status === 'restricted' && restrictionEnd ? (
          <div className="mt-3 rounded-[16px] px-3 py-2 text-[11px]" style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}>
            处罚结束：{formatDateTime(restrictionEnd)}
          </div>
        ) : null}
        {runtimeReason ? (
          <div
            className="mt-3 rounded-[16px] px-3 py-2 text-[11px]"
            style={{
              background: runtime.key === 'session_error' ? 'var(--status-danger-bg)' : 'var(--surface-soft)',
              color: runtime.key === 'session_error' ? 'var(--status-danger-text)' : 'var(--text-secondary)',
            }}
          >
            {runtimeReason}
          </div>
        ) : null}
        {restriction?.deleteOnly ? (
          <div className="mt-3">
            <RestrictionNotice message={restriction.restrictionMessage} isMobile />
          </div>
        ) : null}

        <div className="grid grid-cols-3 gap-2.5 mt-4">
          <SummaryMetric label="安全分" value={score ? `${score}/100` : '—'} accent={gradient(score || 0)} isMobile />
            <SummaryMetric label="闲鱼币" value={formatNumber(coin)} accent="linear-gradient(135deg, var(--warning), color-mix(in srgb, var(--warning) 54%, white))" isMobile sensitive />
          <SummaryMetric label="累计赚" value={formatCurrency(totalEarned)} accent="linear-gradient(135deg, var(--primary), #5B8BEE)" isMobile sensitive />
        </div>
      </div>
    )
  }

  return (
    <div className="surface-card rounded-[18px] p-4">
      <div className="flex items-start gap-3">
        {displayAvatar
          ? <img src={displayAvatar} className="w-14 h-14 rounded-full object-cover" alt="" />
          : <div className="liquid-avatar w-14 h-14 rounded-full flex items-center justify-center text-[22px] font-semibold">{avatarInitial}</div>}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <SensitiveValue kind="account" className="text-[18px] font-semibold" value={displayNick} />
            {profileActionGroup}
            <span
              className="inline-flex shrink-0 items-center whitespace-nowrap rounded-full px-2 py-[2px] text-[11px] leading-none"
              style={{ background: runtime.chipBg, color: runtime.chipText }}
            >
              {runtime.label}
            </span>
            {account?.status === 'restricted' && restrictionEnd ? (
              <span className="text-[11px]" style={{ color: 'var(--status-warning-text)' }}>
                处罚结束：{formatDateTime(restrictionEnd)}
              </span>
            ) : null}
            {runtimeReason ? (
              <span className="text-[11px]" style={{ color: runtime.key === 'session_error' ? 'var(--status-danger-text)' : 'var(--text-muted)' }}>
                {runtimeReason}
              </span>
            ) : null}
          </div>
          <div className="text-[12px] font-mono mt-1" style={{ color: 'var(--text-muted)' }}>
            UID: <SensitiveValue kind="uid" value={account?.uid} />
          </div>
        </div>
      </div>
      {restriction?.deleteOnly ? (
        <div className="mt-3">
          <RestrictionNotice message={restriction.restrictionMessage} />
        </div>
      ) : null}
      <div className="grid grid-cols-3 gap-3 mt-4">
        <SummaryMetric label="安全分" value={score ? `${score}/100` : '—'} accent={gradient(score || 0)} />
        <SummaryMetric label="闲鱼币" value={formatNumber(coin)} accent="linear-gradient(135deg, var(--warning), color-mix(in srgb, var(--warning) 54%, white))" sensitive />
        <SummaryMetric label="累计赚" value={formatCurrency(totalEarned)} accent="linear-gradient(135deg, var(--primary), #4F8EF7)" sensitive />
      </div>
    </div>
  )
}

function InfoCard({ title, children, isMobile = false, note = '' }) {
  return (
    <div
      className={isMobile ? 'mobile-surface-card p-4' : 'rounded-[16px] p-4'}
      style={isMobile ? undefined : { background: 'var(--surface-card)', border: '1px solid var(--glass-border)', boxShadow: 'var(--glass-shadow-soft)' }}
    >
      {isMobile ? (
        <div className="mobile-section-head mb-3 items-start">
          <div className="mobile-section-copy">
            <div className="mobile-section-title text-[20px]">{title}</div>
            {note ? <div className="mobile-section-note">{note}</div> : null}
          </div>
        </div>
      ) : (
        <div className="text-[14px] font-semibold mb-3" style={{ color: 'var(--text-main)' }}>{title}</div>
      )}
      {children}
    </div>
  )
}

function EmptyMini({ text, isMobile = false }) {
  return (
    <div
      className={isMobile ? 'mobile-soft-card rounded-[18px] px-4 py-5 text-center text-[12px]' : 'rounded-[12px] px-4 py-5 text-center text-[12px]'}
      style={isMobile ? { color: 'var(--text-secondary)' } : { background: 'var(--surface-soft)', color: 'var(--text-muted)' }}
    >
      {text}
    </div>
  )
}

function SecurityPanel({ profile, scoreRecords, violations, isMobile = false }) {
  const score = Number(profile?.securityScore?.totalScore ?? 0)
  const timeline = useMemo(() => buildSecurityTimeline(scoreRecords, violations), [scoreRecords, violations])
  return (
    <div className="space-y-3">
      <InfoCard title="安全分概览" isMobile={isMobile} note={isMobile ? '先看当前分值，再追踪最近一次扣分或恢复。' : ''}>
        <div className={`${isMobile ? 'mobile-soft-card rounded-[18px] px-4 py-4' : ''} flex items-end gap-3`}>
          <div className="text-[34px] leading-none font-semibold font-mono" style={{ color: score >= 80 ? 'var(--status-success-text)' : score >= 60 ? 'var(--status-warning-text)' : 'var(--status-danger-text)' }}>
            {score || '—'}
          </div>
          <div className="text-[12px] mb-1" style={{ color: 'var(--text-muted)' }}>
            最近更新时间：{profile?.securityScore?.gmtModified || '—'}
          </div>
        </div>
      </InfoCard>
      <InfoCard title="安全分时间线" isMobile={isMobile} note={isMobile ? '处罚记录和分值变化按时间合并，便于在手机上一眼判断。' : ''}>
        {!timeline.length ? (
          <EmptyMini text="暂无安全分变动" isMobile={isMobile} />
        ) : (
          <div className="space-y-2">
            {timeline.map((entry) => {
              const scoreOnlyRecovery = entry.kind === 'score_only' && entry.delta >= 0
              const bg = scoreOnlyRecovery
                ? 'var(--status-success-bg)'
                : entry.kind === 'score_only'
                  ? 'var(--status-danger-bg)'
                  : 'var(--status-warning-bg)'
              const border = scoreOnlyRecovery ? 'rgba(52,199,89,0.18)' : entry.kind === 'score_only' ? 'rgba(239,68,68,0.15)' : 'rgba(245,158,11,0.18)'
              const titleColor = scoreOnlyRecovery ? 'var(--status-success-text)' : entry.kind === 'score_only' ? 'var(--status-danger-text)' : 'var(--status-warning-text)'
              const subtleColor = titleColor
              return (
                <div
                  key={entry.key}
                  className={isMobile ? 'rounded-[18px] p-3.5' : 'rounded-[12px] p-3'}
                  style={{ background: bg, border: `1px solid ${border}`, boxShadow: isMobile ? '0 12px 28px rgba(96, 74, 28, 0.06)' : 'none' }}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0 flex-1">
                      <div className="text-[13px] font-semibold" style={{ color: titleColor }}>
                        {entry.title}
                      </div>
                      {entry.reason ? (
                        <div className="text-[12px] mt-1" style={{ color: subtleColor }}>{entry.reason}</div>
                      ) : null}
                      <div className="text-[11px] mt-2" style={{ color: 'var(--text-muted)' }}>
                        {entry.timeText}
                        {entry.kind !== 'score_only' && entry.punishStart
                          ? ` · 处罚 ${entry.punishStart}${entry.punishEnd ? ` 至 ${entry.punishEnd}` : ''}`
                          : null}
                      </div>
                    </div>
                    <div className="text-right shrink-0">
                      {entry.delta != null ? (
                        <div className="text-[15px] font-semibold font-mono" style={{ color: entry.delta < 0 ? 'var(--status-danger-text)' : 'var(--status-success-text)' }}>
                          {entry.delta > 0 ? '+' : ''}{entry.delta}
                        </div>
                      ) : null}
                      {entry.scoreBefore != null && entry.scoreAfter != null ? (
                        <div className="text-[11px] font-mono mt-0.5" style={{ color: 'var(--text-muted)' }}>
                          {entry.scoreBefore} → {entry.scoreAfter}
                        </div>
                      ) : null}
                      {entry.active != null ? (
                        <span className="inline-block mt-1 text-[11px] px-2 py-[2px] rounded-full"
                              style={{
                                background: entry.active ? 'var(--status-warning-bg)' : 'var(--status-neutral-bg)',
                                color: entry.active ? 'var(--status-warning-text)' : 'var(--status-neutral-text)',
                              }}>
                          {entry.active ? '生效中' : '已结束'}
                        </span>
                      ) : null}
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </InfoCard>
    </div>
  )
}

function CoinPanel({ profile, coinData, coinEntries, restriction, isMobile = false }) {
  const balance = Number(
    coinData?.balance
    ?? coinData?.totalBalance
    ?? coinData?.coinBalance
    ?? coinData?.availableBalance
    ?? profile?.coinBalance
    ?? 0,
  )
  return (
    <div className="space-y-3">
      <InfoCard title="闲鱼币" isMobile={isMobile} note={isMobile ? '余额和流水' : ''}>
        <div className="flex items-end justify-between gap-3 mb-3">
          <div>
            <div className="text-[34px] leading-none font-semibold font-mono" style={{ color: 'var(--warning)' }}>
              <SensitiveValue kind="money" value={formatNumber(balance)} />
            </div>
            <div className="text-[12px] mt-2" style={{ color: 'var(--text-muted)' }}>可用余额</div>
          </div>
        </div>
        {restriction?.deleteOnly ? (
          <div className="mb-3">
            <RestrictionNotice message={restriction.restrictionMessage} isMobile={isMobile} />
          </div>
        ) : null}
      </InfoCard>
      <InfoCard title="闲鱼币流水" isMobile={isMobile} note={isMobile ? '仍然按现有接口顺序展示，不改数据口径。' : ''}>
        {!coinEntries.length ? (
          <EmptyMini text="暂无闲鱼币流水" isMobile={isMobile} />
        ) : (
          <div className="space-y-2">
            {coinEntries.map((entry, index) => (
              <div
                key={index}
                  className={isMobile ? 'liquid-notice liquid-notice-warning rounded-[18px] p-3.5' : 'liquid-notice liquid-notice-warning rounded-[12px] p-3'}
                  style={{ boxShadow: isMobile ? '0 12px 24px color-mix(in srgb, var(--warning) 8%, transparent)' : 'none' }}
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-[13px] font-semibold" style={{ color: 'var(--status-warning-text)' }}>
                      {entry?.describe || '闲鱼币变动'}
                    </div>
                    <div className="text-[11px] mt-1" style={{ color: 'var(--status-warning-text)' }}>
                      {entry?.time || '—'}
                    </div>
                  </div>
                  <div className="text-[15px] font-semibold font-mono shrink-0" style={{ color: Number(entry?.score ?? 0) >= 0 ? 'var(--status-warning-text)' : 'var(--status-danger-text)' }}>
                    {Number(entry?.score ?? 0) >= 0 ? '+' : ''}{formatNumber(entry?.score ?? 0)}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </InfoCard>
    </div>
  )
}

function SellerPanel({ profile, sellerBanner, sellerGraph, isMobile = false }) {
  const revenue = getSellerMetric(profile?.seller30d, SELLER_METRIC_ALIASES.revenue)
  const soldCount = getSellerMetric(profile?.seller30d, SELLER_METRIC_ALIASES.soldCount)
  const buyerCount = Number(sellerBanner?.bannerDataList?.find((item) => item?.name === 'payByrCnt')?.data ?? 0)
  const avgOrderValue = Number(sellerBanner?.bannerDataList?.find((item) => item?.name === 'aov')?.data ?? 0)
  const exposure = Number(sellerBanner?.bannerDataList?.find((item) => item?.name === 'showPv')?.data ?? 0)
  return (
    <div className="space-y-3">
      <div className={`grid ${isMobile ? 'grid-cols-2' : 'grid-cols-2 lg:grid-cols-4'} gap-3`}>
        <SummaryMetric label="近30天成交额" value={formatCurrency(revenue)} accent="linear-gradient(135deg, #34C759, #30D158)" isMobile={isMobile} sensitive />
        <SummaryMetric label="近30天成交单数" value={formatNumber(soldCount)} accent="linear-gradient(135deg, var(--primary), #4F8EF7)" isMobile={isMobile} />
        <SummaryMetric label="支付买家数" value={formatNumber(buyerCount)} accent="linear-gradient(135deg, #8B5CF6, #A855F7)" isMobile={isMobile} />
        <SummaryMetric label="客单价" value={formatCurrency(avgOrderValue)} accent="linear-gradient(135deg, #F97316, #FB923C)" isMobile={isMobile} sensitive />
      </div>
      <InfoCard title="近30天成交指标" isMobile={isMobile} note={isMobile ? '保留原 seller summary 数据，只重排信息层次。' : ''}>
        <div className={`grid grid-cols-1 ${isMobile ? '' : 'lg:grid-cols-2'} gap-3`}>
          {[
            { label: '成交额', key: 'payAmt' },
            { label: '成交单数', key: 'payOrdCnt' },
            { label: '支付买家数', key: 'payByrCnt' },
            { label: '客单价', key: 'aov' },
            { label: '曝光量', key: 'showPv' },
            { label: '访客数', key: 'showUv' },
          ].map((metric) => {
            const item = sellerBanner?.bannerDataList?.find((entry) => entry?.name === metric.key)
            return (
              <div
                key={metric.key}
                className={isMobile ? 'mobile-soft-card rounded-[18px] px-3 py-3' : 'rounded-[12px] px-3 py-2.5'}
                style={isMobile ? undefined : { background: 'var(--surface-soft)', border: '1px solid var(--control-border)' }}
              >
                <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>{metric.label}</div>
                <div className="text-[15px] font-semibold font-mono mt-1" style={{ color: 'var(--text-main)' }}>
                  {item?.dataFormat || item?.dataStr || formatNumber(item?.data ?? 0)}
                </div>
                <div className="text-[11px] mt-1" style={{ color: 'var(--text-muted)' }}>
                  {item?.cycle || '近30天'} {item?.ratioFormat ? `· ${item.ratioFormat}` : ''}
                </div>
              </div>
            )
          })}
        </div>
      </InfoCard>
      <InfoCard title="近30天每日走势" isMobile={isMobile} note={isMobile ? '改成连续卡片列表，便于竖向扫读。' : ''}>
        {!sellerGraph.length ? (
          <EmptyMini text="暂无成交走势数据" isMobile={isMobile} />
        ) : (
          <div className="space-y-2">
            {sellerGraph.slice(-30).reverse().map((row, index) => (
              <div
                key={index}
                className={isMobile ? 'mobile-soft-card rounded-[18px] p-3.5' : 'rounded-[12px] p-3'}
                style={isMobile ? undefined : { background: 'var(--surface-soft)', border: '1px solid var(--control-border)' }}
              >
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{String(row?.ds || '—')}</div>
                    <div className="text-[11px] mt-1" style={{ color: 'var(--text-muted)' }}>
                      曝光 {formatNumber(row?.showPv ?? 0)} · 访客 {formatNumber(row?.showUv ?? 0)}
                    </div>
                  </div>
                  <div className="text-right shrink-0">
                    <div className="text-[15px] font-semibold font-mono" style={{ color: 'var(--success)' }}>
                      {formatCurrency(row?.payAmt ?? 0)}
                    </div>
                    <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>
                      {formatNumber(row?.payOrdCnt ?? 0)} 单
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </InfoCard>
      <InfoCard title="补充说明" isMobile={isMobile} note={isMobile ? '保持原说明文案，只换成和移动端一致的阅读容器。' : ''}>
        <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
          近 30 天成交额直接读取 seller summary 的 `payAmt`，不是累计赚。累计赚走个人中心“在闲鱼赚了”口径。当前曝光量：{formatNumber(exposure)}。
        </div>
      </InfoCard>
    </div>
  )
}

export function AccountDetailContent({
  uid,
  account = null,
  initialProfile = null,
  showHeader = true,
  refreshToken = 0,
  onAccountUpdated,
}: {
  uid: string
  account?: Record<string, any> | null
  initialProfile?: Record<string, any> | null
  showHeader?: boolean
  refreshToken?: number
  onAccountUpdated?: (() => void | Promise<void>) | null
}) {
  const [mode, setMode] = useState<DetailMode>('security')
  const [loading, setLoading] = useState(false)
  const [profile, setProfile] = useState(initialProfile || {})
  const [scoreRecords, setScoreRecords] = useState([])
  const [violations, setViolations] = useState([])
  const [coinData, setCoinData] = useState({})
  const [coinEntries, setCoinEntries] = useState([])
  const [sellerPayload, setSellerPayload] = useState(null)
  const [profileEditorOpen, setProfileEditorOpen] = useState(false)
  const authToken = useAuthStore((state) => state.token)
  const isMobileViewport = useIsMobileViewport()
  const loadAllRef = useRef(() => {})
  const lastResetScopeRef = useRef('')
  const profileScopeRef = useRef('')

  useEffect(() => {
    const scope = `${authToken || getAuthToken()}:${uid || ''}`
    if (profileScopeRef.current && profileScopeRef.current !== scope) {
      setProfile({})
    } else {
      setProfile(initialProfile || {})
    }
    profileScopeRef.current = scope
  }, [uid, initialProfile, authToken])

  useEffect(() => {
    let cancelled = false
    const requestToken = authToken || getAuthToken()
    const requestUid = uid
    const requestScope = `${requestToken}:${uid || ''}`
    const isStale = () => (
      cancelled
      || getAuthToken() !== requestToken
      || lastResetScopeRef.current !== requestScope
    )

    if (lastResetScopeRef.current !== requestScope) {
      const previousScope = lastResetScopeRef.current
      const previousToken = previousScope.split(':')[0] || ''
      lastResetScopeRef.current = requestScope
      setProfile(previousScope && previousToken !== requestToken ? {} : (initialProfile || {}))
      setScoreRecords([])
      setViolations([])
      setCoinData({})
      setCoinEntries([])
      setSellerPayload(null)
    }

    async function loadAll() {
      if (!requestUid || !requestToken) {
        setLoading(false)
        return
      }
      setLoading(true)
      try {
        const forceProfileRefresh = shouldForceAccountProfileRefresh(account || undefined, profile || undefined)
        const [profileResp, scoreResp, violationsResp, coinResp] = await Promise.allSettled([
          accountProfile(requestUid, forceProfileRefresh),
          accountSecurityScoreRecords(requestUid, 1),
          accountViolationsAll(requestUid, 50),
          accountCoinDetail(requestUid, 1, 50),
        ])
        if (isStale()) return
        const nextProfile = profileResp.status === 'fulfilled' ? (profileResp.value || {}) : {}
        if (profileResp.status === 'fulfilled') setProfile(nextProfile)
        if (scoreResp.status === 'fulfilled') setScoreRecords(extractScoreRecords(scoreResp.value))
        if (violationsResp.status === 'fulfilled') setViolations(extractViolations(violationsResp.value))
        if (coinResp.status === 'fulfilled') {
          setCoinData(coinResp.value?.data || coinResp.value || {})
          setCoinEntries(extractCoinEntries(coinResp.value))
        }
        if (nextProfile?.fishShopEnabled === true) {
          try {
            const sellerResp = await accountSellerData(requestUid, 'recent30d')
            if (!isStale()) setSellerPayload(sellerResp || null)
          } catch {
            if (!isStale()) setSellerPayload(null)
          }
        } else if (!isStale()) {
          setSellerPayload(null)
        }
      } finally {
        if (!isStale()) setLoading(false)
      }
    }
    loadAllRef.current = loadAll
    void loadAll()
    return () => { cancelled = true }
  }, [uid, refreshToken, authToken])

  const sellerBanner = useMemo(() => extractSellerBanner(sellerPayload), [sellerPayload])
  const sellerGraph = useMemo(() => extractSellerGraph(sellerPayload), [sellerPayload])
  const supportsSeller30d = useMemo(() => hasSellerShopMetrics(profile, sellerPayload), [profile, sellerPayload])
  const restriction = useMemo(() => getAccountRestrictionState(account), [account])
  const score = Number(profile?.securityScore?.totalScore ?? account?.security_score ?? 0)
  const coinBalance = Number(profile?.coinBalance ?? 0)
  const revenue30d = getSellerMetric(profile?.seller30d, SELLER_METRIC_ALIASES.revenue)
  const sold30d = getSellerMetric(profile?.seller30d, SELLER_METRIC_ALIASES.soldCount)
  const tabs = useMemo(() => {
    const base = [
      {
        key: 'security',
        label: '安全分',
        icon: Shield,
        value: score ? `${score}/100` : '—',
        hint: score > 0 && score < 60 ? '存在扣分与限制' : '查看扣分与处罚原因',
      },
      {
        key: 'coin',
        label: '闲鱼币',
        icon: Coins,
        value: formatNumber(coinBalance),
        hint: coinEntries.length ? `最近 ${coinEntries.length} 条流水` : '查看余额与流水',
      },
    ]
    if (supportsSeller30d) {
      base.push({
        key: 'datacompass',
        label: '鱼小铺经营',
        icon: LineChart,
        value: revenue30d ? formatCurrency(revenue30d) : '¥0.00',
        hint: sold30d ? `${formatNumber(sold30d)} 单 · 经营数据` : '经营数据',
      })
    }
    return base
  }, [score, coinBalance, coinEntries.length, supportsSeller30d, revenue30d, sold30d])

  useEffect(() => {
    if (!tabs.some((tab) => tab.key === mode)) {
      setMode('security')
    }
  }, [tabs, mode])

  const useMobilePresentation = showHeader && isMobileViewport

  return (
    <div className={showHeader ? 'space-y-4' : 'space-y-2.5'}>
      {showHeader ? (
        <HeaderCard
          account={account}
          profile={profile}
          isMobile={useMobilePresentation}
          restriction={restriction}
          onEditProfile={() => setProfileEditorOpen(true)}
        />
      ) : null}
      <AccountProfileEditor
        uid={uid}
        account={account}
        profile={profile}
        open={profileEditorOpen && !restriction.deleteOnly}
        onClose={() => setProfileEditorOpen(false)}
        isMobile={useMobilePresentation}
        onProfileUpdated={(nextProfile) => {
          setProfile(nextProfile)
          void Promise.resolve(onAccountUpdated?.()).catch(() => {})
        }}
      />
      {showHeader ? (
        <SegmentedTabs value={mode} onChange={setMode} tabs={tabs} isMobile={useMobilePresentation} />
      ) : (
        <DetailRail value={mode} onChange={setMode} tabs={tabs} />
      )}
      <div
        className={
          showHeader
            ? (useMobilePresentation ? 'space-y-3' : '')
            : 'account-inline-detail-body'
        }
      >
        {useMobilePresentation ? (
          <div className="mobile-section-head items-start">
            <div className="mobile-section-copy">
              <span className="mobile-section-kicker">Insight lane</span>
              <div className="mobile-section-title">详情模块</div>
              <div className="mobile-section-note">按安全、闲鱼币、经营数据切换，推广任务统一在推广页处理。</div>
            </div>
            <span className="mobile-pill shrink-0">
              <Sparkles className="w-3.5 h-3.5" />
              Mobile
            </span>
          </div>
        ) : null}
        <div className={showHeader ? '' : 'account-inline-detail-panel'}>
          {loading && !Object.keys(profile || {}).length ? (
            <div
              className={useMobilePresentation ? 'mobile-surface-card rounded-[22px] p-8 text-center text-[13px]' : 'rounded-[16px] p-8 text-center text-[13px]'}
              style={useMobilePresentation ? { color: 'var(--text-secondary)' } : { background: 'var(--surface-card)', border: '1px solid var(--glass-border)', color: 'var(--text-muted)' }}
            >
              加载中…
            </div>
          ) : mode === 'security' ? (
            <SecurityPanel profile={profile} scoreRecords={scoreRecords} violations={violations} isMobile={useMobilePresentation} />
          ) : mode === 'coin' ? (
            <CoinPanel profile={profile} coinData={coinData} coinEntries={coinEntries} restriction={restriction} isMobile={useMobilePresentation} />
          ) : supportsSeller30d ? (
            <FishShopDataCompassPanel uid={uid} />
          ) : (
            <InfoCard title="鱼小铺经营" isMobile={useMobilePresentation} note={useMobilePresentation ? '只有鱼小铺账号才会展示经营数据。' : ''}>
              <EmptyMini text="该账号不是鱼小铺账号，没有鱼小铺经营数据" isMobile={useMobilePresentation} />
            </InfoCard>
          )}
        </div>
      </div>
    </div>
  )
}
