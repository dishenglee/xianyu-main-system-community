// @ts-nocheck
import { useEffect, useRef, useState, useCallback, useMemo } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { MessageSquare, X } from 'lucide-react'
import { pcSessions } from '../api'
import { getAuthToken } from '../api/client'
import { useAuthStore } from '../store/authStore'
import { selectUserUnreadTotal, useUserUnreadStore } from '../store/userUnreadStore'
import { isTauriRuntime } from '../tauriRuntime'
import {
  emitMessagesSynced,
  emitOrdersSynced,
  listenSyncEvent,
  XIANYU_ACCOUNTS_SYNCED_EVENT,
} from '../features/runtimeEvents'
import { filterMessageCapableAccounts } from '../features/messages/messageAccounts'
import { useAccounts } from '../hooks/useAccounts'
import { useLiveEvents } from '../features/messages/useLiveEvents'
import { emitActionLog } from '../bootstrap/actionLogging'
import {
  applyLiveEventSnapshot,
  pruneSnapshotToUids,
  reconcilePolledSessions,
  type GlobalMessageSnapshotEntry,
} from './globalMessageNotifyPolling'

interface NotifyItem {
  id: number
  uid: string
  accountNick: string
  accountAvatar: string
  sessionId: string
  sessionType: number
  peerNick: string
  preview: string
  createdAt: number
}

const MAX_VISIBLE = 3
const DISMISS_SEC = 15
const MESSAGE_PAGE_PATH = '/messages'
const POLL_SESSION_LIMIT = 200
const POLL_CONCURRENCY = 4
const STARTUP_SESSION_REFRESH_DELAY_MS = 90_000

let idCounter = 0

interface GlobalMessageNotifyProps {
  startupSessionRefresh?: 'immediate' | 'cached-first' | 'deferred'
}

function isMessagesPath(pathname: string) {
  return pathname === MESSAGE_PAGE_PATH || pathname.startsWith(`${MESSAGE_PAGE_PATH}/`)
}

async function ensureSystemNotificationPermission() {
  if (typeof window === 'undefined') return false
  if (isTauriRuntime()) {
    try {
      const { isPermissionGranted, requestPermission } = await import('@tauri-apps/plugin-notification')
      if (await isPermissionGranted()) return true
      return await requestPermission() === 'granted'
    } catch {
      return false
    }
  }
  if (typeof Notification === 'undefined') return false
  if (Notification.permission === 'granted') return true
  if (Notification.permission !== 'default') return false
  try {
    return await Notification.requestPermission() === 'granted'
  } catch {
    return false
  }
}

async function setNativeUnreadBadge(count: number) {
  if (!isTauriRuntime()) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('set_app_badge_count', { count: Math.max(0, Math.trunc(Number(count) || 0)) })
  } catch {
    // Badge support varies by platform; unread state in the app remains canonical.
  }
}

async function requestNativeAttention() {
  if (!isTauriRuntime()) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('request_app_attention')
  } catch {
    // Taskbar attention is best-effort; the system notification remains primary.
  }
}

function tauriNotificationSound() {
  const platform = typeof navigator === 'undefined' ? '' : `${navigator.platform} ${navigator.userAgent}`
  return /Mac/i.test(platform) ? 'Ping' : undefined
}

function logMessageNotification(
  outcome: 'success' | 'warn' | 'failed',
  message: string,
  detail = '',
  context: Record<string, unknown> = {},
) {
  emitActionLog({
    module: 'message-notification',
    action: 'show_message_notification',
    outcome,
    message,
    detail,
    errorCode: `message_notification_${outcome}`,
    retryable: outcome !== 'success',
    promoteToIncident: outcome === 'failed',
    context,
  })
}

