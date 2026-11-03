// @ts-nocheck
import { useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  MessageSquare,
  RefreshCw,
} from 'lucide-react'

import { EmptyState } from '../../components/EmptyState'
import { PullToRefresh } from '../../components/PullToRefresh'
import { SensitiveVisibilityToggle } from '../../components/SensitiveValue'
import { groupSessionsByBucket } from './aggregateMessagesModel'
import { useMessagesContext } from './MessagesRouteLayout'
import { peerLabel, sessionType } from './messagesConstants'
import { SessionRow } from './SessionRow'

const LONG_PRESS_MS = 450

export function MobileMessages() {
  const navigate = useNavigate()
  const {
    activeAccounts, allSessions, refreshing,
    loadAccounts, loadAllSessions, markSessionRead,
  } = useMessagesContext()

  async function handleRefresh() {
    await loadAccounts()
    await loadAllSessions({ force: true })
  }

  const isPlatformChannel = (session: any) => {
    const type = sessionType(session)
    return type === 3 || type === 6 || type === 53
  }

  const chatOnly = allSessions.filter((session) => !isPlatformChannel(session))
  const dedupedMap = new Map<string, any>()
  for (const session of chatOnly) {
    const sessionId = String(session?.sessionId ?? '')
    if (!sessionId) continue
    const prev = dedupedMap.get(sessionId)
    if (!prev) {
      dedupedMap.set(sessionId, session)
      continue
    }
    const prevTs = Number(prev?.ts ?? 0)
    const nextTs = Number(session?.ts ?? 0)
    const prevHasNick = String(prev?.peerNick ?? '').trim().length > 0
    const nextHasNick = String(session?.peerNick ?? '').trim().length > 0
    if (nextTs > prevTs || (nextTs === prevTs && nextHasNick && !prevHasNick)) {
      dedupedMap.set(sessionId, session)
    }
  }

  const deduped = Array.from(dedupedMap.values())
  const groupedSessions = groupSessionsByBucket(deduped)

  const [sessionMenu, setSessionMenu] = useState<any | null>(null)
  const pressTimerRef = useRef<number | null>(null)
  const pressOriginRef = useRef<{ x: number; y: number } | null>(null)

  function clearPressTimer() {
    if (pressTimerRef.current != null) {
      window.clearTimeout(pressTimerRef.current)
      pressTimerRef.current = null
    }
    pressOriginRef.current = null
  }

  function startSessionLongPress(event: React.TouchEvent, session: any) {
    clearPressTimer()
    const touch = event.touches[0]
    if (!touch) return
    pressOriginRef.current = { x: touch.clientX, y: touch.clientY }
    pressTimerRef.current = window.setTimeout(() => {
      setSessionMenu(session)
    }, LONG_PRESS_MS)
  }

  function moveSessionLongPress(event: React.TouchEvent) {
    const origin = pressOriginRef.current
    const touch = event.touches[0]
    if (!origin || !touch) return
    if (Math.hypot(touch.clientX - origin.x, touch.clientY - origin.y) > 12) {
      clearPressTimer()
    }
  }

  function handleMarkRead(session: any) {
    setSessionMenu(null)
    markSessionRead(session)
  }

  return (
    <div className="relative flex h-full flex-col mobile-page-shell overflow-hidden xianyu-message-list-page">
      {!activeAccounts.length ? (
        <div className="relative flex-1 mobile-page-body pt-3">
          <div
            className="mobile-surface-card flex h-full items-center justify-center px-6"
          >
            <EmptyState icon={MessageSquare} title="暂无账号" description="请先在账号管理中添加账号" />
          </div>
        </div>
      ) : (
        <div className="relative min-h-0 flex-1">
          <header className="xianyu-message-list-header">
            <h1>消息</h1>
            <div className="flex items-center gap-4">
              <SensitiveVisibilityToggle compact className="inline-flex h-8 w-8 items-center justify-center rounded-full" />
              <button type="button" aria-label="刷新消息" onClick={handleRefresh}>
                <RefreshCw className={refreshing ? 'animate-spin' : ''} size={22} />
              </button>
            </div>
          </header>

          <div className="xianyu-message-list-scroll min-h-0 flex-1">
            <PullToRefresh onRefresh={handleRefresh}>
              {!groupedSessions.length && !refreshing ? (
                <div className="flex h-full items-center justify-center px-6">
                  <EmptyState icon={MessageSquare} title="暂无消息" description="还没有任何聊天" />
                </div>
              ) : (
                <div className="pb-3">
                  {groupedSessions.map((group) => (
                    <div key={group.label}>
                      {group.sessions.map((session, index) => (
                        <SessionRow
                          key={`${session._uid}:${session.sessionId}:${session.sessionType || 1}:${index}`}
                          session={session}
                          active={false}
                          variant="mobile"
                          onSelect={(selected) => navigate(
                            `/messages/chat/${selected._uid}/${selected.sessionId}/${selected.sessionType || 1}`,
                            { state: { session: selected } },
                          )}
                          onTouchStart={(event) => startSessionLongPress(event, session)}
                          onTouchMove={moveSessionLongPress}
                          onTouchEnd={clearPressTimer}
                          onTouchCancel={clearPressTimer}
                          onContextMenu={(event) => { event.preventDefault(); setSessionMenu(session) }}
                        />
                      ))}
                    </div>
                  ))}
                </div>
              )}
            </PullToRefresh>
          </div>
        </div>
      )}

      {sessionMenu && (
        <div
          className="fixed inset-0 z-[80] flex flex-col justify-end"
          style={{ backgroundColor: 'color-mix(in srgb, var(--text-main) 28%, transparent)' }}
          onClick={() => setSessionMenu(null)}
        >
          <div
            className="mobile-bottom-sheet mx-2 mb-2 overflow-hidden rounded-[28px]"
            style={{
              paddingBottom: 'env(safe-area-inset-bottom)',
            }}
            onClick={(event) => event.stopPropagation()}
          >
            <div className="border-b px-4 py-3 text-[12px]" style={{ color: 'var(--text-secondary)', borderColor: 'var(--glass-border)' }}>
              {peerLabel(sessionMenu)}
            </div>
            <button
              onClick={() => handleMarkRead(sessionMenu)}
              className="w-full cursor-pointer py-4 text-center text-[15px]"
              style={{ color: 'var(--text-main)' }}
              disabled={Number(sessionMenu?.unread || 0) === 0}
            >
              {Number(sessionMenu?.unread || 0) > 0 ? '标记为已读' : '已读'}
            </button>
          </div>
          <button
            onClick={() => setSessionMenu(null)}
            className="mx-2 mb-2 cursor-pointer rounded-[24px] border py-4 text-center text-[15px]"
            style={{
              background: 'var(--mobile-glass-surface-strong, var(--mobile-glass-surface))',
              borderColor: 'var(--glass-border)',
              color: 'var(--primary)',
              paddingBottom: 'calc(1rem + env(safe-area-inset-bottom))',
            }}
          >
            取消
          </button>
        </div>
      )}
    </div>
  )
}
