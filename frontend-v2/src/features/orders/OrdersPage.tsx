import { useDevice } from '../../hooks/useDevice'
import { DesktopOrders } from './DesktopOrders'
import { MobileOrders } from './MobileOrders'

export default function OrdersPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileOrders />
  return <DesktopOrders />
}