function showSystemMessageNotification(title: string, body: string) {
  if (typeof window === 'undefined') return
  if (!getAuthToken()) return
  if (isTauriRuntime()) {
    void (async () => {
      try {
        if (!getAuthToken()) return
        if (!(await ensureSystemNotificationPermission())) {
          logMessageNotification('warn', '系统通知权限未开启，已显示 App 内新消息提示', '', {
            runtime: 'tauri',
          })
          return
        }
        if (!getAuthToken()) return
        const { sendNotification } = await import('@tauri-apps/plugin-notification')
        const sound = tauriNotificationSound()
        sendNotification({
          title,
          body,
          ...(sound ? { sound } : {}),
          group: 'xianyu-message',
          autoCancel: true,
          silent: false,
        })
        logMessageNotification('success', '系统新消息通知已发送', '', {
          runtime: 'tauri',
          title,
        })
        void requestNativeAttention()
      } catch (error) {
        logMessageNotification(
          'failed',
          '系统新消息通知发送失败，已显示 App 内新消息提示',
          error instanceof Error ? error.message : String(error || ''),
          { runtime: 'tauri', title },
        )
      }
    })()
    return
  }
  if (typeof Notification === 'undefined') {
    logMessageNotification('warn', '当前环境不支持系统通知，已显示 App 内新消息提示', '', {
      runtime: 'web',
    })
    return
  }
  const send = () => {
    try {
      if (!getAuthToken()) return
      const notification = new Notification(title, {
        body,
        tag: 'xianyu-message',
        silent: false,
      })
      notification.onclick = () => {
        window.focus()
        notification.close()
      }
      logMessageNotification('success', '系统新消息通知已发送', '', {
        runtime: 'web',
        title,
      })
    } catch (error) {
      logMessageNotification(
        'failed',
        '系统新消息通知发送失败，已显示 App 内新消息提示',
        error instanceof Error ? error.message : String(error || ''),
        { runtime: 'web', title },
      )
    }
  }

  if (Notification.permission === 'granted') {
    send()
    return
  }
  if (Notification.permission === 'default') {
    void Notification.requestPermission().then((permission) => {
      if (permission === 'granted') send()
      else {
        logMessageNotification('warn', '系统通知权限未开启，已显示 App 内新消息提示', '', {
          runtime: 'web',
          permission,
        })
      }
    }).catch(() => {})
    return
  }
  logMessageNotification('warn', '系统通知权限未开启，已显示 App 内新消息提示', '', {
    runtime: 'web',
    permission: Notification.permission,
  })
}

function CountdownBar({ createdAt }: { createdAt: number }) {
  const [remaining, setRemaining] = useState(DISMISS_SEC)

  useEffect(() => {
    const timer = setInterval(() => {
      const elapsed = (Date.now() - createdAt) / 1000
      const left = Math.max(0, DISMISS_SEC - elapsed)
      setRemaining(left)
      if (left <= 0) clearInterval(timer)
    }, 200)
    return () => clearInterval(timer)
  }, [createdAt])

  const pct = (remaining / DISMISS_SEC) * 100

  return (
    <div className="mt-2 flex items-center gap-2">
      <div className="flex-1 h-[3px] rounded-full overflow-hidden" style={{ backgroundColor: 'var(--surface-sunken)' }}>
        <div
          className="h-full rounded-full transition-all"
          style={{ width: `${pct}%`, backgroundColor: 'var(--primary)', transition: 'width 0.2s linear' }}
        />
      </div>
      <span className="text-[10px] tabular-nums shrink-0" style={{ color: 'var(--text-muted)' }}>{Math.ceil(remaining)}s</span>
    </div>
  )
}

