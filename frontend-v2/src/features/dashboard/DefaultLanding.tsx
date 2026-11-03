import { Navigate } from 'react-router-dom'
import { useDevice } from '../../hooks/useDevice'

export default function DefaultLanding() {
  const { isMobile, isTablet } = useDevice()
  return <Navigate to={isMobile || isTablet ? '/overview' : '/dashboard'} replace />
}
