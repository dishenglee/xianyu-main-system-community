// @ts-nocheck
// Shared business state + handlers for the accounts page.
// Both DesktopAccounts and MobileAccounts consume this hook so logic lives in
// one place; the two pages only differ in their render layer.
//
// Returned shape:
//   {
//     // accounts data
//     accounts, sortedAccounts, profiles, kpi,
//     // QR + add-account flow
//     showQr, qrUrl, qrSid, qrStatusText, qrScanned,
//     handleAddAccount, handleReloginAccount, closeQr,
//     // batch
//     refreshingAll, handleRefreshAll,
//     // inline expand (desktop)
//     expandedUid, toggleExpand, handleRefreshAccount, handleDeleteAccount,
//     // settings toggles
//     savingMap, handleToggle,
//     // publish-location modal
//     locationModal, openLocationModal, closeLocationModal,
//     // helpers (pure)
//     accountRuntimeStatus, accountRuntimeReason,
//     formatCurrencyMetric, formatDateTime,
//     formatAccountAddedAge, formatRefreshedAt,
//   }
import { useEffect, useMemo, useRef, useState } from 'react'
import { useAccounts } from '../../../hooks/useAccounts'
import {
  useAccountProfiles,
  getSellerMetric,
  SELLER_METRIC_ALIASES,
} from '../../../hooks/useAccountProfiles'
import {
  updateAccountSetting,
  addAccount,
  createAccountUnbindRequest,
  deleteAccount,
  qrStatus,
  refreshAccount,
  refreshAllAccounts,
  syncItems,
  syncOrders,
} from '../../../api/index'
import { useNotify } from '../../../components/Notify'
import {
  emitAccountsSynced,
  emitItemsSynced,
  emitOrdersSynced,
  listenSyncEvent,
  XIANYU_ACCOUNTS_SYNCED_EVENT,
} from '../../runtimeEvents'
import { useAppDialog } from '../../../components/AppDialog'
import { useTaskProgressStore, type TaskProgressResult } from '../../../store/taskProgressStore'
import { sortAccountsByAddedAt } from './accountSort'
import { getAuthToken } from '../../../api/client'
import { useAuthScope } from '../../../hooks/useAuthScope'
import { useAuthStore } from '../../../store/authStore'
import { isAdminUser } from '../../../utils/authRole'
import { filterBusinessAccounts, isDeleteOnlyAccount } from '../../../utils/accountAccess'
import {
  emitActionFailure,
  emitActionLog,
  emitActionStarted,
  emitActionSuccess,
} from '../../../bootstrap/actionLogging'

// ---------------------------------------------------------------------------
// Pure helpers — extracted from the original AccountsPage so both Desktop and
// Mobile renderers can call them without re-implementing the rules.
// ---------------------------------------------------------------------------

export function countEnabledAccounts(accounts: any[]) {
  return accounts.filter((account) => (
    String(account?.status || '').trim() !== 'disabled' && !isDeleteOnlyAccount(account)
  )).length
}

export function accountRuntimeStatus(account: any) {
  const platformStatus = String(account?.status || '').trim()
  const sessionStatus = String(account?.last_session_status || '').trim().toLowerCase()

  if (isDeleteOnlyAccount(account)) {
    return {
      key: 'delete_only',
      label: '仅可删除',
      tone: 'var(--danger)',
      chipBg: 'var(--status-danger-bg)',
      chipText: 'var(--status-danger-text)',
    }
  }

  if (platformStatus === 'expired') {
    return {
      key: 'expired',
      label: '失效',
      tone: 'var(--danger)',
      chipBg: 'var(--status-danger-bg)',
      chipText: 'var(--status-danger-text)',
    }
  }

  if (sessionStatus === 'error') {
    return {
      key: 'session_error',
      label: '会话异常',
      tone: 'var(--danger)',
      chipBg: 'var(--status-danger-bg)',
      chipText: 'var(--status-danger-text)',
    }
  }

  if (sessionStatus === 'refreshing' || sessionStatus === 'checking') {
    return {
      key: 'checking',
      label: '检查中',
      tone: 'var(--primary)',
      chipBg: 'var(--status-info-bg)',
      chipText: 'var(--status-info-text)',
    }
  }

  if (platformStatus === 'restricted') {
    return {
      key: 'restricted',
      label: '受限',
      tone: 'var(--warning)',
      chipBg: 'var(--status-warning-bg)',
      chipText: 'var(--status-warning-text)',
    }
  }

  if (platformStatus !== 'active') {
    return {
      key: 'offline',
      label: '离线',
      tone: 'var(--status-neutral-text)',
      chipBg: 'var(--status-neutral-bg)',
      chipText: 'var(--status-neutral-text)',
    }
  }

  return {
    key: 'ok',
    label: '正常',
    tone: 'var(--success)',
    chipBg: 'var(--status-success-bg)',
    chipText: 'var(--status-success-text)',
  }
}

