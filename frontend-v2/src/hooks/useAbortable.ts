import { useEffect, useRef } from 'react'

export function useAbortable() {
  const controllerRef = useRef<AbortController | null>(null)

  useEffect(() => {
    return () => {
      controllerRef.current?.abort()
    }
  }, [])

  function getSignal(): AbortSignal {
    controllerRef.current?.abort()
    controllerRef.current = new AbortController()
    return controllerRef.current.signal
  }

  function abort() {
    controllerRef.current?.abort()
    controllerRef.current = null
  }

  return { getSignal, abort }
}
