import { useCallback, useEffect, useState } from 'react'
import { getAuthToken } from '../api/client'

export interface AuthScopeSnapshot {
  token: string
  key: string
}

export function useAuthScope(onChange?: () => void) {
  const [authToken, setAuthToken] = useState(() => getAuthToken())

  useEffect(() => {
    const syncToken = () => {
      setAuthToken(getAuthToken())
      onChange?.()
    }
    window.addEventListener('xy:authChanged', syncToken)
    window.addEventListener('storage', syncToken)
    return () => {
      window.removeEventListener('xy:authChanged', syncToken)
      window.removeEventListener('storage', syncToken)
    }
  }, [onChange])

  const captureScope = useCallback((key = ''): AuthScopeSnapshot => ({
    token: getAuthToken(),
    key,
  }), [])

  const isScopeCurrent = useCallback((scope: AuthScopeSnapshot, key = scope.key) => (
    getAuthToken() === scope.token && key === scope.key
  ), [])

  return { authToken, captureScope, isScopeCurrent }
}