export function accountRuntimeReason(account: any) {
  const runtime = accountRuntimeStatus(account)
  const raw = String(account?.last_session_reason || '').trim()
  const upper = raw.toUpperCase()

  if (runtime.key === 'delete_only') return '该账号已解除或被其他用户绑定，只允许删除'
  if (runtime.key === 'expired') return '登录态已失效，请重新登录'
  if (runtime.key === 'restricted') return '账号当前受限，部分功能不可用'
  if (runtime.key === 'offline') return '账号当前未连接'
  if (runtime.key === 'checking') return '正在检查登录态'

  if (runtime.key === 'session_error') {
    if (
      upper.includes('SESSION_EXPIRED')
      || upper.includes('NOT_LOGIN')
      || upper.includes('TOKEN')
    ) {
      return '登录态已失效，请重新登录'
    }
    if (upper.includes('PROBE_FAILED') || upper.includes('PROBE_ERROR')) {
      return '会话校验失败，请重新登录'
    }
    return '会话异常，请重新登录'
  }

  return ''
}

export function formatCurrencyMetric(value: unknown) {
  const num = Number(value)
  if (!Number.isFinite(num)) return '—'
  return `¥${num.toLocaleString()}`
}

export function isProfileEligibleAccount(account: any) {
  return !isDeleteOnlyAccount(account) && (account?.status === 'active' || account?.status === 'restricted')
}

