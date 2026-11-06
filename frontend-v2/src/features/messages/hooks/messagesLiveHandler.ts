// @ts-nocheck
// Live-event dispatcher extracted out of useMessagesPageState so the main
// hook stays under the 600-line target. Pure reducer-shape — no React state
// owned here; consumers pass their setters in.
import { applyLiveEventToHistory, applyLiveEventToSessions } from '../liveEvents'

const IS_DEV = import.meta.env.DEV

function devLog(...args: unknown[]) {
  if (IS_DEV) console.log(...args)
}

interface CreateLiveHandlerOptions {
  selectedSessionIdRef: { current: string }
  sessionsByUidRef: { current: Record<string, any[]> }
  setSessionsByUid: (updater: (prev: Record<string, any[]>) => Record<string, any[]>) => void
  setHistoryByKey: (updater: (prev: Record<string, any>) => Record<string, any>) => void
  setConnectionStates: (
    updater: (prev: Record<string, unknown>) => Record<string, unknown>,
  ) => void
}

export function createLiveEventHandler({
  selectedSessionIdRef,
  sessionsByUidRef,
  setSessionsByUid,
  setHistoryByKey,
  setConnectionStates,
}: CreateLiveHandlerOptions) {
  return function handleLiveEvent(event: any) {
    const type = String(event?.type || '')
    if (type === 'pong') return

    if (type.startsWith('connection.')) {
      devLog('[live-handler] connection event', type, event.uid)
      setConnectionStates((prev) => {
        const uid = String(event.uid || '').trim()
        if (!uid) return prev
        return {
          ...prev,
          [uid]: {
            status: type.replace('connection.', ''),
            reason: String(event.reason || '').trim(),
            snapshot: Boolean(event.snapshot),
            updatedAt: Number(event.updatedAt || 0) || Date.now(),
          },
        }
      })
      return
    }

    if (type !== 'message.received' && type !== 'message.sent') {
      devLog('[live-handler] ignored event type:', type)
      return
    }
    const uid = String(event.uid || '').trim()
    if (!uid) return

    devLog(
      '[live-handler] message event', type, 'uid=', uid,
      'session=', event.sessionId,
    )

    setSessionsByUid((prev) => {
      const current = prev[uid] || []
      const updated = applyLiveEventToSessions(current, event, {
        markSeen: selectedSessionIdRef.current.includes(event.sessionId),
      })
      return { ...prev, [uid]: updated }
    })

    const evtSessionKey = `${uid}:${event.sessionId}:${Number(event.sessionType || 1)}`
    devLog(
      '[live-handler] evtSessionKey=', evtSessionKey,
      'selected=', selectedSessionIdRef.current,
      'match=', evtSessionKey === selectedSessionIdRef.current,
    )
    if (evtSessionKey === selectedSessionIdRef.current) {
      setHistoryByKey((prev) => {
        const key = evtSessionKey
        const existing = prev[key] || {}
        const allSessionsNow = sessionsByUidRef.current[uid] || []
        const session = allSessionsNow.find((s: any) => s.sessionId === event.sessionId) || {}
        const updated = applyLiveEventToHistory(existing, session, event)
        return { ...prev, [key]: updated }
      })
    }
  }
}
