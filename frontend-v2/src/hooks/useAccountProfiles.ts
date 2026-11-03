import { useEffect, useRef, useState } from 'react'
import { accountProfile } from '../api/index'
import { getAuthToken } from '../api/client'
import type { Account } from '../api/types'
import { useAuthStore } from '../store/authStore'
import { isDeleteOnlyAccount } from '../utils/accountAccess'

const PROFILE_REFRESH_INTERVAL_SECONDS = 6000

/**
 * Account profile shape we care about for the dashboard + accounts list.
 * Fields come straight from `GET /api/account/profile?uid=...` which the
 * embedded backend builds from the live MTOP payload — see
 * `src/routes/accounts/summary.rs::build_account_summary_live`.
 *
 * All fields are optional because the upstream MTOP endpoints can be rate-
 * limited / unavailable per account; the UI degrades gracefully when a
 * specific field isn't present.
 *
 * NOTE: `seller30d` is NOT `{soldCount, revenue}` —— it's a Map keyed by the
 * Chinese metric name that the MTOP banner returns (e.g. "成交额",
 * "成交单数"). Use `getSellerMetric()` below to pull a numeric value out.
 */
export interface AccountProfile {
  uid: string
  _cached_at?: number
  coinBalance?: number
  fishPower?: number
  totalEarned?: number
  fishShopEnabled?: boolean
  fishShopLevel?: string
  securityScore?: {
    totalScore?: number
    gmtModified?: string
  }
  violations?: {
    totalCount?: number
  }
  seller30d?: Record<string, { value?: string; cycle?: string; ratio?: string }>
}

function buildSeller30dFromAccount(account?: Account) {
  const sold = String(account?.seller_30d_sold || '').trim()
  if (!sold) return undefined
  return {
    已卖出: {
      value: sold,
      cycle: 'recent30d',
      ratio: '',
    },
  }
}

export function accountProfileFallback(account?: Account): AccountProfile {
  return {
    uid: account?.uid || '',
    _cached_at: Number(account?.summaryCachedAt ?? 0) || undefined,
    coinBalance: Number(account?.coin_balance ?? 0) || 0,
    fishPower: Number(account?.fish_power ?? 0) || 0,
    totalEarned: Number(account?.totalEarned ?? 0) || 0,
    fishShopEnabled: account?.fishShopEnabled === true,
    fishShopLevel: String(account?.fishShopLevel || '').trim(),
    securityScore: Number(account?.security_score ?? 0) > 0
      ? { totalScore: Number(account?.security_score ?? 0) }
      : undefined,
    violations: { totalCount: Number(account?.violation_count ?? 0) || 0 },
    seller30d: buildSeller30dFromAccount(account),
  }
}

/**
 * Pull a numeric value out of `seller30d` by matching any of the provided
 * Chinese name aliases. Strips "¥"/","/"%"/空格 etc. before parsing.
 * Returns 0 if no alias matches or the value isn't parseable as a number.
 */
export function getSellerMetric(seller30d: AccountProfile['seller30d'], aliases: string[]): number {
  if (!seller30d) return 0
  for (const alias of aliases) {
    // 直接命中
    if (seller30d[alias]?.value) {
      const n = parseMetricNumber(seller30d[alias].value)
      if (!isNaN(n)) return n
    }
  }
  // 兜底：模糊匹配 key 包含任意 alias
  for (const [k, v] of Object.entries(seller30d)) {
    if (!v?.value) continue
    if (aliases.some((a) => k.includes(a))) {
      const n = parseMetricNumber(v.value)
      if (!isNaN(n)) return n
    }
  }
  return 0
}

export function shouldForceAccountProfileRefresh(
  account?: Account,
  existing?: AccountProfile,
  nowSeconds = Date.now() / 1000,
) {
  const cachedAt = Number(existing?._cached_at ?? account?.summaryCachedAt ?? 0)
  if (!Number.isFinite(cachedAt) || cachedAt <= 0) return true
  return nowSeconds - cachedAt >= PROFILE_REFRESH_INTERVAL_SECONDS
}

function parseMetricNumber(raw: string | undefined): number {
  if (!raw) return NaN
  // "¥4,520" / "4520元" / "12%" / "21单" 全部清掉非数字
  const cleaned = String(raw).replace(/[^\d.-]/g, '')
  if (!cleaned) return NaN
  return parseFloat(cleaned)
}

// seller30d 指标别名。真正的 MTOP 返回（抓包验证）用**英文 code** 作为 name：
//   payAmt / fstByrPayAmt / rptByrPayAmt / payOrdCnt / aov / rfdAmt / showUv
// 别名里英文在前，中文保留做兜底（某些店铺或老版本可能是中文）。
export const SELLER_METRIC_ALIASES = {
  revenue:   ['payAmt', 'gmv', '成交金额', '成交额', '交易金额', '成交', 'GMV'],
  soldCount: ['payOrdCnt', 'ordCnt', 'orderCount', '成交单数', '成交数', '成交件数', '订单数', '卖出件数'],
  visitor:   ['showUv', 'uv', 'visitor', '新增访客', '访客数', '访客'],
  exposure:  ['showPv', 'pv', 'exposure', '卖家曝光', '曝光量', '曝光'],
  aov:       ['aov', '客单价'],
  refund:    ['rfdAmt', 'refund', '退款金额'],
}

