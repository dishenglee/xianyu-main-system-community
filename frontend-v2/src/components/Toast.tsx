import { useState, useCallback, useRef } from 'react'
import { createPortal } from 'react-dom'
import { CheckCircle, XCircle, Info, X } from 'lucide-react'

type ToastType = 'success' | 'error' | 'info'

interface ToastItem {
  id: number
  type: ToastType
  message: string
}

const ICONS = {
  success: CheckCircle,
  error: XCircle,
  info: Info,
}

const COLORS = {
  success: 'var(--status-success-text)',
  error: 'var(--status-danger-text)',
  info: 'var(--primary)',
}

let counter = 0

const MIN_DURATIONS: Record<ToastType, number> = {
  success: 5000,
  info: 5200,
  error: 6500,
}

export function useToast() {
  const [toasts, setToasts] = useState<ToastItem[]>([])
  const timers = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map())

  const dismiss = useCallback((id: number) => {
    clearTimeout(timers.current.get(id))
    timers.current.delete(id)
    setToasts((prev) => prev.filter((t) => t.id !== id))
  }, [])

  const show = useCallback((message: string, type: ToastType = 'info', duration = MIN_DURATIONS[type]) => {
    const id = ++counter
    const displayDuration = Math.max(duration, MIN_DURATIONS[type])
    setToasts((prev) => [...prev, { id, type, message }])
    const timer = setTimeout(() => dismiss(id), displayDuration)
    timers.current.set(id, timer)
  }, [dismiss])

  return { toasts, show, dismiss }
}

interface ToastContainerProps {
  toasts: ToastItem[]
  dismiss: (id: number) => void
}

export function ToastContainer({ toasts, dismiss }: ToastContainerProps) {
  if (!toasts.length) return null
  const content = (
    <div className="fixed bottom-20 left-1/2 -translate-x-1/2 z-[300] flex flex-col gap-2 items-center pointer-events-none">
      {toasts.map((t) => {
        const Icon = ICONS[t.type]
        return (
          <div
            key={t.id}
            className="liquid-panel flex items-center gap-2 px-4 py-2.5 rounded-2xl pointer-events-auto"
            style={{ color: 'var(--text-main)', minWidth: 200, maxWidth: 320 }}
          >
            <Icon className="w-4 h-4 shrink-0" style={{ color: COLORS[t.type] }} />
            <span className="text-[14px] flex-1 whitespace-pre-wrap break-words">{t.message}</span>
            <button onClick={() => dismiss(t.id)} className="cursor-pointer" aria-label="关闭">
              <X className="w-3.5 h-3.5 opacity-60" />
            </button>
          </div>
        )
      })}
    </div>
  )
  if (typeof document === 'undefined') return content
  return createPortal(content, document.body)
}
