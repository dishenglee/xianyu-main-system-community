import { useDevice } from '../../hooks/useDevice'
import DesktopPromotion from './DesktopPromotion'
import MobilePromotion from './MobilePromotion'

export default function PromotionPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobilePromotion />
  return <DesktopPromotion />
}