/**
 * Concurrent-limited fetcher: loads `accountProfile()` for each account in
 * `accounts`, at most `concurrency` in flight. Returns a keyed map
 * `{ [uid]: AccountProfile }` that populates incrementally — the UI re-
 * renders as each account lands instead of waiting for the last one.
 *
 * We intentionally swallow per-account errors (returning `{ uid }` with no
 * other fields) so one flaky account doesn't blank the whole dashboard.
 */
export function useAccountProfiles(accounts: Account[], concurrency = 3) {
  const [profiles, setProfiles] = useState<Record<string, AccountProfile>>({})
  const [loading, setLoading] = useState(false)
  const storeAuthToken = useAuthStore((state) => state.token)
  const effectiveAuthToken = storeAuthToken || getAuthToken()
  const profileScopeRef = useRef('')
  const accountsSnapshotRef = useRef<Account[] | null>(null)
  const accountsSignature = accounts
    .map((account) => [
      account.uid,
      account.status,
      account.deleteOnly,
      account.delete_only,
      account.remoteBindingState,
      account.remote_binding_state,
      account.summaryCachedAt,
      account.coin_balance,
      account.fish_power,
      account.violation_count,
      account.totalEarned,
      account.fishShopEnabled,
      account.fishShopLevel,
      account.seller_30d_sold,
    ].map((value) => String(value ?? '')).join('\u001f'))
    .join('\u001e')

  useEffect(() => {
    const requestToken = effectiveAuthToken || getAuthToken()
    const previousScope = profileScopeRef.current
    const sameScope = previousScope === requestToken
    const tokenChanged = Boolean(previousScope) && !sameScope
    const accountsUnchangedDuringTokenSwitch = tokenChanged && accountsSnapshotRef.current === accounts
    profileScopeRef.current = requestToken
    accountsSnapshotRef.current = accounts
    if (accountsUnchangedDuringTokenSwitch) {
      setProfiles({})
      setLoading(false)
      return
    }
    if (!accounts.length) {
      setProfiles((prev) => (Object.keys(prev).length ? {} : prev))
      return
    }
    let cancelled = false
    setLoading(true)
    setProfiles((prev) => {
      const scopedPrev = sameScope ? prev : {}
      const next: Record<string, AccountProfile> = {}
      for (const account of accounts) {
        const fallback = accountProfileFallback(account)
        const existing = scopedPrev[account.uid] || { uid: account.uid }
        next[account.uid] = {
          ...fallback,
          ...existing,
          uid: account.uid,
          securityScore: existing.securityScore || fallback.securityScore,
          violations: existing.violations || fallback.violations,
          seller30d: existing.seller30d || fallback.seller30d,
          fishShopEnabled: existing.fishShopEnabled ?? fallback.fishShopEnabled,
          fishShopLevel: existing.fishShopLevel || fallback.fishShopLevel,
          totalEarned: existing.totalEarned ?? fallback.totalEarned,
          coinBalance: existing.coinBalance ?? fallback.coinBalance,
          fishPower: existing.fishPower ?? fallback.fishPower,
        }
      }
      return next
    })
    const queue = accounts
      .filter((a) => !isDeleteOnlyAccount(a) && (a.status === 'active' || a.status === 'restricted'))
      .map((a) => {
        const existing = sameScope ? profiles[a.uid] : undefined
        const force = shouldForceAccountProfileRefresh(a, existing)
        if (!force) return null
        return { uid: a.uid, force }
      })
      .filter((task) => task !== null)

    async function run() {
      const workers = Array.from({ length: Math.min(concurrency, queue.length) }, async () => {
        while (!cancelled) {
          const task = queue.shift()
          if (!task) return
          const { uid, force } = task
          try {
            const p = (await accountProfile(uid, force)) as AccountProfile
            if (!cancelled && getAuthToken() === requestToken) {
              setProfiles((prev) => ({ ...prev, [uid]: p ?? { uid } }))
            }
          } catch {
            if (!cancelled && getAuthToken() === requestToken) {
              setProfiles((prev) => ({ ...prev, [uid]: { uid } }))
            }
          }
        }
      })
      await Promise.all(workers)
      if (!cancelled && getAuthToken() === requestToken) setLoading(false)
    }
    run()
    return () => {
      cancelled = true
    }
  }, [accountsSignature, concurrency, effectiveAuthToken])

  return { profiles, loading }
}
