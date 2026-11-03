// @ts-nocheck
// Shared business state + handlers for the messages page.
// Both AggregateMessages (desktop) and MobileMessages (mobile list) consume
// this hook so all the session/history/live-event/unread-store wiring lives
// in one place. The two pages only differ in how they render the resulting
// state.
//
// Returned shape (grouped):
//   state:
//     accounts, activeAccounts, activeAccountUids,
//     sessionsByUid, allSessions, refreshing,
//     historyByKey, historyLoadingByKey,
//     selectedAccountUid, selectedSessionId,
//     composerText, sending,
//     connectionStates, itemPriceByItemId, headinfoBySessionId,
//     detailScrollRef, imageInputRef,
//   computed:
//     filteredSessions, pinnedSessions, chatSessions, groupedSessions,
//     selectedSession, selectedHistory, selectedMessages, selectedItem,
//     selectedLoadingState, totalUnread, isPinnedNotificationType,
//   setters:
//     setSelectedAccountUid, setSelectedSessionId, setComposerText,
//   actions:
//     loadAllSessions({ background, force }), loadHistory(session, { background }),
//     selectSession(session), recallMessage(message), markSessionRead(session),
//     handleSend(), handleImagePick(event),
//
// Desktop-only state (itemPrice / headInfo for the conversation panel) is
// surfaced here because the page-level shell owns it. Mobile's separate
// MobileChat route resolves the same fields locally — see its own component.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

import {
  getMessageNotificationSettings,
  liveMarkSessionRead,
  pcMarkSessionRead,
  pcMessageHistory,
  pcSessions,
} from '../../../api/index'
import { getAuthToken } from '../../../api/client'
import { useNotify } from '../../../components/Notify'
import { useAccounts } from '../../../hooks/useAccounts'
import { useAuthScope } from '../../../hooks/useAuthScope'
import { useUserUnreadStore } from '../../../store/userUnreadStore'
import {
  groupSessionsByBucket,
  isPlatformNotificationSession,
  isSessionTypeEnabled,
} from '../aggregateMessagesModel'
import { mergeHistoryState } from '../historyState'
import { loadCompleteHistory } from '../loadHistory'
import {
  resolveSessionItem,
  sessionKey,
  sessionType,
} from '../messagesConstants'
import { dedupeWorkbenchSessions } from '../sessionLayout'
import { mergeSessionLists } from '../sessionMerge'
import { normalizeSessionTimestamp } from '../sessionState'
import { useLiveEvents } from '../useLiveEvents'
import { filterMessageCapableAccounts } from '../messageAccounts'
import { createLiveEventHandler } from './messagesLiveHandler'
import { createSendActions } from './messagesSendActions'
import { useSelectedSessionResolvers } from './useSelectedSessionResolvers'

// Platform "virtual channel" sessions (sessionType 3/6/53) are pinned at the
// top in single-account view and hidden in the "全部账号" aggregate (where
// they'd repeat once per account). Keep the predicate in one place so every
// consumer agrees.
function isPinnedNotificationType(st: number) {
  return st === 3 || st === 6 || st === 53
}

// Desktop's session-type filter — Mobile renders all kinds (chat/system/
// interaction), so we hard-code the desktop default rather than expose it
// as state. If we ever need a per-page filter UI it gets lifted back out.
const TYPE_FILTERS = { chat: true, system: true, favorite: true }

