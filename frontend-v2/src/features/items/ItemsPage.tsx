import { useDevice } from '../../hooks/useDevice'
import DesktopItems from './DesktopItems'
import MobileItems from './MobileItems'

export default function ItemsPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileItems />
  return <DesktopItems />
}
