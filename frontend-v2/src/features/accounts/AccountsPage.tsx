import { useDevice } from '../../hooks/useDevice'
import DesktopAccounts from './DesktopAccounts'
import MobileAccounts from './MobileAccounts'

export default function AccountsPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileAccounts />
  return <DesktopAccounts />
}
