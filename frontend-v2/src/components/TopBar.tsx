import { ChevronLeft } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { useDevice } from '../hooks/useDevice'

interface TopBarProps {
  title: string
  back?: boolean | (() => void)
  right?: React.ReactNode
  subtitle?: string
}

export function TopBar({ title, back, right, subtitle }: TopBarProps) {
  const navigate = useNavigate()
  const { isMobile } = useDevice()
  const hasInteractiveChrome = Boolean(back || right)

  if (isMobile && !hasInteractiveChrome) {
    return null
  }

  function handleBack() {
    if (typeof back === 'function') {
      back()
    } else {
      navigate(-1)
    }
  }

  return (
    <header className="sticky top-0 z-30 px-4 mobile-topbar">
      <div className="flex min-h-12 items-center gap-2 py-2 mobile-topbar-inner">
        {back && (
          <button
            onClick={handleBack}
            className="mobile-icon-button w-8 h-8 max-md:w-10 max-md:h-10 -ml-2 max-md:-ml-1 flex items-center justify-center rounded-full transition-colors cursor-pointer shrink-0"
            aria-label="返回"
          >
            <ChevronLeft className="w-5 h-5 max-md:w-6 max-md:h-6" />
          </button>
        )}
        <div className="min-w-0 flex-1 mobile-topbar-copy">
          {subtitle ? <span className="mobile-topbar-subtitle">{subtitle}</span> : null}
          <h1 className="min-w-0 flex-1 text-[16px] font-semibold truncate mobile-topbar-title">
            {title}
          </h1>
        </div>
        {right ? (
          <div className="ml-auto flex min-w-0 max-w-full flex-wrap items-center justify-end gap-1 mobile-topbar-actions">
            {right}
          </div>
        ) : null}
      </div>
    </header>
  )
}