export function formatDateTime(value: unknown) {
  if (!value) return ''
  const raw = String(value).trim()
  if (!raw) return ''
  const normalized = /^\d{4}-\d{2}-\d{2}$/.test(raw) ? `${raw} 23:59:59` : raw
  const date = new Date(normalized.replace(' ', 'T'))
  if (Number.isNaN(date.getTime())) return normalized
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

function parseTimestampToMs(raw: unknown): number | null {
  if (raw == null || raw === '' || raw === 0) return null
  if (typeof raw === 'number') {
    return raw > 1_000_000_000_000 ? raw : raw * 1000
  }
  if (typeof raw === 'string') {
    const n = Number(raw)
    if (!Number.isNaN(n) && n > 0) return n > 1_000_000_000_000 ? n : n * 1000
    const d = new Date(raw)
    const t = d.getTime()
    return Number.isNaN(t) ? null : t
  }
  return null
}

export function formatAccountAddedAge(raw: unknown): string {
  const days = accountOnlineDays(raw)
  return days == null ? '已在线 —' : `已在线 ${days} 天`
}

export function accountOnlineDays(raw: unknown): number | null {
  const ms = parseTimestampToMs(raw)
  if (!ms) return null
  const days = Math.floor((Date.now() - ms) / 86_400_000)
  return days < 0 ? null : days
}

export const formatAddedAge = formatAccountAddedAge

export function accountAddedAt(account: any): unknown {
  return account?.added_at ?? account?.addedAt ?? account?.bound_at ?? account?.boundAt
}

export function formatRefreshedAt(raw: unknown): string {
  const ms = parseTimestampToMs(raw)
  if (!ms) return '未刷新'
  const diff = Date.now() - ms
  if (diff < 0) return '刚刚'
  const sec = Math.floor(diff / 1000)
  if (sec < 60) return '刚刚'
  if (sec < 3600) return `${Math.floor(sec / 60)} 分钟前`
  if (sec < 86400) return `${Math.floor(sec / 3600)} 小时前`
  const days = Math.floor(sec / 86400)
  if (days < 30) return `${days} 天前`
  if (days < 365) return `${Math.floor(days / 30)} 个月前`
  return `${Math.floor(days / 365)} 年前`
}

function formatDurationSince(raw: unknown): string {
  const ms = parseTimestampToMs(raw)
  if (!ms) return ''
  const diff = Date.now() - ms
  if (diff < 0) return ''
  const sec = Math.floor(diff / 1000)
  if (sec < 60) return '刚刚'
  if (sec < 3600) return `${Math.floor(sec / 60)} 分钟`
  if (sec < 86400) return `${Math.floor(sec / 3600)} 小时`
  const days = Math.floor(sec / 86400)
  if (days < 30) return `${days} 天`
  if (days < 365) return `${Math.floor(days / 30)} 个月`
  return `${Math.floor(days / 365)} 年`
}

export function formatLiveConnectionLabel(input: any): string {
  const status = String(input?.liveStatus ?? input?.live_status ?? input?.status ?? '').trim().toLowerCase()
  const lastConnectedAt = input?.lastConnectedAt ?? input?.liveLastConnectedAt ?? input?.live_last_connected_at
  const lastDisconnectedAt = input?.lastDisconnectedAt ?? input?.liveLastDisconnectedAt ?? input?.live_last_disconnected_at

  if (status === 'connected') {
    const duration = formatDurationSince(lastConnectedAt)
    return duration ? `消息通道已连接 ${duration}` : '消息通道已连接'
  }

  if (status === 'disconnected') {
    const duration = formatDurationSince(lastDisconnectedAt)
    return duration ? `消息通道已断开 ${duration}` : '消息通道已断开'
  }

  if (status === 'connecting' || status === 'checking') return '消息通道连接中'
  if (status === 'error' || status === 'failed') return '消息通道异常'
  return '消息通道未连接'
}

function sleep(ms: number) {
  if (ms <= 0) return Promise.resolve()
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

function findQrMatchedAccount(accounts: any[], beforeUids: Set<string>, resolvedUid: string) {
  const normalizedResolvedUid = String(resolvedUid || '').trim()
  if (normalizedResolvedUid) {
    const matchedResolved = accounts.find((account: any) => String(account?.uid || '').trim() === normalizedResolvedUid)
    if (matchedResolved) return matchedResolved
  }
  return accounts.find((account: any) => {
    const uid = String(account?.uid || '').trim()
    return uid && !beforeUids.has(uid)
  }) || null
}

export async function loadAccountsUntilQrAccountVisible({
  loadAccounts,
  beforeUids,
  resolvedUid = '',
  maxAttempts = 8,
  delayMs = 750,
  isCurrent = () => true,
}: {
  loadAccounts: (options: { force: boolean }) => Promise<any[] | undefined>
  beforeUids: Set<string>
  resolvedUid?: string
  maxAttempts?: number
  delayMs?: number
  isCurrent?: () => boolean
}) {
  let latestAccounts: any[] = []
  let matchedAccount: any = null

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    if (!isCurrent()) return null
    const accounts = await loadAccounts({ force: true })
    if (!isCurrent()) return null

    latestAccounts = Array.isArray(accounts) ? accounts : []
    matchedAccount = findQrMatchedAccount(latestAccounts, beforeUids, resolvedUid)
    if (matchedAccount) break
    if (attempt < maxAttempts - 1) await sleep(delayMs)
  }

  return { accounts: latestAccounts, matchedAccount }
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useAccountsPageState() {
  const { accounts, loadAccounts } = useAccounts()
  const notify = useNotify()
  const { showConfirm, showPrompt } = useAppDialog()
  const taskProgressStart = useTaskProgressStore((state) => state.start)
  const taskProgressTick = useTaskProgressStore((state) => state.tick)
  const taskProgressFinish = useTaskProgressStore((state) => state.finish)
  const authUser = useAuthStore((state) => state.user)
  const isAdmin = isAdminUser(authUser)
  const [refreshingAll, setRefreshingAll] = useState(false)
  const [showQr, setShowQr] = useState(false)
  const [qrUrl, setQrUrl] = useState('')
  const [qrSid, setQrSid] = useState('')
  const [qrStatusText, setQrStatusText] = useState('请用闲鱼 APP 扫码登录')
  const [qrScanned, setQrScanned] = useState(false)
  const [qrConflict, setQrConflict] = useState<any>(null)
  const [unbindApplying, setUnbindApplying] = useState(false)
  const [expandedUid, setExpandedUid] = useState<string | null>(null)
  const [savingMap, setSavingMap] = useState<Record<string, boolean>>({})
  // 发布地 modal — { uid, nick }；null 关闭。扫码成功后会自动挂起一次。
  const [locationModal, setLocationModal] = useState<{ uid: string; nick?: string } | null>(null)
  // 记录扫码登录前的 uid 集合，扫码成功后用 set diff 找出新加入的 uid，
  // 自动为其打开发布地配置弹窗（满足「新账号后立刻提示设置位置」需求）。
  const [preAddUids, setPreAddUids] = useState<Set<string> | null>(null)
  const authTokenRef = useRef(getAuthToken())
  const lastQrStatusRef = useRef('')
  const resetTransientState = () => {
    setRefreshingAll(false)
    setShowQr(false)
    setQrUrl('')
    setQrSid('')
    setQrStatusText('请用闲鱼 APP 扫码登录')
    setQrScanned(false)
    setQrConflict(null)
    setUnbindApplying(false)
    setExpandedUid(null)
    setSavingMap({})
    setLocationModal(null)
    setPreAddUids(null)
  }
  const { authToken } = useAuthScope(() => {
    authTokenRef.current = getAuthToken()
    resetTransientState()
  })
  authTokenRef.current = authToken || getAuthToken()
  const enabledAccountCount = useMemo(() => countEnabledAccounts(accounts), [accounts])

  const isAuthCurrent = (token = authTokenRef.current) => getAuthToken() === token

  async function bootstrapAccountData(uid: string, nick?: string, requestToken = getAuthToken()) {
    const label = nick || uid
    const failures: string[] = []
    const progressResults: TaskProgressResult[] = []
    emitActionStarted({
      module: 'accounts-qr',
      action: 'bootstrap_account_data',
      message: '扫码添加账号后开始同步订单商品',
      context: { uid, nick: nick || '' },
    })

    const syncTasks = [
      {
        label: '订单',
        failureLabel: '订单',
        run: async () => {
          const value = await syncOrders(uid)
          const total = Number(value?.total || 0)
          emitOrdersSynced({ uid, total, source: 'accounts-qr-bootstrap' })
          return { ok: value?.ok !== false, total }
        },
      },
      {
        label: '商品',
        failureLabel: '商品',
        run: async () => {
          const value = await syncItems(uid)
          const total = Number(value?.total || 0)
          emitItemsSynced({ uid, total, source: 'accounts-qr-bootstrap' })
          return { ok: value?.ok !== false, total }
        },
      },
    ]

    taskProgressStart(syncTasks.length, '同步订单商品')
    for (const task of syncTasks) {
      if (!isAuthCurrent(requestToken)) {
        taskProgressFinish([...progressResults])
        return null
      }
      try {
        const result = await task.run()
        if (!isAuthCurrent(requestToken)) {
          taskProgressFinish([...progressResults])
          return null
        }
        if (result.ok === false) {
          failures.push(task.failureLabel)
          emitActionFailure({
            module: 'accounts-qr',
            action: `bootstrap_${task.failureLabel === '订单' ? 'orders' : 'items'}`,
            message: `${task.failureLabel}同步失败`,
            context: { uid, nick: nick || '', total: Number(result.total || 0) },
            promoteToIncident: false,
          })
          progressResults.push({
            uid,
            label: `${task.label} · ${label}`,
            ok: false,
            msg: '同步失败',
          })
        } else {
          emitActionSuccess({
            module: 'accounts-qr',
            action: `bootstrap_${task.failureLabel === '订单' ? 'orders' : 'items'}`,
            message: `${task.failureLabel}同步成功`,
            context: { uid, nick: nick || '', total: Number(result.total || 0) },
          })
          progressResults.push({
            uid,
            label: `${task.label} · ${label}`,
            ok: true,
            msg: `${Number(result.total || 0).toLocaleString()} 条`,
          })
        }
      } catch (error) {
        if (!isAuthCurrent(requestToken)) {
          taskProgressFinish([...progressResults])
          return null
        }
        failures.push(task.failureLabel)
        emitActionFailure({
          module: 'accounts-qr',
          action: `bootstrap_${task.failureLabel === '订单' ? 'orders' : 'items'}`,
          message: `${task.failureLabel}同步异常`,
          error,
          context: { uid, nick: nick || '' },
        })
        progressResults.push({
          uid,
          label: `${task.label} · ${label}`,
          ok: false,
          msg: '同步失败',
        })
      }
      taskProgressTick(progressResults.length, [...progressResults])
    }

    if (!isAuthCurrent(requestToken)) {
      taskProgressFinish([...progressResults])
      return null
    }
    taskProgressFinish([...progressResults])

    if (failures.length) {
      emitActionLog({
        module: 'accounts-qr',
        action: 'bootstrap_account_data',
        outcome: 'warn',
        message: '扫码添加账号后订单商品部分同步失败',
        context: { uid, nick: nick || '', failures: [...new Set(failures)] },
        promoteToIncident: false,
      })
      notify(`${label} 已添加，但订单/商品同步未完成：${[...new Set(failures)].join('、')}`, 'error', 4200)
      return { ok: false, failures }
    }
    emitActionSuccess({
      module: 'accounts-qr',
      action: 'bootstrap_account_data',
      message: '扫码添加账号后订单商品同步完成',
      context: { uid, nick: nick || '', failures: [] },
    })
    notify(`${label} 已添加，订单和商品已同步`, 'success', 2600)
    return { ok: true, failures: [] }
  }

  useEffect(() => { loadAccounts() }, [authToken])
  useEffect(() => {
    return listenSyncEvent(XIANYU_ACCOUNTS_SYNCED_EVENT, () => {
      void loadAccounts()
    })
  }, [loadAccounts])

  const { profiles } = useAccountProfiles(accounts)

  const sortedAccounts = useMemo(() => {
    return sortAccountsByAddedAt(accounts)
  }, [accounts])

  // Aggregate KPIs from /api/account/profile — missing fields contribute 0 so
  // the strip never fabricates data when a single account is rate-limited.
  const kpi = useMemo(() => {
    let coin = 0, fp = 0, vio = 0, sold = 0, totalEarned = 0, secSum = 0, secN = 0, lowSec = 0
    const eligible = accounts.filter((a: any) => isProfileEligibleAccount(a))
    const active = filterBusinessAccounts(accounts)
    const available = active.filter((a: any) => {
      const runtime = accountRuntimeStatus(a).key
      return runtime === 'ok' || runtime === 'checking'
    })
    const sessionErrors = active.filter((a: any) => accountRuntimeStatus(a).key === 'session_error')
    for (const a of eligible) {
      const p = profiles[a.uid] || {}
      coin += Number(p.coinBalance ?? 0)
      fp += Number(p.fishPower ?? 0)
      vio += Number(p.violations?.totalCount ?? 0)
      sold += getSellerMetric(p.seller30d, SELLER_METRIC_ALIASES.soldCount)
      totalEarned += Number(p.totalEarned ?? 0)
      const s = Number(p.securityScore?.totalScore ?? 0)
      if (s > 0) { secSum += s; secN += 1 }
      if (s > 0 && s < 60) lowSec += 1
    }
    return {
      online: available.length,
      total: accounts.length,
      sessionErrors: sessionErrors.length,
      coin, fp, vio, sold, totalEarned,
      avgSec: secN ? Math.round(secSum / secN) : 0,
      lowSec,
    }
  }, [accounts, profiles])

  async function handleRefreshAll() {
    if (refreshingAll) return
    const requestToken = getAuthToken()
    setRefreshingAll(true)
    try {
      const r: any = await refreshAllAccounts()
      if (!isAuthCurrent(requestToken)) return
      await loadAccounts()
      emitAccountsSynced({ source: 'accounts-refresh-all' })
      const ok = r?.ok_count ?? 0
      const fail = r?.fail_count ?? 0
      notify(`cookie 刷新完成：成功 ${ok}，失败 ${fail}`, fail > 0 ? 'error' : 'success', 3600)
    } catch (e: any) {
      if (isAuthCurrent(requestToken)) notify('刷新失败：' + (e?.message || e), 'error', 3600)
    } finally {
      if (isAuthCurrent(requestToken)) setRefreshingAll(false)
    }
  }

  async function handleAddAccount() {
    const requestToken = getAuthToken()
    try {
      setQrUrl('')
      setQrSid('')
      setQrScanned(false)
      setQrConflict(null)
      setQrStatusText('正在生成二维码…')
      setShowQr(true)
      lastQrStatusRef.current = ''
      emitActionStarted({
        module: 'accounts-qr',
        action: 'create_qr',
        message: '开始生成扫码添加账号二维码',
        context: {
          enabledAccountCount,
        },
      })
      // 快照当前 uid 集合，扫码成功后用它做 diff 找新账号。
      setPreAddUids(new Set(accounts.map((a: any) => a.uid)))
      const r: any = await addAccount()
      if (!isAuthCurrent(requestToken)) return
      const url = r?.qr_code_url || r?.qrUrl || r?.qr_url || ''
      const sid = r?.session_id || r?.sessionId || ''
      if (!url) {
        setQrStatusText('生成二维码失败：' + (r?.message || '响应缺少 qr_code_url'))
        emitActionFailure({
          module: 'accounts-qr',
          action: 'create_qr',
          message: '生成扫码添加账号二维码失败',
          detail: r?.message || '响应缺少 qr_code_url',
          context: { hasSessionId: Boolean(sid) },
        })
        return
      }
      setQrUrl(url)
      setQrSid(sid)
      const expiresIn = Number(r?.expires_in ?? 0)
      setQrStatusText(expiresIn > 0 ? `请用闲鱼 APP 扫码登录（剩余 ${expiresIn} 秒）` : '请用闲鱼 APP 扫码登录')
      emitActionSuccess({
        module: 'accounts-qr',
        action: 'create_qr',
        message: '扫码添加账号二维码生成成功',
        context: { sessionId: sid, expiresIn },
      })
    } catch (e: any) {
      if (isAuthCurrent(requestToken)) setQrStatusText('生成失败：' + e.message)
      emitActionFailure({
        module: 'accounts-qr',
        action: 'create_qr',
        message: '生成扫码添加账号二维码异常',
        error: e,
      })
    }
  }

  async function refreshQr() {
    await handleAddAccount()
  }

  async function handleReloginAccount(uid: string, nick?: string) {
    setQrStatusText(`请用闲鱼 APP 扫码登录，更新账号「${nick || uid}」`)
    await handleAddAccount()
  }

  function closeQr() {
    setShowQr(false)
  }

  // QR 状态轮询 — sid 改变（或 modal 关闭）时清理。
  useEffect(() => {
    if (!showQr || !qrSid) return
    let cancelled = false
    const requestToken = getAuthToken()
    const timer = setInterval(async () => {
      try {
        const s: any = await qrStatus(qrSid)
        if (cancelled || !isAuthCurrent(requestToken)) return
        const st = s?.status
        if (st && st !== 'waiting' && lastQrStatusRef.current !== st) {
          lastQrStatusRef.current = st
          emitActionLog({
            module: 'accounts-qr',
            action: 'qr_status',
            outcome: st === 'success' ? 'success' : st === 'scanned' ? 'started' : st === 'owner_conflict' || st === 'import_error' ? 'failed' : 'warn',
            message: `扫码添加账号状态：${st}`,
            context: {
              sessionId: qrSid,
              status: st,
              accountUid: s?.account_uid || '',
              expiresIn: Number(s?.expires_in ?? 0),
            },
            promoteToIncident: st === 'import_error',
          })
        }
        const expiresIn = Number(s?.expires_in ?? 0)
        if (st === 'waiting') setQrStatusText(expiresIn > 0 ? `请用闲鱼 APP 扫码登录（剩余 ${expiresIn} 秒）` : '请用闲鱼 APP 扫码登录')
        else if (st === 'scanned') {
          setQrScanned(true)
          setQrStatusText('已扫码，请在手机上确认…')
        }
        else if (st === 'expired') { setQrStatusText(s?.message || '二维码已过期，请重新生成'); setQrScanned(false); clearInterval(timer) }
        else if (st === 'cancelled') { setQrStatusText('已取消登录'); setQrScanned(false); clearInterval(timer) }
        else if (st === 'owner_conflict') {
          const uid = String(s?.account_uid || '')
          setQrConflict({
            accountUid: uid,
            message: s?.message || '该账号已绑定到其他用户，为防止该账号多端登录频繁请求引起不必要的风控，你可以提交解绑申请，管理员处理后再重新扫码添加。',
            canApplyUnbind: Boolean(s?.can_apply_unbind && uid),
          })
          setQrScanned(false)
          setQrStatusText(s?.message || '该账号已绑定到其他用户，为防止该账号多端登录频繁请求引起不必要的风控，你可以提交解绑申请，管理员处理后再重新扫码添加。')
          clearInterval(timer)
        }
        else if (st === 'import_error') { setQrStatusText(s?.message || '账号导入失败，请重新扫码'); setQrScanned(false); clearInterval(timer) }
        else if (st === 'success') {
          clearInterval(timer)
          setQrConflict(null)
          setQrScanned(true)
          setQrStatusText('已确认，正在更新账号列表…')
          window.setTimeout(async () => {
            if (cancelled || !isAuthCurrent(requestToken)) return
            const before = preAddUids ?? new Set<string>()
            const resolvedUid = String(s?.account_uid || '').trim()
            const accountLoadResult = await loadAccountsUntilQrAccountVisible({
              loadAccounts,
              beforeUids: before,
              resolvedUid,
              isCurrent: () => !cancelled && isAuthCurrent(requestToken),
            })
            if (cancelled || !isAuthCurrent(requestToken)) return
            if (!accountLoadResult) return

            const { matchedAccount } = accountLoadResult
            const syncUid = String(matchedAccount?.uid || resolvedUid || '').trim()
            const syncNick = matchedAccount?.nick || syncUid
            emitActionLog({
              module: 'accounts-qr',
              action: 'load_account_after_qr',
              outcome: matchedAccount ? 'success' : 'warn',
              message: matchedAccount ? '扫码成功后账号列表已出现新账号' : '扫码成功后账号列表未匹配到新账号',
              context: {
                sessionId: qrSid,
                resolvedUid,
                matchedUid: matchedAccount?.uid || '',
                accountCount: accountLoadResult.accounts?.length || 0,
              },
              promoteToIncident: false,
            })
            setQrStatusText('账号已添加，正在后台同步订单和商品…')

            emitAccountsSynced({
              uid: syncUid || undefined,
              source: 'accounts-qr-success',
            })

            if (syncUid) {
              void bootstrapAccountData(syncUid, syncNick, requestToken)
            }
            window.setTimeout(() => {
              if (cancelled || !isAuthCurrent(requestToken)) return
              setShowQr(false)
              setQrScanned(false)
              if (matchedAccount && !before.has(matchedAccount.uid)) {
                setLocationModal({ uid: matchedAccount.uid, nick: matchedAccount.nick })
              }
              setPreAddUids(null)
            }, 700)
          }, 800)
        } else if (st === 'risk_control') {
          setQrStatusText('账号风控，需要人工验证')
          setQrScanned(false)
          clearInterval(timer)
        }
      } catch (e: any) {
        const message = e?.message || ''
        if (message.includes('绑定到其他用户')) {
          setQrStatusText(message)
          emitActionFailure({
            module: 'accounts-qr',
            action: 'poll_qr_status',
            message: '扫码状态轮询发现账号绑定冲突',
            error: e,
            context: { sessionId: qrSid },
            promoteToIncident: false,
          })
          clearInterval(timer)
        }
        // 其他错误多为网络抖动或 session 被清理，继续等下一轮或由用户手动关闭。
      }
    }, 1500)
    return () => { cancelled = true; clearInterval(timer) }
  }, [showQr, qrSid, authToken])

  async function handleApplyAccountUnbind() {
    const accountUid = String(qrConflict?.accountUid || '').trim()
    if (!accountUid || unbindApplying) return
    const requestToken = getAuthToken()
    setUnbindApplying(true)
    emitActionStarted({
      module: 'accounts-qr',
      action: 'request_account_unbind',
      message: '开始提交账号解绑申请',
      context: { accountUid },
    })
    try {
      const result: any = await createAccountUnbindRequest({
        account_uid: accountUid,
        reason: '扫码导入时发现账号已绑定到其他用户，请求管理员解绑',
      })
      if (!isAuthCurrent(requestToken)) return
      emitActionSuccess({
        module: 'accounts-qr',
        action: 'request_account_unbind',
        message: '账号解绑申请提交成功',
        context: { accountUid, message: result?.message || '' },
      })
      notify(result?.message || '解绑申请已提交，管理员处理后可重新扫码添加', 'success', 3600)
    } catch (e: any) {
      emitActionFailure({
        module: 'accounts-qr',
        action: 'request_account_unbind',
        message: '账号解绑申请提交失败',
        error: e,
        context: { accountUid },
      })
      if (isAuthCurrent(requestToken)) notify(e?.message || '提交解绑申请失败', 'error', 3600)
    } finally {
      if (isAuthCurrent(requestToken)) setUnbindApplying(false)
    }
  }

  function toggleExpand(uid: string) {
    setExpandedUid((cur) => (cur === uid ? null : uid))
  }

  async function handleRefreshAccount(uid: string, nick?: string) {
    if (savingMap[uid]) return
    const requestToken = getAuthToken()
    setSavingMap((prev) => ({ ...prev, [uid]: true }))
    try {
      const r: any = await refreshAccount(uid)
      if (!isAuthCurrent(requestToken)) return
      await loadAccounts()
      emitAccountsSynced({ uid, source: 'accounts-refresh-one' })
      const probe = r?.probe
      if (probe && probe.ok === false) {
        notify(`${nick || uid} 刷新完成，但会话仍不可用：${probe?.ret_code || '校验失败'}`, 'error', 4200)
        return
      }
      notify(`${nick || uid} 刷新完成`, 'success', 2600)
    } catch (e: any) {
      if (isAuthCurrent(requestToken)) notify(`刷新失败：${e?.message || e}`, 'error', 3600)
    } finally {
      if (isAuthCurrent(requestToken)) setSavingMap((prev) => ({ ...prev, [uid]: false }))
    }
  }

  async function handleDeleteAccount(uid: string, nick?: string) {
    if (savingMap[uid]) return
    const requestToken = getAuthToken()
    const target = nick || uid
    const confirmed = await showConfirm(
      `确认删除账号「${target}」？这会同时移除本地消息、通知绑定和会话缓存。`,
      { title: '删除账号', confirmLabel: '删除', danger: true, width: 460 },
    )
    if (!confirmed) {
      return
    }
    setSavingMap((prev) => ({ ...prev, [uid]: true }))
    emitActionStarted({
      module: 'accounts',
      action: 'delete_account',
      message: '开始删除账号',
      context: { uid, hasNick: Boolean(nick) },
    })
    try {
      await deleteAccount(uid)
      if (!isAuthCurrent(requestToken)) return
      await loadAccounts({ force: true })
      emitAccountsSynced({ uid, source: 'accounts-delete' })
      if (expandedUid === uid) setExpandedUid(null)
      emitActionSuccess({
        module: 'accounts',
        action: 'delete_account',
        message: '账号删除成功',
        context: { uid, hasNick: Boolean(nick) },
      })
      notify(`已删除账号：${target}`, 'success', 2600)
    } catch (e: any) {
      emitActionFailure({
        module: 'accounts',
        action: 'delete_account',
        message: '账号删除失败',
        error: e,
        context: { uid, hasNick: Boolean(nick) },
      })
      if (isAuthCurrent(requestToken)) notify(`删除失败：${e?.message || e}`, 'error', 3600)
    } finally {
      if (isAuthCurrent(requestToken)) setSavingMap((prev) => ({ ...prev, [uid]: false }))
    }
  }

  async function handleToggle(uid: string, key: string, value: boolean) {
    const requestToken = getAuthToken()
    setSavingMap((prev) => ({ ...prev, [uid]: true }))
    try {
      await updateAccountSetting(uid, { [key]: value })
      if (!isAuthCurrent(requestToken)) return
      await loadAccounts({ force: true })
      emitAccountsSynced({ uid, source: `accounts-toggle-${key}` })
    } catch (e: any) { if (isAuthCurrent(requestToken)) notify(e.message || '设置更新失败', 'error', 3600) }
    finally { if (isAuthCurrent(requestToken)) setSavingMap((prev) => ({ ...prev, [uid]: false })) }
  }

  function openLocationModal(uid: string, nick?: string) {
    setLocationModal({ uid, nick })
  }

  function closeLocationModal() {
    setLocationModal(null)
  }

  return {
    // data
    accounts,
    sortedAccounts,
    profiles,
    kpi,
    enabledAccountCount,
    isAdmin,
    loadAccounts,
    // QR / add account
    showQr,
    qrUrl,
    qrSid,
    qrStatusText,
    qrScanned,
    qrConflict,
    unbindApplying,
    handleAddAccount,
    handleReloginAccount,
    handleApplyAccountUnbind,
    refreshQr,
    closeQr,
    // batch
    refreshingAll,
    handleRefreshAll,
    // inline expand (desktop only)
    expandedUid,
    toggleExpand,
    handleRefreshAccount,
    handleDeleteAccount,
    // toggles
    savingMap,
    handleToggle,
    // publish-location modal
    locationModal,
    openLocationModal,
    closeLocationModal,
  }
}