export function GlobalMessageNotify({ startupSessionRefresh = 'immediate' }: GlobalMessageNotifyProps = {}) {
  const navigate = useNavigate()
  const location = useLocation()
  const { accounts, loadAccounts } = useAccounts()
  const [items, setItems] = useState<NotifyItem[]>([])
  const [connectionStates, setConnectionStates] = useState<Record<string, unknown>>({})
  const storeAuthToken = useAuthStore((state) => state.token)
  const unreadTotal = useUserUnreadStore(selectUserUnreadTotal)
  const authToken = storeAuthToken || getAuthToken()
  const timersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map())
  const locationRef = useRef(location.pathname)
  locationRef.current = location.pathname
  const accountMapRef = useRef<Record<string, { nick: string; avatar: string }>>({})
  const pollSnapshotRef = useRef<Record<string, GlobalMessageSnapshotEntry>>({})
  const activeAccountKeyRef = useRef('')
  const lastSeededAccountKeyRef = useRef('')
  const pollGenerationRef = useRef(0)
  const pollingRef = useRef(false)
  const deferredStartupTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    if (!authToken) return
    void loadAccounts()
  }, [authToken, loadAccounts])

  useEffect(() => {
    if (!authToken) return
    void ensureSystemNotificationPermission()
  }, [authToken])

  useEffect(() => {
    void setNativeUnreadBadge(unreadTotal)
  }, [unreadTotal])

  const activeAccounts = useMemo(() => filterMessageCapableAccounts(accounts), [accounts])
  const activeAccountUids = useMemo(
    () => activeAccounts.map((account: any) => account.uid),
    [activeAccounts],
  )
  const activeAccountKey = activeAccountUids.join('|')

  useEffect(() => {
    activeAccountKeyRef.current = activeAccountKey
    if (!authToken) {
      accountMapRef.current = {}
      pollSnapshotRef.current = {}
      lastSeededAccountKeyRef.current = ''
      setConnectionStates({})
      for (const timer of timersRef.current.values()) clearTimeout(timer)
      timersRef.current.clear()
      setItems([])
      useUserUnreadStore.getState().reset()
      return
    }

    const map: Record<string, { nick: string; avatar: string }> = {}
    for (const account of Array.isArray(accounts) ? accounts : []) {
      if (account?.uid) {
        map[account.uid] = {
          nick: account.nick || account.uid,
          avatar: account.avatar || '',
        }
      }
    }
    accountMapRef.current = map
    pollSnapshotRef.current = pruneSnapshotToUids(pollSnapshotRef.current, activeAccountUids)
    useUserUnreadStore.getState().pruneToUids(activeAccountUids)
  }, [accounts, activeAccountKey, activeAccountUids, authToken])

  useEffect(() => {
    if (!authToken) return
    return listenSyncEvent(XIANYU_ACCOUNTS_SYNCED_EVENT, () => {
      void loadAccounts({ force: true })
    })
  }, [authToken, loadAccounts])

  const dismiss = useCallback((id: number) => {
    const timer = timersRef.current.get(id)
    if (timer) {
      clearTimeout(timer)
      timersRef.current.delete(id)
    }
    setItems((prev) => prev.filter((n) => n.id !== id))
  }, [])

  const handleClick = useCallback((item: NotifyItem) => {
    dismiss(item.id)
    const key = `${item.uid}:${item.sessionId}:${item.sessionType}`
    window.dispatchEvent(new CustomEvent('xy:openSession', { detail: { key, uid: item.uid, sessionId: item.sessionId, sessionType: item.sessionType } }))
    if (!isMessagesPath(location.pathname)) {
      navigate(MESSAGE_PAGE_PATH)
    }
  }, [dismiss, navigate, location.pathname])

  const enqueueNotification = useCallback((entry: {
    uid: string
    sessionId: string
    sessionType: number
    peerNick: string
    preview: string
  }) => {
    if (!getAuthToken()) return
    const id = ++idCounter
    const account = accountMapRef.current[entry.uid]
    const item: NotifyItem = {
      id,
      uid: entry.uid,
      accountNick: account?.nick || entry.uid || '未知账号',
      accountAvatar: account?.avatar || '',
      sessionId: entry.sessionId,
      sessionType: entry.sessionType,
      peerNick: entry.peerNick || '买家',
      preview: String(entry.preview || '').slice(0, 60),
      createdAt: Date.now(),
    }
    logMessageNotification('success', 'App 内新消息提示已显示', '', {
      uid: item.uid,
      sessionId: item.sessionId,
      route: locationRef.current,
      onMessagesPage: isMessagesPath(locationRef.current),
    })
    setItems((prev) => [...prev.slice(-(MAX_VISIBLE - 1)), item])
    showSystemMessageNotification(
      `${item.peerNick || '客户'} · ${item.accountNick}`,
      item.preview || '收到一条新消息',
    )
    const timer = setTimeout(() => dismiss(id), DISMISS_SEC * 1000)
    timersRef.current.set(id, timer)
  }, [dismiss])

  const pollAccountSessions = useCallback(async (
    uid: string,
    requestToken: string,
    generation: number,
    notifyOnNewActivity: boolean,
    source: 'cache' | 'upstream',
  ) => {
    const response = await pcSessions(uid, '[1,3,6]', POLL_SESSION_LIMIT, {
      force: source === 'upstream',
      cached: source === 'cache',
    })
    if (getAuthToken() !== requestToken) return
    if (generation !== pollGenerationRef.current) return
    if (activeAccountKeyRef.current !== activeAccountKey) return

    const sessions = Array.isArray(response?.sessions) ? response.sessions : []
    useUserUnreadStore.getState().setSessionsForUid(uid, sessions)
    const { snapshot, notifications } = reconcilePolledSessions(
      pollSnapshotRef.current,
      uid,
      sessions,
      { notifyOnNewKeys: notifyOnNewActivity },
    )
    pollSnapshotRef.current = snapshot
    emitMessagesSynced({
      uid,
      total: sessions.reduce((sum: number, session: any) => {
        if (Number(session?.sessionType ?? 1) !== 1) return sum
        return sum + Math.max(0, Number(session?.unread || 0) || 0)
      }, 0),
      source: 'global-message-poll',
    })
    for (const entry of notifications) {
      enqueueNotification(entry)
    }
  }, [activeAccountKey, enqueueNotification])

  const refreshUnreadFromSessions = useCallback(async (
    {
      notifyOnNewActivity,
      source = 'upstream',
    }: {
      notifyOnNewActivity: boolean
      source?: 'cache' | 'upstream'
    },
  ) => {
    if (!authToken) return
    if (!activeAccountUids.length) {
      useUserUnreadStore.getState().reset()
      pollSnapshotRef.current = {}
      return
    }
    if (pollingRef.current) return

    pollingRef.current = true
    pollGenerationRef.current += 1
    const generation = pollGenerationRef.current
    const requestToken = getAuthToken()
    const queue = [...activeAccountUids]

    try {
      const workers = Array.from(
        { length: Math.min(POLL_CONCURRENCY, queue.length) },
        async () => {
          while (queue.length > 0) {
            const uid = queue.shift()
            if (!uid) continue
            try {
              await pollAccountSessions(uid, requestToken, generation, notifyOnNewActivity, source)
            } catch {
              // Global polling is best-effort; the next round or WS push will recover.
            }
          }
        },
      )
      await Promise.all(workers)
      if (getAuthToken() === requestToken && generation === pollGenerationRef.current) {
        useUserUnreadStore.getState().pruneToUids(activeAccountUids)
      }
    } finally {
      if (generation === pollGenerationRef.current) {
        pollingRef.current = false
      }
    }
  }, [activeAccountUids, authToken, pollAccountSessions])

  const handleFallbackRefresh = useCallback(({ background }: { background: boolean }) => {
    if (!background) return
    void refreshUnreadFromSessions({ notifyOnNewActivity: true })
  }, [refreshUnreadFromSessions])

  const handleLiveEvent = useCallback((data: any) => {
    const type = String(data?.type || '').trim()
    if (!type || type === 'pong') return

    if (type.startsWith('connection.')) {
      const uid = String(data?.uid || '').trim()
      if (!uid) return
      setConnectionStates((prev) => ({
        ...prev,
        [uid]: {
          status: type.replace('connection.', ''),
          reason: String(data?.reason || '').trim(),
          snapshot: Boolean(data?.snapshot),
          updatedAt: Number(data?.updatedAt || 0) || Date.now(),
        },
      }))
      return
    }

    if (type === 'orders.synced') {
      emitOrdersSynced({
        uid: String(data.uid || ''),
        total: Number(data.total || 0),
        source: 'live-orders',
      })
      return
    }

    if (type !== 'message.received' || data?.isSelf) return

    const eventTs = Number(data?.ts) || 0
    const freshWindowMs = 5 * 60 * 1000
    if (eventTs > 0 && Date.now() - eventTs > freshWindowMs) {
      return
    }

    const uid = String(data?.uid || '').trim()
    const sessionId = String(data?.sessionId || '').trim()
    const sessionType = Number(data?.sessionType || 1) || 1
    if (!uid || !sessionId || sessionType !== 1) return

    useUserUnreadStore.getState().incrementUnread(
      uid,
      sessionId,
      sessionType,
      Number(data?.version) || 0,
    )
    pollSnapshotRef.current = applyLiveEventSnapshot(pollSnapshotRef.current, data)
    emitMessagesSynced({
      uid,
      total: Number(data?.unreadCount || 0),
      source: 'live-message',
    })
    enqueueNotification({
      uid,
      sessionId,
      sessionType,
      peerNick: String(data?.peerNick || data?.senderNick || '').trim(),
      preview: String(data?.rawPreview || data?.text || '').trim(),
    })
  }, [enqueueNotification])

  const { subscribe: liveSubscribe } = useLiveEvents({
    activeUids: activeAccountUids,
    connectionStates,
    onEvent: handleLiveEvent,
    onFallbackRefresh: handleFallbackRefresh,
  })

  useEffect(() => {
    if (activeAccountUids.length > 0) {
      liveSubscribe(activeAccountUids)
    }
  }, [activeAccountKey, activeAccountUids, liveSubscribe])

  useEffect(() => {
    if (!authToken) return
    if (!activeAccountUids.length) {
      useUserUnreadStore.getState().reset()
      pollSnapshotRef.current = {}
      lastSeededAccountKeyRef.current = ''
      if (deferredStartupTimerRef.current) {
        clearTimeout(deferredStartupTimerRef.current)
        deferredStartupTimerRef.current = null
      }
      return
    }

    const suppressNotifications = lastSeededAccountKeyRef.current !== activeAccountKey
    lastSeededAccountKeyRef.current = activeAccountKey
    if (startupSessionRefresh === 'deferred') {
      if (deferredStartupTimerRef.current) {
        clearTimeout(deferredStartupTimerRef.current)
      }
      deferredStartupTimerRef.current = setTimeout(() => {
        deferredStartupTimerRef.current = null
        void refreshUnreadFromSessions({ notifyOnNewActivity: false, source: 'upstream' })
      }, STARTUP_SESSION_REFRESH_DELAY_MS)
      return () => {
        if (deferredStartupTimerRef.current) {
          clearTimeout(deferredStartupTimerRef.current)
          deferredStartupTimerRef.current = null
        }
      }
    }
    if (startupSessionRefresh === 'cached-first') {
      void refreshUnreadFromSessions({ notifyOnNewActivity: false, source: 'cache' })
      if (deferredStartupTimerRef.current) {
        clearTimeout(deferredStartupTimerRef.current)
      }
      deferredStartupTimerRef.current = setTimeout(() => {
        deferredStartupTimerRef.current = null
        void refreshUnreadFromSessions({ notifyOnNewActivity: false, source: 'upstream' })
      }, STARTUP_SESSION_REFRESH_DELAY_MS)
      return () => {
        if (deferredStartupTimerRef.current) {
          clearTimeout(deferredStartupTimerRef.current)
          deferredStartupTimerRef.current = null
        }
      }
    }
    void refreshUnreadFromSessions({ notifyOnNewActivity: !suppressNotifications, source: 'upstream' })
  }, [authToken, activeAccountKey, activeAccountUids, refreshUnreadFromSessions, startupSessionRefresh])

  useEffect(() => {
    return () => {
      if (deferredStartupTimerRef.current) clearTimeout(deferredStartupTimerRef.current)
      for (const timer of timersRef.current.values()) clearTimeout(timer)
      timersRef.current.clear()
    }
  }, [])

  if (!items.length) return null

  return (
    <div className="global-message-notify fixed z-[200] flex flex-col gap-2 pointer-events-none">
      {items.map((item) => (
        <div
          key={item.id}
          onClick={() => handleClick(item)}
          className="glass-panel pointer-events-auto flex flex-col rounded-2xl px-4 py-3 cursor-pointer transition-all hover:scale-[1.01]"
          style={{
            animation: 'slideIn 0.3s ease-out',
          }}
        >
          <div className="flex items-start gap-3">
            {item.accountAvatar ? (
              <img src={item.accountAvatar} alt="" className="mt-0.5 h-9 w-9 shrink-0 rounded-full object-cover" />
            ) : (
              <div
                className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-full"
                style={{ backgroundColor: 'var(--surface-sunken)', color: 'var(--primary)' }}
              >
                <MessageSquare className="h-4 w-4" />
              </div>
            )}

            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-primary)' }}>
                  {item.accountNick}
                </span>
                <span className="truncate text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  {item.peerNick}
                </span>
              </div>
              <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                {item.preview || '收到一条新消息'}
              </div>
              <CountdownBar createdAt={item.createdAt} />
            </div>

            <button
              type="button"
              onClick={(event) => {
                event.stopPropagation()
                dismiss(item.id)
              }}
              className="mt-0.5 rounded-full p-1 transition-colors"
              style={{ color: 'var(--text-muted)' }}
              aria-label="关闭通知"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>
      ))}
    </div>
  )
}
