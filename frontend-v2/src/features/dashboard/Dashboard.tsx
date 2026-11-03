import { useCallback, useEffect, useRef, useState } from 'react'
import { Navigate } from 'react-router-dom'
import type { DataCenterAccountRow, DataCenterStats } from '../../api/types'
import { getDataCenterStats } from '../../api/index'
import { getAuthToken } from '../../api/client'
import { useDevice } from '../../hooks/useDevice'
import { useAuthScope } from '../../hooks/useAuthScope'
import { createLiveTransport } from '../messages/useLiveTransport'
import {
  listenBatchedSyncEvent,
  XIANYU_ACCOUNTS_SYNCED_EVENT,
  XIANYU_ITEMS_SYNCED_EVENT,
  XIANYU_MESSAGES_SYNCED_EVENT,
  XIANYU_ORDERS_SYNCED_EVENT,
} from '../runtimeEvents'
import { DashboardPresentation } from './DashboardPresentation'

export default function Dashboard() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <Navigate to="/overview" replace />
  return <DesktopDashboard />
}

function DesktopDashboard() {
  const [stats, setStats] = useState<DataCenterStats | null>(null)
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState('')
  const [trendUid, setTrendUid] = useState('')
  const [loadedTrendUid, setLoadedTrendUid] = useState<string | null>(null)
  const refreshInFlightRef = useRef(false)
  const liveTransportRef = useRef<ReturnType<typeof createLiveTransport> | null>(null)
  const accountUidsRef = useRef<string[]>([])
  const subscribedUidsKeyRef = useRef('')
  const pendingLiveEventsRef = useRef<Map<string, AccountConnectionEvent>>(new Map())
  const liveFlushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const { authToken } = useAuthScope(() => {
    setStats(null); setLoadError(''); setLoading(false); setTrendUid(''); setLoadedTrendUid(null)
    accountUidsRef.current = []; subscribedUidsKeyRef.current = ''; pendingLiveEventsRef.current.clear()
  })

  useEffect(() => {
    const previousOverflow = document.body.style.overflow
    window.scrollTo({ top: 0, left: 0 }); document.body.style.overflow = 'hidden'
    return () => { document.body.style.overflow = previousOverflow }
  }, [])

  const refresh = useCallback(async (options: { silent?: boolean; refresh?: boolean } = {}) => {
    if (refreshInFlightRef.current) return
    refreshInFlightRef.current = true
    const requestToken = getAuthToken()
    const visible = !options.silent
    if (visible) setLoading(true)
    setLoadError('')
    try {
      const requestedTrendUid = trendUid
      const next = await getDataCenterStats({ trendUid: requestedTrendUid || undefined, refresh: options.refresh })
      if (getAuthToken() === requestToken) {
        setStats(next)
        setLoadedTrendUid(requestedTrendUid)
      }
    } catch (error) {
      if (getAuthToken() !== requestToken || options.silent) return
      const message = error instanceof Error ? error.message : '数据中心加载失败'
      setLoadError(message)
    } finally {
      refreshInFlightRef.current = false
      if (visible && getAuthToken() === requestToken) setLoading(false)
    }
  }, [trendUid])

  const queueAccountLiveEvent = useCallback((event: unknown) => {
    if (!isAccountConnectionEvent(event)) return
    pendingLiveEventsRef.current.set(event.uid, event)
    if (liveFlushTimerRef.current) return
    liveFlushTimerRef.current = setTimeout(() => {
      liveFlushTimerRef.current = null
      const events = [...pendingLiveEventsRef.current.values()]
      pendingLiveEventsRef.current.clear()
      if (events.length) setStats((current) => current ? applyAccountLiveEvents(current, events) : current)
    }, 80)
  }, [])

  // The authenticated scope change is the source event for the initial aggregate read.
  // eslint-disable-next-line react-hooks/set-state-in-effect
  useEffect(() => { void refresh() }, [refresh, authToken])

  useEffect(() => {
    const pendingEvents = pendingLiveEventsRef.current
    const transport = createLiveTransport({
      authToken,
      onEvent: (event: unknown) => queueAccountLiveEvent(event),
      onStateChange: (state: string) => { if (state === 'ws' && accountUidsRef.current.length) transport.subscribe(accountUidsRef.current) },
    })
    liveTransportRef.current = transport
    transport.connect()
    return () => {
      if (liveFlushTimerRef.current) clearTimeout(liveFlushTimerRef.current)
      liveFlushTimerRef.current = null; pendingEvents.clear(); transport.close()
      if (liveTransportRef.current === transport) liveTransportRef.current = null
    }
  }, [authToken, queueAccountLiveEvent])

  useEffect(() => {
    const uids = stats?.accounts.map((account) => account.uid).filter(Boolean) || []
    accountUidsRef.current = uids
    const key = [...uids].sort().join('|')
    if (key && key !== subscribedUidsKeyRef.current) {
      subscribedUidsKeyRef.current = key
      liveTransportRef.current?.subscribe(uids)
    }
  }, [stats?.accounts])

  useEffect(() => {
    const events = [XIANYU_ACCOUNTS_SYNCED_EVENT, XIANYU_ITEMS_SYNCED_EVENT, XIANYU_ORDERS_SYNCED_EVENT, XIANYU_MESSAGES_SYNCED_EVENT]
    const unsubscribers = events.map((name) => listenBatchedSyncEvent(name, 350, () => { void refresh({ silent: true }) }))
    return () => unsubscribers.forEach((unsubscribe) => unsubscribe())
  }, [refresh])

  return <DashboardPresentation
    stats={stats}
    loading={loading}
    loadError={loadError}
    scopeUid={trendUid || 'all'}
    trendReady={loadedTrendUid === trendUid}
    onScopeChange={(uid) => setTrendUid(uid === 'all' ? '' : uid)}
    onRefresh={() => { void refresh({ refresh: true }) }}
  />
}

type AccountConnectionEvent = { type: string; uid: string; reason?: string }
function isAccountConnectionEvent(event: unknown): event is AccountConnectionEvent {
  if (!event || typeof event !== 'object') return false
  const candidate = event as Record<string, unknown>
  return String(candidate.type || '').startsWith('connection.') && typeof candidate.uid === 'string' && candidate.uid.trim().length > 0
}
function applyAccountLiveEvents(current: DataCenterStats, events: AccountConnectionEvent[]) {
  const byUid = new Map(events.map((event) => [event.uid, event]))
  let changed = false
  const accounts = current.accounts.map((account) => {
    const event = byUid.get(account.uid)
    if (!event) return account
    const liveStatus = event.type.replace('connection.', '')
    const lastError = String(event.reason || account.lastError || '')
    if (account.liveStatus === liveStatus && account.lastError === lastError) return account
    changed = true
    return { ...account, liveStatus, lastError }
  })
  if (!changed) return current
  const liveOnline = accounts.filter((account: DataCenterAccountRow) => account.liveStatus === 'connected').length
  return { ...current, accounts, totals: { ...current.totals, liveOnlineAccounts: liveOnline }, sync: { ...current.sync, liveOnline, liveTotal: current.sync.liveTotal || accounts.length } }
}
