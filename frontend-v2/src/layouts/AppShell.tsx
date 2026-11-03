import { useDevice } from '../hooks/useDevice'
import { SensitiveScopeProvider } from '../components/SensitiveValue'
import { DesktopLayout } from './DesktopLayout'
import { MobileLayout } from './MobileLayout'

export function AppShell() {
  const { isMobile, isTablet } = useDevice()

  const useMobileLayout = isMobile || isTablet

  return (
    <SensitiveScopeProvider>
      {useMobileLayout ? <MobileLayout /> : <DesktopLayout />}
    </SensitiveScopeProvider>
  )
}
