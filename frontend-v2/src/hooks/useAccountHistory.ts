import { useEffect, useState } from 'react'
import { useAuthStore } from '../store/authStore'
import { getSellerMetric, SELLER_METRIC_ALIASES } from './useAccountProfiles'

/**
 * 账号指标的历史采样（localStorage 持久化）。
 *
 * 后端 /api/account/profile 只返回当前时刻的单点值（totalScore /
 * coinBalance / seller30d.soldCount / revenue）。本 hook 每次拿到新 profile
 * 时追加一条 `{t, sec, coin, sold, gmv}` 到本地存储，用于画趋势曲线。
 *
 * 一个账号最多保存 60 个采样点（足够铺 60 次访问，视觉上 30 个点就很好了）。
 * 同一天内重复访问只保留**最新一条**，避免曲线密到糊。
 */

export interface HistoryPoint {
  t: number              // unix ms
  sec?: number           // securityScore.totalScore
  coin?: number          // coinBalance
  sold?: number          // seller30d.soldCount
  gmv?: number           // seller30d.revenue / soldMoney
  fp?: number            // fishPower
}

const MAX_POINTS = 60
const PREFIX = 'xy.accountHistory.v1.'

function hashScope(value: string): string {
  let hash = 0
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash * 31 + value.charCodeAt(i)) % 2_147_483_647
  }
  return hash.toString(36)
}

function currentOwnerScope(): string {
  const state = useAuthStore.getState()
  return state.user?.username || (state.token ? `token:${hashScope(state.token)}` : 'anonymous')
}

function keyOf(uid: string, ownerScope = currentOwnerScope()) {
  return `${PREFIX}${ownerScope}.${uid}`
}

function loadHistory(uid: string, ownerScope?: string): HistoryPoint[] {
  try {
    const raw = localStorage.getItem(keyOf(uid, ownerScope))
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed : []
  } catch { return [] }
}

function saveHistory(uid: string, list: HistoryPoint[], ownerScope?: string) {
  try {
    localStorage.setItem(keyOf(uid, ownerScope), JSON.stringify(list.slice(-MAX_POINTS)))
  } catch { /* storage full / disabled */ }
}

function sameDay(a: number, b: number): boolean {
  const da = new Date(a); const db = new Date(b)
  return da.getFullYear() === db.getFullYear() && da.getMonth() === db.getMonth() && da.getDate() === db.getDate()
}

/**
 * Appends a sample to `uid`'s history if the profile is non-empty, then
 * returns the full history list. Dedupes same-day samples (newest wins).
 */
export function useAccountHistory(uid: string, profile: any): HistoryPoint[] {
  const ownerScope = useAuthStore((state) => state.user?.username || (state.token ? `token:${hashScope(state.token)}` : 'anonymous'))
  const [history, setHistory] = useState<HistoryPoint[]>(() => uid ? loadHistory(uid, ownerScope) : [])

  useEffect(() => {
    setHistory(uid ? loadHistory(uid, ownerScope) : [])
  }, [uid, ownerScope])

  useEffect(() => {
    if (!uid || !profile) return
    const sec = Number(profile.securityScore?.totalScore ?? 0)
    const coin = Number(profile.coinBalance ?? 0)
    const sold = getSellerMetric(profile.seller30d, SELLER_METRIC_ALIASES.soldCount)
    const gmv = getSellerMetric(profile.seller30d, SELLER_METRIC_ALIASES.revenue)
    const fp = Number(profile.fishPower ?? 0)
    if (!sec && !coin && !sold && !gmv && !fp) return  // 全空忽略

    const now = Date.now()
    const point: HistoryPoint = { t: now, sec, coin, sold, gmv, fp }
    const existing = loadHistory(uid, ownerScope)
    const filtered = existing.filter((p) => !sameDay(p.t, now))
    const next = [...filtered, point]
    saveHistory(uid, next, ownerScope)
    setHistory(next)
  }, [uid, profile, ownerScope])

  return history
}

/**
 * Manual clear for a single account (debug / reset).
 */
export function clearAccountHistory(uid: string) {
  try {
    localStorage.removeItem(keyOf(uid))
    localStorage.removeItem(PREFIX + uid)
  } catch { /* ignore */ }
}
