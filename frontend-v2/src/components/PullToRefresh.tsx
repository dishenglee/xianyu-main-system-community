import { useState, useRef, type ReactNode, type TouchEvent } from 'react'

interface PullToRefreshProps {
  onRefresh: () => Promise<void>
  children: ReactNode
  threshold?: number
}

export function PullToRefresh({ onRefresh, children, threshold = 64 }: PullToRefreshProps) {
  const [pulling, setPulling] = useState(false)
  const [pullY, setPullY] = useState(0)
  const [refreshing, setRefreshing] = useState(false)
  const startY = useRef(0)
  const containerRef = useRef<HTMLDivElement>(null)

  function onTouchStart(e: TouchEvent) {
    if ((containerRef.current?.scrollTop ?? 0) > 0) return
    startY.current = e.touches[0].clientY
    setPulling(true)
  }

  function onTouchMove(e: TouchEvent) {
    if (!pulling) return
    const delta = Math.max(0, e.touches[0].clientY - startY.current)
    setPullY(Math.min(delta * 0.5, threshold * 1.5))
  }

  async function onTouchEnd() {
    if (!pulling) return
    setPulling(false)
    if (pullY >= threshold && !refreshing) {
      setRefreshing(true)
      setPullY(threshold)
      try {
        await onRefresh()
      } finally {
        setRefreshing(false)
        setPullY(0)
      }
    } else {
      setPullY(0)
    }
  }

  return (
    <div
      ref={containerRef}
      className="overflow-y-auto h-full"
      onTouchStart={onTouchStart}
      onTouchMove={onTouchMove}
      onTouchEnd={onTouchEnd}
    >
      {(pullY > 0 || refreshing) && (
        <div
          className="flex items-center justify-center text-[13px] overflow-hidden transition-all"
          style={{ height: pullY, color: 'var(--text-secondary)' }}
        >
          {refreshing ? '刷新中…' : pullY >= threshold ? '松手刷新' : '下拉刷新'}
        </div>
      )}
      {children}
    </div>
  )
}
