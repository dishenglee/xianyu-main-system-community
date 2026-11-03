import { useEffect, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { X } from 'lucide-react'
import { useDevice } from '../hooks/useDevice'

interface ModalProps {
  open: boolean
  onClose: () => void
  title?: string
  children: ReactNode
  footer?: ReactNode
  width?: number
}

export function Modal({ open, onClose, title, children, footer, width = 480 }: ModalProps) {
  const { isMobile } = useDevice()

  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    document.addEventListener('keydown', handler)
    return () => document.removeEventListener('keydown', handler)
  }, [open, onClose])

  if (!open) return null

  if (isMobile) {
    return <BottomSheet open={open} onClose={onClose} title={title} footer={footer}>{children}</BottomSheet>
  }

  return createPortal((
    <div className="fixed inset-0 z-[80] flex items-center justify-center p-4">
      <div className="modal-scrim absolute inset-0" onClick={onClose} />
      <div
        className="modal-card relative flex flex-col"
        style={{ width, maxWidth: '100%', maxHeight: '80vh' }}
      >
        {title && (
          <div className="modal-card-header flex items-center justify-between px-6 py-4">
            <h2 className="modal-card-title text-[17px] font-semibold">{title}</h2>
            <button onClick={onClose} className="mobile-icon-button w-8 h-8 flex items-center justify-center rounded-full transition-colors cursor-pointer" aria-label="关闭">
              <X className="w-4 h-4" />
            </button>
          </div>
        )}
        <div className="flex-1 overflow-y-auto p-6">{children}</div>
        {footer && (
          <div className="modal-card-footer px-6 py-4 flex justify-end gap-2">
            {footer}
          </div>
        )}
      </div>
    </div>
  ), document.body)
}

interface BottomSheetProps {
  open: boolean
  onClose: () => void
  title?: string
  children: ReactNode
  footer?: ReactNode
}

export function BottomSheet({ open, onClose, title, children, footer }: BottomSheetProps) {
  if (!open) return null
  return createPortal((
    <div className="fixed inset-0 z-[120] flex items-end">
      <div className="modal-scrim absolute inset-0" onClick={onClose} />
      <div
        className="mobile-bottom-sheet relative w-full flex flex-col overflow-hidden"
        style={{
          maxHeight: 'calc(100dvh - env(safe-area-inset-top) - env(keyboard-inset-height, 0px) - 8px)',
        }}
      >
        <div className="sheet-handle w-12 h-1.5 rounded-full mx-auto mt-3 mb-1.5 shrink-0" />
        {title && (
          <div className="modal-card-header flex items-center justify-between px-5 py-3 shrink-0">
            <div className="min-w-0">
              <div className="modal-card-kicker text-[10px] font-semibold uppercase tracking-[0.16em]">Panel</div>
              <h2 className="modal-card-title text-[18px] font-semibold mt-1">{title}</h2>
            </div>
            <button onClick={onClose} className="mobile-icon-button w-10 h-10 -mr-2 flex items-center justify-center rounded-full transition-colors cursor-pointer" aria-label="关闭">
              <X className="w-5 h-5" />
            </button>
          </div>
        )}
        <div
          className="flex-1 min-h-0 overflow-y-auto px-5 pt-5"
          style={{
            paddingBottom: footer ? 20 : 'calc(24px + env(safe-area-inset-bottom))',
          }}
        >
          {children}
        </div>
        {footer && (
          <div
            className="modal-card-footer sticky bottom-0 z-10 shrink-0 px-5 pt-4 flex gap-2"
            style={{ paddingBottom: 'calc(16px + env(safe-area-inset-bottom))' }}
          >
            {footer}
          </div>
        )}
      </div>
    </div>
  ), document.body)
}
