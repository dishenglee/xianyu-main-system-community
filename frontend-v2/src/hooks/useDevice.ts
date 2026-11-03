import { useState, useEffect } from 'react'

export type DeviceType = 'mobile' | 'tablet' | 'desktop'

export function getDeviceTypeForWidth(w: number): DeviceType {
  if (w < 768) return 'mobile'
  if (w < 1024) return 'tablet'
  return 'desktop'
}

function getDeviceType(): DeviceType {
  if (typeof window === 'undefined') return 'desktop'
  return getDeviceTypeForWidth(window.innerWidth)
}

export function useDevice() {
  const [device, setDevice] = useState<DeviceType>(getDeviceType)

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return

    const mobileQuery = window.matchMedia('(max-width: 767px)')
    const tabletQuery = window.matchMedia('(min-width: 768px) and (max-width: 1023px)')

    function update() {
      setDevice(getDeviceType())
    }

    mobileQuery.addEventListener('change', update)
    tabletQuery.addEventListener('change', update)
    return () => {
      mobileQuery.removeEventListener('change', update)
      tabletQuery.removeEventListener('change', update)
    }
  }, [])

  return {
    device,
    isMobile: device === 'mobile',
    isTablet: device === 'tablet',
    isDesktop: device === 'desktop',
  }
}
