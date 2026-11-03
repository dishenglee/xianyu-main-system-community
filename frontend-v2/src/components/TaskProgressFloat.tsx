import { useEffect, useRef, useState, type PointerEvent } from 'react'
import { CheckCircle2, Package, RefreshCw, X, XCircle } from 'lucide-react'
import { useAccounts } from '../hooks/useAccounts'
import { useTaskProgressStore } from '../store/taskProgressStore'
import { accountDisplayNameByUid } from '../utils/accountDisplay'

const DEFAULT_POSITION = {
  x: 16,
  y: 98,
}

function clampPosition(position: { x: number; y: number }) {
  if (typeof window === 'undefined') return position
  const maxX = Math.max(12, window.innerWidth - 132)
  const maxY = Math.max(12, window.innerHeight - 60)
  return {
    x: Math.min(Math.max(position.x, 12), maxX),
    y: Math.min(Math.max(position.y, 12), maxY),
  }
}

export function TaskProgressFloat() {
  const { title, indeterminate, running, done, total, results, visible, dismiss } = useTaskProgressStore()
  const { accounts } = useAccounts()
  const [hovered, setHovered] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const [position, setPosition] = useState(DEFAULT_POSITION)
  const [dragging, setDragging] = useState(false)
  const dragRef = useRef<{
    pointerId: number
    startClientX: number
    startClientY: number
    startX: number
    startY: number
    moved: boolean
  } | null>(null)

  useEffect(() => {
    if (!visible) return
    const onResize = () => setPosition((current) => clampPosition(current))
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [visible])

  if (!visible) return null

  const successCount = results.filter((r) => r.ok).length
  const failCount = results.filter((r) => !r.ok).length
  const allOk = results.length > 0 && failCount === 0
  const pct = total > 0 ? Math.round((done / total) * 100) : 0
  const floatingOnLeft = position.x > (typeof window === 'undefined' ? 360 : window.innerWidth / 2)
  const showDetails = results.length > 0 && (hovered || expanded)

  function handlePointerDown(e: PointerEvent<HTMLDivElement>) {
    if ((e.target as HTMLElement).closest('button')) return
    dragRef.current = {
      pointerId: e.pointerId,
      startClientX: e.clientX,
      startClientY: e.clientY,
      startX: position.x,
      startY: position.y,
      moved: false,
    }
    e.currentTarget.setPointerCapture(e.pointerId)
    setDragging(true)
  }

  function handlePointerMove(e: PointerEvent<HTMLDivElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    const next = clampPosition({
      x: drag.startX - (e.clientX - drag.startClientX),
      y: drag.startY - (e.clientY - drag.startClientY),
    })
    drag.moved = drag.moved || Math.abs(e.clientX - drag.startClientX) > 3 || Math.abs(e.clientY - drag.startClientY) > 3
    setPosition(next)
  }

  function finishDrag(e: PointerEvent<HTMLDivElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    const shouldToggle = !drag.moved
    dragRef.current = null
    setDragging(false)
    try {
      e.currentTarget.releasePointerCapture(e.pointerId)
    } catch {
      /* ignore */
    }
    if (shouldToggle) setExpanded((value) => !value)
  }

  return (
    <div
      className="fixed z-[9999]"
      style={{
        bottom: `calc(env(safe-area-inset-bottom) + ${position.y}px)`,
        right: position.x,
        touchAction: 'none',
      }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={finishDrag}
      onPointerCancel={finishDrag}
    >
      {showDetails && (
        <div
          className="modal-card mb-2 overflow-hidden"
          style={{
            width: 340,
            maxWidth: 'calc(100vw - 24px)',
            maxHeight: 300,
            animation: 'fadeInUp 150ms ease-out',
            transform: floatingOnLeft ? 'translateX(calc(100% - 120px))' : undefined,
          }}
        >
          <div
            className="px-3 py-2 text-[12px] font-medium flex items-center justify-between"
            style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)', borderBottom: '1px solid var(--control-border)' }}
          >
            <span>{title}详情 {indeterminate ? '账号级结果' : `${done}/${total}`}</span>
            {!running && successCount > 0 && failCount > 0 ? (
              <span>{successCount}成功 {failCount}失败</span>
            ) : null}
          </div>
          <div className="overflow-y-auto" style={{ maxHeight: 240 }}>
            {results.map((r, i) => {
              const label = r.label || (r.uid ? accountDisplayNameByUid(r.uid, accounts as any) : '') || `任务 ${i + 1}`
              return (
                <div
                  key={`${r.uid || r.label || 'task'}-${i}`}
                  className="flex items-center gap-2 px-3 py-[6px] text-[12px]"
                  style={{ borderTop: i > 0 ? '1px solid var(--control-border)' : 'none' }}
                >
                  {r.ok
                    ? <CheckCircle2 className="w-3.5 h-3.5 shrink-0" style={{ color: 'var(--success)' }} />
                    : <XCircle className="w-3.5 h-3.5 shrink-0" style={{ color: 'var(--danger)' }} />}
                  <span className="truncate flex-1" style={{ color: 'var(--text-main)' }}>{label}</span>
                  <span className="shrink-0 max-w-[128px] truncate text-[11px]" style={{ color: r.ok ? 'var(--success)' : 'var(--danger)' }}>{r.msg}</span>
                </div>
              )
            })}
          </div>
        </div>
      )}

      <div
        className="glass-dock flex items-center gap-2 px-3.5 py-2 rounded-full select-none"
        style={{
          background: running
            ? 'linear-gradient(135deg, color-mix(in srgb, var(--warning) 88%, transparent), color-mix(in srgb, var(--warning) 70%, var(--danger)))'
            : allOk
              ? 'linear-gradient(135deg, rgba(52,199,89,0.90), rgba(48,179,80,0.86))'
              : 'linear-gradient(135deg, color-mix(in srgb, var(--warning) 88%, transparent), color-mix(in srgb, var(--warning) 78%, var(--text-main)))',
          color: 'var(--liquid-highlight)',
          fontSize: 13,
          fontWeight: 600,
          minWidth: 120,
          cursor: dragging ? 'grabbing' : 'grab',
          transform: dragging ? 'scale(0.98)' : 'none',
        }}
      >
        {running
          ? <RefreshCw className="w-4 h-4 animate-spin" />
          : <Package className="w-4 h-4" />}
        <span>
          {running
            ? indeterminate ? `${title}请求中` : `${title}中 ${done}/${total}`
            : `${title}完成 ${done}/${total}`}
        </span>
        {running ? (
          indeterminate ? null : (
          <div className="ml-1 w-12 h-1.5 rounded-full overflow-hidden" style={{ background: 'rgba(255,255,255,0.3)' }}>
            <div className="h-full rounded-full transition-all duration-300" style={{ width: `${pct}%`, background: 'var(--liquid-highlight)' }} />
          </div>
          )
        ) : (
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); dismiss() }}
            className="ml-1 p-0.5 rounded-full cursor-pointer transition-colors"
            style={{ background: 'transparent' }}
            onMouseEnter={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.20)' }}
            onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent' }}
            aria-label="关闭任务进度"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
      </div>

      <style>{`
        @keyframes fadeInUp {
          from { opacity: 0; transform: translateY(8px); }
          to   { opacity: 1; transform: translateY(0); }
        }
      `}</style>
    </div>
  )
}
