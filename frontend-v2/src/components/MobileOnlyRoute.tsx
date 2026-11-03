import type { ReactNode } from 'react'
import { Navigate } from 'react-router-dom'
import { useDevice } from '../hooks/useDevice'

interface MobileOnlyRouteProps {
  children: ReactNode
  fallback: string
}

export function MobileOnlyRoute({ children, fallback }: MobileOnlyRouteProps) {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <>{children}</>
  return <Navigate to={fallback} replace />
}
