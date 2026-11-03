import { useDevice } from '../../hooks/useDevice'
import DesktopSearch from './DesktopSearch'
import MobileSearch from './MobileSearch'

export default function SearchPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileSearch />
  return <DesktopSearch />
}
