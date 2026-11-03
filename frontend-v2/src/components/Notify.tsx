import React, { createContext, useCallback, useContext, useMemo } from 'react'
import { ToastContainer, useToast } from './Toast'

type NotifyType = 'success' | 'error' | 'info'

type NotifyCallable = (
  messageOrType: string,
  typeOrMessage?: NotifyType | string,
  duration?: number,
) => void

type NotifyApi = NotifyCallable & {
  show: (message: string, type?: NotifyType, duration?: number) => void
  success: (message: string, duration?: number) => void
  error: (message: string, duration?: number) => void
  info: (message: string, duration?: number) => void
}

const NotifyContext = createContext<NotifyApi | null>(null)

export function NotifyProvider({ children }: { children: React.ReactNode }) {
  const { toasts, show, dismiss } = useToast()

  const api = useMemo<NotifyApi>(() => {
    const notifyImpl = (
      messageOrType: string,
      typeOrMessage?: NotifyType | string,
      duration?: number,
    ) => {
      const looksLikeType = (value: string): value is NotifyType =>
        value === 'success' || value === 'error' || value === 'info'

      if (looksLikeType(messageOrType) && typeof typeOrMessage === 'string') {
        show(typeOrMessage, messageOrType, duration)
        return
      }

      const type = typeof typeOrMessage === 'string' && looksLikeType(typeOrMessage)
        ? typeOrMessage
        : 'info'
      show(messageOrType, type, duration)
    }

    const notify = notifyImpl as NotifyApi
    notify.show = (message, type = 'info', duration) => show(message, type, duration)
    notify.success = (message, duration = 5000) => show(message, 'success', duration)
    notify.error = (message, duration = 6500) => show(message, 'error', duration)
    notify.info = (message, duration = 5200) => show(message, 'info', duration)
    return notify
  }, [show])

  return (
    <NotifyContext.Provider value={api}>
      {children}
      <ToastContainer toasts={toasts} dismiss={dismiss} />
    </NotifyContext.Provider>
  )
}

export function useNotify() {
  const ctx = useContext(NotifyContext)
  if (!ctx) {
    throw new Error('useNotify must be used within NotifyProvider')
  }
  return ctx
}

export function useSafeNotify() {
  const ctx = useContext(NotifyContext)
  return useCallback((message: string, type: NotifyType = 'info', duration?: number) => {
    if (!ctx) return
    ctx(message, type, duration)
  }, [ctx])
}
