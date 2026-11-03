import type { LucideProps } from 'lucide-react'
import { useDevice } from '../hooks/useDevice'

interface EmptyStateProps {
  icon?: React.ComponentType<LucideProps>
  title: string
  description?: string
  action?: React.ReactNode
}

export function EmptyState({ icon: Icon, title, description, action }: EmptyStateProps) {
  const { isMobile } = useDevice()

  return (
    <div className={isMobile ? 'mobile-empty-shell' : 'flex flex-col items-center justify-center py-16 px-6 text-center'}>
      <div className={isMobile ? 'mobile-surface-card w-full max-w-[440px] px-6 py-8 text-center' : 'contents'}>
      {Icon && (
        <div
          className={isMobile ? 'mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-[20px]' : 'w-14 h-14 rounded-2xl flex items-center justify-center mb-4'}
          style={{ backgroundColor: isMobile ? 'rgba(255, 255, 255, 0.74)' : 'var(--surface-soft)' }}
        >
          <Icon className="w-7 h-7" style={{ color: isMobile ? 'var(--primary)' : 'var(--text-muted)' }} />
        </div>
      )}
      <p className={isMobile ? 'mb-1 text-[18px] font-semibold' : 'text-[16px] font-semibold mb-1'} style={{ color: 'var(--text-main)' }}>{title}</p>
      {description && (
        <p className={isMobile ? 'mx-auto max-w-xs text-[13px] leading-6' : 'text-[14px] max-w-xs'} style={{ color: 'var(--text-secondary)' }}>{description}</p>
      )}
      {action && <div className="mt-4">{action}</div>}
      </div>
    </div>
  )
}
