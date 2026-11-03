// @ts-nocheck
import { useEffect, useRef, useCallback } from 'react'
import { createLiveTransport } from './useLiveTransport'
import { resolveRefreshInterval } from './refreshPolicy'
import { useAuthStore } from '../../store/authStore'
import { getAuthToken } from '../../api/client'

interface UseLiveEventsOptions {
  activeUids: string[]
  connectionStates: Record<string, unknown>
  onEvent: (event: unknown) => void
  onStateChange?: (uid: string, state: string) => void
  onTransportStateChange?: (state: string) => void
  onFallbackRefresh: (opts: { background: boolean }) => void
}

export function useLiveEvents({
  activeUids,
  connectionStates,
  onEvent,
  onStateChange,
  onTransportStateChange,
  onFallbackRefresh,
}: UseLiveEventsOptions) {
  const storeAuthToken = useAuthStore((state) => state.token)
  const authToken = storeAuthToken || getAuthToken()
  const liveSourceRef = useRef(null)
  const fallbackTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const transportConnectedRef = useRef(false)
  const subscribedUidsRef = useRef<string[]>([])

  const scheduleFallbackPolling = useCallback(() => {
    const interval = resolveRefreshInterval({
      transportConnected: transportConnectedRef.current,
      activeUids,
      connectionStates,
      fallbackRefreshInterval: 30000,
      degradedRefreshInterval: 15000,
    })
    if (fallbackTimerRef.current) {
      clearInterval(fallbackTimerRef.current)
      fallbackTimerRef.current = null
    }
    if (interval) {
      fallbackTimerRef.current = setInterval(() => {
        onFallbackRefresh({ background: true })
      }, interval)
    }
  }, [activeUids, connectionStates, onFallbackRefresh])

  useEffect(() => {
    if (!authToken) {
      liveSourceRef.current?.close?.()
      liveSourceRef.current = null
      transportConnectedRef.current = false
      if (fallbackTimerRef.current) {
        clearInterval(fallbackTimerRef.current)
        fallbackTimerRef.current = null
      }
      onTransportStateChange?.('disconnected')
      return
    }
    const transport = createLiveTransport({
      authToken,
      onEvent: (event) => {
        onEvent(event)
      },
      onStateChange: (state) => {
        transportConnectedRef.current = state === 'ws'
        if (state === 'ws' && subscribedUidsRef.current.length > 0) {
          liveSourceRef.current?.subscribe?.(subscribedUidsRef.current)
        }
        onTransportStateChange?.(state)
        scheduleFallbackPolling()
      },
    })
    liveSourceRef.current = transport
    transport.connect()

    const handleVisibility = () => {
      if (document.visibilityState === 'visible') {
        onFallbackRefresh({ background: true })
      }
    }
    document.addEventListener('visibilitychange', handleVisibility)

    return () => {
      transport.close?.()
      if (fallbackTimerRef.current) clearInterval(fallbackTimerRef.current)
      document.removeEventListener('visibilitychange', handleVisibility)
    }
  }, [authToken])

  useEffect(() => {
    scheduleFallbackPolling()
  }, [scheduleFallbackPolling])

  const send = useCallback((command: unknown) => {
    liveSourceRef.current?.send?.(command)
  }, [])

  const subscribe = useCallback((uids: string[]) => {
    subscribedUidsRef.current = uids
    liveSourceRef.current?.subscribe?.(uids)
  }, [])

  return { send, subscribe, scheduleFallbackPolling }
}
