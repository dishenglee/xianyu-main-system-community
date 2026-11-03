import { useDevice } from '../../hooks/useDevice'
import DesktopNotifications from './DesktopNotifications'
import MobileNotifications from './MobileNotifications'

export default function NotificationsPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileNotifications />
  return <DesktopNotifications />
}
