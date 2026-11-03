import { useCallback, useEffect, useState } from 'react'
import { assertSuperPolishRequestSuccess } from '../mtop'
import { useSuperPolishScope } from './useSuperPolishScope'

interface RequestState<T> {
  data: T | null
  loading: boolean
  error: string
  reload: () => void
}

interface RequestSnapshot<T> {
  key: string
  data: T | null
  error: string
}

function serializeDep(value: unknown) {
  if (value === null) return 'null'
  switch (typeof value) {
    case 'string':
    case 'number':
    case 'boolean':
    case 'bigint':
    case 'undefined':
      return `${typeof value}:${String(value)}`
    default:
      try {
        return `${typeof value}:${JSON.stringify(value)}`
      } catch {
        return `${typeof value}:${Object.prototype.toString.call(value)}`
      }
  }
}

export function useSuperPolishRequest<T>(
  loader: (uid: string) => Promise<T>,
  deps: unknown[] = [],
  enabled = true,
): RequestState<T> {
  const { uid } = useSuperPolishScope()
  const [snapshot, setSnapshot] = useState<RequestSnapshot<T>>({
    key: '',
    data: null,
    error: '',
  })
  const [reloadTick, setReloadTick] = useState(0)

  const reload = useCallback(() => {
    setReloadTick((value) => value + 1)
  }, [])

  const requestKey = [uid ?? '', enabled ? '1' : '0', String(reloadTick), ...deps.map(serializeDep)].join('||')

  useEffect(() => {
    if (!uid || !enabled) return

    let active = true

    loader(uid)
      .then((result) => {
        if (!active) return
        const resolved = assertSuperPolishRequestSuccess(result)
        setSnapshot({
          key: requestKey,
          data: resolved,
          error: '',
        })
      })
      .catch((reason: unknown) => {
        if (!active) return
        setSnapshot({
          key: requestKey,
          data: null,
          error: reason instanceof Error ? reason.message : '加载失败',
        })
      })

    return () => {
      active = false
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uid, enabled, requestKey, reloadTick, ...deps])

  const activeRequest = Boolean(uid && enabled)
  const matchedSnapshot = activeRequest && snapshot.key === requestKey

  return {
    data: matchedSnapshot ? snapshot.data : null,
    loading: activeRequest && !matchedSnapshot,
    error: matchedSnapshot ? snapshot.error : '',
    reload,
  }
}
