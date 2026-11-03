import { useDevice } from '../../hooks/useDevice'
import DesktopCards from './DesktopCards'
import MobileCards from './MobileCards'

export default function CardsPage({ embedded = false }: { embedded?: boolean }) {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileCards embedded={embedded} />
  return <DesktopCards embedded={embedded} />
}
