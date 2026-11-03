// @ts-nocheck
import { useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { TopBar } from '../../components/TopBar'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { useDevice } from '../../hooks/useDevice'
import TasksPage from './TasksPage'
import CommentTargetsPage from '../comments/CommentTargetsPage'
import CommentCandidatesPage from '../comments/CommentCandidatesPage'
import CommentLibraryPage from '../comments/CommentLibraryPage'
import CommentDeleteQueuePage from '../comments/CommentDeleteQueuePage'

const TABS = [
  { key: 'list',       label: '任务列表' },
  { key: 'targets',    label: '目标池' },
  { key: 'candidates', label: '候选商品' },
  { key: 'library',    label: '内容库' },
  { key: 'delete-queue', label: '删评队列' },
]

const VALID_KEYS = new Set(TABS.map((t) => t.key))

export default function CommentTaskHub() {
  const [params, setParams] = useSearchParams()
  const { isMobile, isTablet } = useDevice()
  const compact = isMobile || isTablet
  // URL ?tab=… is the source of truth so refresh / cross-page redirects land on the right tab.
  const initial = params.get('tab')
  const [active, setActive] = useState(VALID_KEYS.has(initial) ? initial : 'list')

  // Slot DOM node lives in the TopBar; portals from each view render into it based on active tab.
  const slotRef = useRef(null)
  const [slot, setSlot] = useState(null)

  useEffect(() => {
    const fromUrl = params.get('tab')
    if (fromUrl && VALID_KEYS.has(fromUrl) && fromUrl !== active) setActive(fromUrl)
    // Mirror default tab back to URL when missing, so reload preserves it.
    if (!fromUrl) {
      const next = new URLSearchParams(params)
      next.set('tab', active)
      setParams(next, { replace: true })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [params])

  function pickTab(key) {
    setActive(key)
    const next = new URLSearchParams(params)
    next.set('tab', key)
    setParams(next, { replace: false })
  }

  // Each view is mounted only when active to avoid wasted polling/network.
  const body = useMemo(() => {
    const props = { embedded: true, actionSlot: slot }
    switch (active) {
      case 'targets':      return <CommentTargetsPage {...props} />
      case 'candidates':   return <CommentCandidatesPage {...props} />
      case 'library':      return <CommentLibraryPage {...props} />
      case 'delete-queue': return <CommentDeleteQueuePage {...props} />
      case 'list':
      default:             return <TasksPage {...props} />
    }
  }, [active, slot])

  return (
    <div className={compact ? 'h-full flex flex-col mobile-page-shell' : 'contents'} style={compact ? { backgroundColor: 'var(--bg-page)' } : undefined}>
      {compact ? (
        <TopBar
          title="评论任务"
          right={
            <div
              ref={(node) => { slotRef.current = node; if (node !== slot) setSlot(node) }}
              className="flex items-center gap-1.5"
            />
          }
        />
      ) : null}

      {!compact && (
        <HubTabs
          tabs={HUB_PRESETS.automation}
          right={
            <div
              ref={(node) => { slotRef.current = node; if (node !== slot) setSlot(node) }}
              className="flex items-center gap-1"
            />
          }
        />
      )}

      <div className={compact ? 'flex-1 min-h-0 overflow-y-auto mobile-page-body' : 'flex-1 min-h-0 overflow-hidden'}>
        <div className={compact ? 'mobile-page-stack min-h-full' : 'flex h-full flex-col'}>
          {compact ? (
            <section className="mobile-surface-card p-4">
              <div className="mobile-section-head items-start">
                <div className="mobile-section-copy">
                  <h2 className="mobile-section-title">评论任务</h2>
                </div>
                <span className="mobile-pill mobile-muted-pill shrink-0">{TABS.length} 个面板</span>
              </div>

              <nav
                className="mt-4 flex gap-2 overflow-x-auto whitespace-nowrap pb-1"
                style={{ scrollbarWidth: 'none' }}
              >
                {TABS.map((t) => {
                  const on = t.key === active
                  return (
                    <button
                      key={t.key}
                      onClick={() => pickTab(t.key)}
                      className={`mobile-pill shrink-0 transition-colors ${on ? '' : 'mobile-muted-pill'}`}
                      style={on ? { background: 'var(--primary-light)', color: 'var(--primary)' } : { color: 'var(--text-secondary)' }}
                    >
                      {t.label}
                    </button>
                  )
                })}
              </nav>
            </section>
          ) : (
            <nav
              className="shrink-0 flex gap-1 px-3 py-2 overflow-x-auto whitespace-nowrap border-b"
              style={{ borderColor: 'var(--border-light)', backgroundColor: 'var(--bg-page)' }}
            >
              {TABS.map((t) => {
                const on = t.key === active
                return (
                  <button
                    key={t.key}
                    onClick={() => pickTab(t.key)}
                    className="px-3 py-1.5 rounded-full text-[12.5px] font-medium shrink-0 transition-colors"
                    style={on
                      ? { background: 'var(--primary)', color: 'var(--liquid-highlight)' }
                      : { background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
                  >
                    {t.label}
                  </button>
                )
              })}
            </nav>
          )}

          <div className="flex flex-col flex-1 min-h-0 overflow-hidden">
            {body}
          </div>
        </div>
      </div>
    </div>
  )
}
