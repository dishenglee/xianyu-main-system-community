type Status = 'success' | 'warning' | 'danger' | 'info' | 'neutral'

const STATUS_STYLES: Record<Status, { bg: string; color: string }> = {
  success: { bg: 'var(--status-success-bg)', color: 'var(--status-success-text)' },
  warning: { bg: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' },
  danger: { bg: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' },
  info: { bg: 'var(--primary-light)', color: 'var(--primary)' },
  neutral: { bg: 'var(--status-neutral-bg, var(--surface-soft))', color: 'var(--status-neutral-text, var(--text-secondary))' },
}

interface StatusBadgeProps {
  status: Status
  label: string
  dot?: boolean
}

export function StatusBadge({ status, label, dot = true }: StatusBadgeProps) {
  const styles = STATUS_STYLES[status]
  return (
    <span
      className={`status-badge status-badge-${status} inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[12px] font-medium`}
      style={{ backgroundColor: styles.bg, color: styles.color }}
    >
      {dot && (
        <span className="w-1.5 h-1.5 rounded-full" style={{ backgroundColor: styles.color }} />
      )}
      {label}
    </span>
  )
}
