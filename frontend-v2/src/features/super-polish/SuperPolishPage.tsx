import { useDevice } from '../../hooks/useDevice'
import DesktopSuperPolish from './DesktopSuperPolish'
import MobileSuperPolish from './MobileSuperPolish'

export default function SuperPolishPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileSuperPolish />
  return <DesktopSuperPolish />
}