export function useMessagesPageState() {
  const notify = useNotify()
  const { accounts, loadAccounts } = useAccounts()

  const [refreshing, setRefreshing] = useState(false)
  const [sessionsByUid, setSessionsByUid] = useState<Record<string, any[]>>({})
  const [selectedAccountUid, setSelectedAccountUid] = useState('all')
  const [selectedSessionId, setSelectedSessionId] = useState('')
  const [historyByKey, setHistoryByKey] = useState<Record<string, any>>({})
  const [historyLoadingByKey, setHistoryLoadingByKey] = useState<Record<string, any>>({})
  // Sentinel '' = resolved-empty so we don't refire. undefined = not fetched yet.
  const [itemPriceByItemId, setItemPriceByItemId] = useState<Record<string, string>>({})
  const [headinfoBySessionId, setHeadinfoBySessionId] = useState<Record<string, any>>({})
  const [composerText, setComposerText] = useState('')
  const [sending, setSending] = useState(false)
  const [connectionStates, setConnectionStates] = useState<Record<string, unknown>>({})
  const [platformMessagesEnabled, setPlatformMessagesEnabled] = useState(true)
  const [detailScrollRequest, setDetailScrollRequest] = useState(0)
  // 引用回复目标：用户点了哪条消息的「回复」按钮 → 输入框上面渲染 quote 头。
  // 发送后自动清掉。切换会话也清掉（下面的 selectSession effect）。
  const [replyTarget, setReplyTarget] = useState<{ messageId: string; text: string } | null>(null)

  const detailScrollRef = useRef<HTMLDivElement | null>(null)
  const imageInputRef = useRef<HTMLInputElement | null>(null)
  const selectedSessionIdRef = useRef(selectedSessionId)
  selectedSessionIdRef.current = selectedSessionId
  const sessionsByUidRef = useRef(sessionsByUid)
  sessionsByUidRef.current = sessionsByUid
  const historyByKeyRef = useRef(historyByKey)
  historyByKeyRef.current = historyByKey

  const activeAccounts = useMemo(
    () => filterMessageCapableAccounts(accounts),
    [accounts],
  )
  const activeAccountUids = useMemo(
    () => activeAccounts.map((a: any) => a.uid),
    [activeAccounts],
  )
  const activeAccountKey = useMemo(() => activeAccountUids.join('|'), [activeAccountUids])
  const activeAccountKeyRef = useRef(activeAccountKey)
  activeAccountKeyRef.current = activeAccountKey
  const activeUidSetRef = useRef(new Set<string>())
  activeUidSetRef.current = new Set(activeAccountUids)
  const platformMessagesEnabledRef = useRef(platformMessagesEnabled)
  platformMessagesEnabledRef.current = platformMessagesEnabled

  function clearMessageState() {
    setSessionsByUid({})
    setSelectedSessionId('')
    setHistoryByKey({})
    setHistoryLoadingByKey({})
    setItemPriceByItemId({})
    setHeadinfoBySessionId({})
    setComposerText('')
    setSending(false)
    setConnectionStates({})
    setPlatformMessagesEnabled(true)
    setReplyTarget(null)
    const store = useUserUnreadStore.getState()
    store.reset()
  }
  const { authToken } = useAuthScope(clearMessageState)

  // ─── live event handler — pulled into helper to keep this hook short ──
  const handleLiveEvent = useCallback(
    (event: any) => {
      const uid = String(event?.uid || '').trim()
      if (uid && !activeUidSetRef.current.has(uid)) return
      if (!platformMessagesEnabledRef.current && isPlatformNotificationSession(event)) return
      createLiveEventHandler({
      selectedSessionIdRef,
      sessionsByUidRef,
      setSessionsByUid,
      setHistoryByKey,
      setConnectionStates,
      })(event)
    },
    [],
  )

  // ─── session list loading (stale-while-revalidate) ──────────────────
  // Modes:
  //   default          → cached DB read, spinner visible (initial mount).
  //   { force: true }  → upstream mtop sync, spinner visible (refresh btn).
  //   { background: true } → upstream mtop sync, spinner hidden (idle refresh).
  const loadAllSessions = useCallback(
    async (
      { background = false, force = false }: { background?: boolean; force?: boolean } = {},
    ) => {
      if (!activeAccounts.length) {
        clearMessageState()
        return
      }
      const requestToken = getAuthToken()
      const requestAccountKey = activeAccountKeyRef.current

      const hitUpstream = background || force
      // Spinner UX 跟数据合并解耦：
      //   spinner —— 只等「第一个账号 settle」，UI 立刻给反馈
      //   data    —— 等所有账号 settle 后做 ONE 个 batch 合并
      // 之前每个账号 .then 里单独 setSessionsByUid，账号多时触发 N 次重渲染
      // 把整个 webview 卡死，刷新完点页面不动。改回一次性合并就好。
      if (!background) setRefreshing(true)

      const fetchOne = async (account: any) => {
        const response = await pcSessions(account.uid, '[1,3,6]', 500, {
          cached: !hitUpstream,
          force: hitUpstream,
        })
        if (response?.warning) {
          notify.error(`${account.nick || account.uid} 消息会话已使用缓存：${response.warning}`)
        }
        return {
          uid: account.uid,
          sessions: Array.isArray(response?.sessions) ? response.sessions : [],
        }
      }

      // 并发限流——上一版一次性 fire N 个上行，活跃账号 20+ 时把后端 sign
      // service + MTOP 通道挤爆，连 GET /api/accounts 都跟着挂。窗口大小 4
      // 是经验值（拉一次 sessions 平均 ~500ms，sign service 单 worker 时
      // 12 个账号刚好 1.5s 内出全部数据）。
      const CONCURRENCY = 4
      const tasks: Array<Promise<{ uid: string; sessions: any[] }>> = []
      const resolvers: Array<(value: { uid: string; sessions: any[] } | PromiseLike<{ uid: string; sessions: any[] }>) => void> = []
      const rejecters: Array<(reason?: any) => void> = []
      activeAccounts.forEach(() => {
        tasks.push(new Promise((resolve, reject) => {
          resolvers.push(resolve)
          rejecters.push(reject)
        }))
      })

      ;(async () => {
        let cursor = 0
        async function worker() {
          while (cursor < activeAccounts.length) {
            const i = cursor++
            try {
              resolvers[i](await fetchOne(activeAccounts[i]))
            } catch (e) {
              rejecters[i](e)
            }
          }
        }
        await Promise.all(
          Array.from({ length: Math.min(CONCURRENCY, activeAccounts.length) }, worker),
        )
      })().catch(() => {})

      // 第一个 settle 立刻关 spinner——独立于数据合并 流程；finally 兜底
      // 全部 reject 的情况。
      if (!background && tasks.length > 0) {
        Promise.any(tasks)
          .catch(() => {})
          .finally(() => {
            if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
              setRefreshing(false)
            }
          })
      }

      try {
        const results = await Promise.allSettled(tasks)
        if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
        // ONE 个 batch update —— 避免 setState fan-out 引起重渲染雪崩
        setSessionsByUid((prev) => {
          const next = { ...prev }
          const activeUidSet = new Set(activeAccounts.map((account: any) => account.uid))
          for (const uid of Object.keys(next)) {
            if (!activeUidSet.has(uid)) delete next[uid]
          }
          for (const result of results) {
            if (result.status !== 'fulfilled') continue
            next[result.value.uid] = mergeSessionLists(result.value.sessions, prev[result.value.uid] || [])
          }
          return next
        })
        const store = useUserUnreadStore.getState()
        for (const result of results) {
          if (result.status !== 'fulfilled') continue
          store.setSessionsForUid(result.value.uid, result.value.sessions)
        }
        store.pruneToUids(activeAccounts.map((a: any) => a.uid))
      } finally {
        // Promise.any 失败兜底（全部 reject 时 .finally 上面也跑过，但保险起见）
        if (!background && getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
          setRefreshing(false)
        }
      }
    },
    [activeAccounts, notify],
  )

  const loadAllSessionsRef = useRef(loadAllSessions)
  loadAllSessionsRef.current = loadAllSessions

  const handleFallbackRefresh = useCallback(({ background }: { background: boolean }) => {
    if (background) {
      loadAllSessionsRef.current().catch(() => {})
    }
  }, [authToken])

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
  }, [activeAccountUids, liveSubscribe])

  // ─── derived: combined sessions / filters / groups ─────────────────
  // Every state mutation re-renders. With 700+ sessions across 26 accounts,
  // recomputing flatMap → sort → filter → groupByBucket each time was what
  // made clicking a session feel like it took seconds. Memoize so only the
  // pieces that actually changed get recomputed.
  const allSessions = useMemo(
    () =>
      activeAccounts
        .flatMap((account: any) => (sessionsByUid[account.uid] || []).map((session: any) => ({
          ...session,
          _uid: account.uid,
          _accountNick: account.nick || account.uid,
          _accountAvatar: account.avatar,
          // Mobile list reads `_nick` / `_avatar` rather than desktop's
          // `_accountNick` / `_accountAvatar`. Surface both so neither page
          // needs to know what the other calls them.
          _nick: account.nick || account.uid,
          _avatar: account.avatar,
        })))
        .filter((session: any) => (
          platformMessagesEnabled || !isPlatformNotificationSession(session)
        ))
        .sort((left: any, right: any) =>
          normalizeSessionTimestamp(right.ts) - normalizeSessionTimestamp(left.ts),
        ),
    [activeAccounts, platformMessagesEnabled, sessionsByUid],
  )

  const filteredSessions = useMemo(
    () => allSessions.filter((session: any) => {
      if (selectedAccountUid !== 'all' && session._uid !== selectedAccountUid) return false
      if (selectedAccountUid === 'all') {
        if (isPinnedNotificationType(sessionType(session))) return false
      }
      return isSessionTypeEnabled(session, TYPE_FILTERS)
    }),
    [allSessions, selectedAccountUid],
  )

  const pinnedSessions = useMemo(
    () => {
      if (selectedAccountUid === 'all') return []
      const pinned = filteredSessions.filter((s: any) => isPinnedNotificationType(sessionType(s)))
      // 闲鱼把"工作台通知"(sessionType=53) fan-out 到 3 个平台子账号 — 同一通知
      // 出 3 行。手机 APP 按 latestMessageId 折叠成 1 行。这里按 _uid 去重。
      return dedupeWorkbenchSessions(pinned)
    },
    [filteredSessions, selectedAccountUid],
  )

  const chatSessions = useMemo(
    () => selectedAccountUid !== 'all'
      ? filteredSessions.filter((s: any) => !isPinnedNotificationType(sessionType(s)))
      : filteredSessions,
    [filteredSessions, selectedAccountUid],
  )

  const groupedSessions = useMemo(() => groupSessionsByBucket(chatSessions), [chatSessions])

  // Resolve the selected session by key. Prefer the filtered list (used by
  // desktop UI), fall back to allSessions so MobileChat — which navigates by
  // URL and may target a session hidden by the desktop filter, or arrive
  // before its account's list finishes loading — still gets a hydrated
  // session for sendActions / loadHistory / mark-read. Without the fallback
  // selectedSession was null on direct-link refresh and the composer's
  // handleSend silently dropped sends.
  const selectedSession = useMemo(() => {
    if (!selectedSessionId) return null
    const fromFiltered = filteredSessions.find(
      (session: any) => sessionKey(session) === selectedSessionId,
    )
    if (fromFiltered) return fromFiltered
    return allSessions.find(
      (session: any) => sessionKey(session) === selectedSessionId,
    ) || null
  }, [filteredSessions, allSessions, selectedSessionId])
  const selectedHistory = selectedSession ? historyByKey[sessionKey(selectedSession)] || {} : {}
  const selectedMessages = useMemo(() => {
    const msgs = selectedHistory?.messages || []
    const st = sessionType(selectedSession)
    if (st === 3 || st === 6) return [...msgs].reverse()
    return msgs
  }, [selectedHistory?.messages, selectedSession])
  const selectedLatestMessageKey = useMemo(() => {
    const latest = selectedMessages[selectedMessages.length - 1]
    if (!latest) return ''
    return [
      latest.messageId || '',
      latest.version || '',
      latest.ts || '',
      latest.preview || latest.text || latest.title || '',
    ].join('|')
  }, [selectedMessages])
  const selectedItem = useMemo(() => resolveSessionItem(selectedSession), [selectedSession])
  const selectedLoadingState = selectedSession ? historyLoadingByKey[sessionKey(selectedSession)] || {} : {}

  // 只计 sessionType=1 私聊的未读——通知（3）/ 互动（6）这些平台 broadcast
  // 不算「我有 N 条消息没看」，跟手机端 row 上不显示未读徽章保持一致。
  // 否则用户看消息列表里没未读，左上角却写 (36)，体验割裂。
  const totalUnread = useMemo(
    () => allSessions.reduce((sum: number, session: any) => {
      if (sessionType(session) !== 1) return sum
      return sum + Number(session?.unread || 0)
    }, 0),
    [allSessions],
  )

  // ─── helpers ──────────────────────────────────────────────────────
  // MobileChat opens via a direct route — the user may hard-refresh on a
  // chat URL before `loadAllSessions` has populated `sessionsByUid` for that
  // uid. Without a session record in the store, the hook's `selectedSession`
  // memo can't resolve the URL into a hydrated session, so handleSend bails
  // out and the loadHistory effect short-circuits. `upsertSeedSession`
  // inserts a minimal placeholder so the rest of the pipeline has something
  // to chew on; the next loadAllSessions merges over it (sessionMerge keeps
  // existing fields like peerUid / itemId when the API doesn't carry them).
  const upsertSeedSession = useCallback((session: any) => {
    if (!platformMessagesEnabledRef.current && isPlatformNotificationSession(session)) return
    const uid = String(session?._uid || '').trim()
    const sid = String(session?.sessionId || '').trim()
    if (!uid || !sid) return
    setSessionsByUid((prev) => {
      const current = prev[uid] || []
      if (current.some((s: any) => String(s?.sessionId || '') === sid)) return prev
      const merged = [{ ...session, _uid: uid }, ...current]
      return {
        ...prev,
        [uid]: merged.sort((left: any, right: any) =>
          normalizeSessionTimestamp(right.ts) - normalizeSessionTimestamp(left.ts),
        ),
      }
    })
  }, [])

  const updateSessionsForUid = useCallback((uid: string, updater: (sessions: any[]) => any[]) => {
    setSessionsByUid((prev) => {
      const current = prev[uid] || []
      const nextSessions = updater(current)
      return {
        ...prev,
        [uid]: [...nextSessions].sort((left: any, right: any) =>
          normalizeSessionTimestamp(right.ts) - normalizeSessionTimestamp(left.ts),
        ),
      }
    })
  }, [])

  const updateSelectedSession = useCallback((patch: any) => {
    const uid = String(selectedSession?._uid || '').trim()
    const sid = String(selectedSession?.sessionId || patch?.sessionId || '').trim()
    if (!uid || !sid) return

    updateSessionsForUid(uid, (sessions: any[]) => sessions.map((current: any) => (
      current.sessionId === sid
        ? { ...current, ...patch }
        : current
    )))
  }, [selectedSession?._uid, selectedSession?.sessionId, updateSessionsForUid])

  // ─── history loading (stale-while-revalidate, mirrors loadAllSessions) ─
  // background=false: `cached=true`, skips 4-5s mtop upstream call.
  // background=true:  `force=true`, runs the mtop fetch silently to close
  //                   any gaps. Only the first call toggles `loading`.
  const loadHistory = useCallback(async (session: any, { background = false } = {}) => {
    if (!session?._uid || !session?.sessionId) return

    const key = sessionKey(session)
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    if (!background) {
      setHistoryLoadingByKey((prev) => ({
        ...prev,
        [key]: { loading: true, count: 0, pages: 0 },
      }))
    }

    try {
      const result = await loadCompleteHistory({
        fetchPage: (uid: string, sid: string, stype: number, n: number, cursor: any) =>
          pcMessageHistory(uid, sid, stype, n, cursor as number, {
            cached: !background,
            force: background,
          }),
        uid: session._uid,
        sessionId: session.sessionId,
        sessionType: sessionType(session),
        // Cache reads hit the local SQLite `message_history_cache` and
        // comfortably handle thousands of rows in a single roundtrip — pull
        // everything in one shot rather than paginating. Without this, the
        // "已加载最近 2500 条" notice fires even when the DB has full history.
        pageSize: 5000,
        maxPages: 1,
        onProgress: ({ count, pages }: any) => {
          if (background) return
          setHistoryLoadingByKey((prev) => ({
            ...prev,
            [key]: { loading: true, count, pages },
          }))
        },
      })

      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      setHistoryByKey((prev) => {
        const prevMessages = prev[key]?.messages || []
        // Merge fresh fetch with what's already on-screen (optimistic bubbles,
        // newer messages that came in via a different poll, etc.). Without
        // this, every 15s tick wipes the just-sent optimistic and any inbound
        // reply the backend hadn't echoed to cache yet.
        const merged = mergeHistoryState(
          { messages: prevMessages },
          { messages: result.messages },
          'refresh',
        ).messages

        // Drop an optimistic_* bubble once a real echo with matching signature
        // (text+imageUrl) and isSelf shows up within 30s. For images we match
        // on imageUrl; for text we match text.
        const ECHO_WINDOW_MS = 30_000
        const sig = (m: any) =>
          `${String(m?.text ?? m?.content ?? '')}|${String(m?.imageUrl ?? '')}`
        const realMatches: { sig: string; ts: number }[] = merged
          .filter((m: any) => !String(m?.messageId || '').startsWith('optimistic_'))
          .filter((m: any) => m?.isSelf)
          .map((m: any) => ({ sig: sig(m), ts: Number(m?.ts) || 0 }))
        const deduped = merged.filter((m: any) => {
          const id = String(m?.messageId || '')
          if (!id.startsWith('optimistic_')) return true
          const mySig = sig(m)
          const ts = Number(m?.ts) || 0
          const matched = realMatches.some(
            (real) => real.sig === mySig && Math.abs(real.ts - ts) <= ECHO_WINDOW_MS,
          )
          return !matched
        })

        return {
          ...prev,
          [key]: {
            ...(prev[key] || {}),
            uid: session._uid,
            session,
            messages: deduped,
            nextCursor: result.nextCursor,
            latestCursor: result.latestCursor,
            reachedLimit: result.reachedLimit,
            notice: result.notice,
            loadedAll: true,
            loadedAt: Date.now(),
          },
        }
      })
    } finally {
      if (!background) {
        if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
          setHistoryLoadingByKey((prev) => ({
            ...prev,
            [key]: {
              ...(prev[key] || {}),
              loading: false,
            },
          }))
        }
      }
    }
  }, [])

  // ─── markRead: shared by selectSession (desktop) + long-press (mobile) ──
  const markSessionRead = useCallback((session: any) => {
    const uid = String(session?._uid || '')
    const sid = String(session?.sessionId || '')
    const st = sessionType(session)
    if (!uid || !sid) return

    // 1) local zero-out so the row updates immediately
    updateSessionsForUid(uid, (sessions: any[]) => sessions.map((current: any) => (
      current.sessionId === sid
        ? { ...current, unread: 0 }
        : current
    )))

    // 2) global unread store (affects "消息" nav badge)
    useUserUnreadStore.getState().setUnread(uid, sid, st, 0)

    // 3) backend — writes local DB + mtop pc.session.unread.clean. Without
    //    this the next `pcSessions` round-trip restores unread.
    pcMarkSessionRead(uid, sid, st).catch(() => {})
    if (st === 1) {
      // Real-API read endpoint mtop.taobao.idlemessage.message.read.
      // Pass latestMessageId explicitly so the cursor is pinned correctly
      // even when the cache hasn't refreshed yet.
      const latestMsgId = String(session?.latestMessageId || '').trim()
      const peerUid = String(session?.peerUid || session?.peer_uid || '').trim()
      liveMarkSessionRead(uid, sid, {
        msgIdList: latestMsgId ? [latestMsgId] : [],
        peerUid,
      }, {
        timeoutMs: 2500,
        timeoutMessage: '已读同步超时',
      }).catch(() => {})
    }
  }, [])

  // Stable reference so memoized SessionRow props don't invalidate on every
  // re-render (which would defeat React.memo).
  const selectSession = useCallback((session: any) => {
    const key = sessionKey(session)
    const selectingSameSession = selectedSessionIdRef.current === key
    setSelectedSessionId(key)
    setDetailScrollRequest((value) => value + 1)
    if (!selectingSameSession) {
      setComposerText('')
      // 切到别的会话时把残留的 reply 清掉，否则会把上一个会话的引用粘到新会话。
      setReplyTarget(null)
    }
    markSessionRead(session)
  }, [markSessionRead])

  useEffect(() => {
    function handleOpenSession(e: Event) {
      const detail = (e as CustomEvent).detail
      if (!detail?.key) return
      if (detail.uid) setSelectedAccountUid(detail.uid)
      setTimeout(() => {
        setSelectedSessionId(detail.key)
        setDetailScrollRequest((value) => value + 1)
      }, 50)
    }
    window.addEventListener('xy:openSession', handleOpenSession)
    return () => window.removeEventListener('xy:openSession', handleOpenSession)
  }, [])

  // ─── send / recall actions — pulled into helper to keep this hook short ─
  // The factory closes over refs so re-renders don't reinvoke it; the action
  // functions read the live state at call time, not at hook-render time.
  const selectedSessionRef = useRef(selectedSession)
  selectedSessionRef.current = selectedSession
  const composerTextRef = useRef(composerText)
  composerTextRef.current = composerText
  const sendingRef = useRef(sending)
  sendingRef.current = sending
  const replyTargetRef = useRef(replyTarget)
  replyTargetRef.current = replyTarget

  const sendActions = useMemo(
    () => createSendActions({
      selectedSessionRef,
      composerTextRef,
      sendingRef,
      replyTargetRef,
      setSending,
      setComposerText,
      setHistoryByKey,
      setReplyTarget,
      updateSessionsForUid,
      loadHistory,
      notifyError: (message: string) => notify.error(message),
    }),
    [loadHistory, notify],
  )
  const {
    handleSend,
    handleImagePick,
    handleSendLocation,
    handleSendItem,
    recallMessage,
  } = sendActions

  // ─── effects ──────────────────────────────────────────────────────
  useEffect(() => {
    loadAccounts().catch(() => {})
  }, [authToken])

  useEffect(() => {
    const requestToken = getAuthToken()
    getMessageNotificationSettings()
      .then((settings: any) => {
        if (getAuthToken() !== requestToken) return
        setPlatformMessagesEnabled(settings?.platformMessagesEnabled !== false)
      })
      .catch(() => {})
  }, [authToken])

  useEffect(() => {
    if (!activeAccounts.length) {
      clearMessageState()
      return
    }
    // Pure cached read on mount. The live worker keeps the cache warm via
    // ACCS push; users can hit refresh for manual upstream sync.
    loadAllSessions().catch(() => {})
  }, [accounts])

  // Auto-pick the first session ONLY when nothing has been explicitly
  // selected. Critically, we don't clobber an explicit selectedSessionId
  // that's missing from filteredSessions — that happens when MobileChat
  // navigates to /messages/chat/<uid>/<sid>/<type> before the session list
  // for that uid has loaded. Before this change the reconcile would yank the
  // selection back to filteredSessions[0], stranding MobileChat with the
  // wrong history. Desktop's account-filter switch is handled by the
  // separate effect below that clears the selection on selectedAccountUid
  // change, which then triggers this reconcile cleanly.
  useEffect(() => {
    if (!filteredSessions.length) {
      setSelectedSessionId('')
      return
    }
    if (!selectedSessionId) {
      setSelectedSessionId(sessionKey(filteredSessions[0]))
    }
  }, [filteredSessions, selectedSessionId])

  // Switching account filter invalidates the prior selection — clear so the
  // reconcile effect above picks the first session for the new account.
  // Skipped on initial mount (selectedSessionId === '' already, no-op).
  useEffect(() => {
    setSelectedSessionId('')
  }, [selectedAccountUid])

  // itemDetail + pcMessageHeadinfo lazy resolvers — pulled into helper to
  // keep this hook short.
  useSelectedSessionResolvers({
    selectedSession,
    selectedItem,
    itemPriceByItemId,
    setItemPriceByItemId,
    headinfoBySessionId,
    setHeadinfoBySessionId,
    updateSelectedSession,
  })

  useEffect(() => {
    if (!selectedSession) return
    if (selectedHistory?.loadedAll || selectedLoadingState?.loading) return
    // Pure cache read — the live worker keeps `message_history_cache` warm
    // via ACCS push, and the backend persists self-sent messages on send.
    loadHistory(selectedSession).catch(() => {})
  }, [selectedSessionId, filteredSessions.length, selectedHistory?.loadedAll, loadHistory])

  // Track the focused session in the global store so live-WS pushes don't
  // bump the "消息" badge for a chat the user is actively reading.
  useEffect(() => {
    const store = useUserUnreadStore.getState()
    if (selectedSession?._uid && selectedSession?.sessionId && sessionType(selectedSession) === 1) {
      store.setFocused(selectedSession._uid, selectedSession.sessionId)
    } else {
      store.setFocused('', '')
    }
    return () => {
      useUserUnreadStore.getState().setFocused('', '')
    }
  }, [selectedSession?._uid, selectedSession?.sessionId])

  useLayoutEffect(() => {
    if (!selectedMessages.length) return
    const node = detailScrollRef.current
    if (!node) return
    let cancelled = false
    const scrollToCurrentEdge = () => {
      if (cancelled) return
      const st = sessionType(selectedSessionRef.current)
      if (st === 3 || st === 6) {
        node.scrollTop = 0
      } else {
        node.scrollTop = node.scrollHeight
      }
    }
    scrollToCurrentEdge()
    let nestedRaf = 0
    const raf = window.requestAnimationFrame(() => {
      scrollToCurrentEdge()
      nestedRaf = window.requestAnimationFrame(scrollToCurrentEdge)
    })
    const timers = [80, 240, 600].map((delay) => window.setTimeout(scrollToCurrentEdge, delay))
    return () => {
      cancelled = true
      window.cancelAnimationFrame(raf)
      if (nestedRaf) window.cancelAnimationFrame(nestedRaf)
      timers.forEach((timer) => window.clearTimeout(timer))
    }
  }, [
    selectedSessionId,
    selectedMessages.length,
    selectedLatestMessageKey,
    selectedHistory?.loadedAt,
    detailScrollRequest,
  ])

  return {
    // raw state
    accounts, activeAccounts, activeAccountUids,
    sessionsByUid, allSessions, refreshing,
    historyByKey, historyLoadingByKey,
    selectedAccountUid, selectedSessionId,
    composerText, sending,
    replyTarget,
    connectionStates, itemPriceByItemId, headinfoBySessionId,
    detailScrollRef, imageInputRef,

    // computed
    filteredSessions, pinnedSessions, chatSessions, groupedSessions,
    selectedSession, selectedHistory, selectedMessages, selectedItem,
    selectedLoadingState, totalUnread,
    isPinnedNotificationType,

    // setters
    setSelectedAccountUid, setSelectedSessionId, setComposerText,
    setReplyTarget,

    // actions
    loadAccounts, loadAllSessions, loadHistory,
    selectSession, recallMessage, markSessionRead,
    handleSend, handleImagePick, handleSendLocation, handleSendItem,
    upsertSeedSession,
  }
}
