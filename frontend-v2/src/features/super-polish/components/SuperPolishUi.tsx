import type { ReactNode } from 'react'
import { AlertCircle } from 'lucide-react'
import { EmptyState } from '../../../components/EmptyState'
import { formatJson } from '../model'

export type SuperPolishTone = 'neutral' | 'info' | 'success' | 'warning' | 'danger'

function toneStyles(tone: SuperPolishTone) {
  switch (tone) {
    case 'info':
      return {
        background: 'color-mix(in srgb, var(--primary) 10%, var(--surface-soft))',
        border: 'color-mix(in srgb, var(--primary) 18%, transparent)',
        text: 'var(--primary)',
      }
    case 'success':
      return {
        background: 'color-mix(in srgb, var(--success) 11%, var(--surface-soft))',
        border: 'color-mix(in srgb, var(--success) 18%, transparent)',
        text: 'var(--status-success-text)',
      }
    case 'warning':
      return {
        background: 'color-mix(in srgb, var(--warning) 12%, var(--surface-soft))',
        border: 'color-mix(in srgb, var(--warning) 18%, transparent)',
        text: 'var(--status-warning-text)',
      }
    case 'danger':
      return {
        background: 'color-mix(in srgb, var(--danger) 10%, var(--surface-soft))',
        border: 'color-mix(in srgb, var(--danger) 18%, transparent)',
        text: 'var(--danger)',
      }
    case 'neutral':
    default:
      return {
        background: 'var(--surface-soft)',
        border: 'var(--control-border)',
        text: 'var(--text-main)',
      }
  }
}

export function SuperPolishSection({
  title,
  subtitle,
  actions,
  children,
}: {
  title: string
  subtitle?: string
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <section className="surface-card rounded-[20px] border p-5" style={{ borderColor: 'var(--glass-border)' }}>
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[16px] font-semibold" style={{ color: 'var(--text-main)' }}>{title}</h2>
          {subtitle ? (
            <p className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>{subtitle}</p>
          ) : null}
        </div>
        {actions ? <div className="shrink-0">{actions}</div> : null}
      </div>
      {children}
    </section>
  )
}

export function SuperPolishMetricGrid({
  items,
  mobileColumns = 1,
}: {
  items: Array<{ label: string; value: string; helper?: string; tone?: SuperPolishTone }>
  mobileColumns?: 1 | 2 | 3
}) {
  const mobileGridClass = mobileColumns === 3
    ? 'grid-cols-3'
    : mobileColumns === 2
      ? 'grid-cols-2'
      : 'grid-cols-1'
  return (
    <div className={`grid ${mobileGridClass} gap-2 md:gap-3 md:grid-cols-2 xl:grid-cols-4`}>
      {items.map((item) => (
        <div
          key={item.label}
          className="min-w-0 rounded-[16px] px-3 py-2.5 md:rounded-[18px] md:px-4 md:py-3"
          style={{
            background: toneStyles(item.tone ?? 'neutral').background,
            border: `1px solid ${toneStyles(item.tone ?? 'neutral').border}`,
          }}
        >
          <div className="truncate text-[10px] md:text-[11px]" style={{ color: 'var(--text-secondary)' }}>{item.label}</div>
          <div
            className="mt-1 truncate text-[17px] font-semibold leading-none md:text-[20px]"
            style={{ color: toneStyles(item.tone ?? 'neutral').text }}
          >
            {item.value}
          </div>
          {item.helper ? (
            <div className="mt-2 text-[11px] leading-4" style={{ color: 'var(--text-muted)' }}>
              {item.helper}
            </div>
          ) : null}
        </div>
      ))}
    </div>
  )
}

export function SuperPolishError({ message }: { message: string }) {
  if (!message) return null
  return (
    <div
      className="mb-4 flex items-start gap-2 rounded-[16px] px-4 py-3 text-[12px]"
      style={{
        background: 'color-mix(in srgb, var(--danger) 10%, var(--surface-card))',
        border: '1px solid color-mix(in srgb, var(--danger) 18%, transparent)',
        color: 'var(--danger)',
      }}
    >
      <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
      <span>{message}</span>
    </div>
  )
}

export function SuperPolishEmpty({
  title,
  description,
}: {
  title: string
  description?: string
}) {
  return <EmptyState title={title} description={description} />
}

export function SuperPolishBadge({
  children,
  tone = 'neutral',
}: {
  children: ReactNode
  tone?: SuperPolishTone
}) {
  const palette = toneStyles(tone)
  return (
    <span
      className="inline-flex items-center whitespace-nowrap rounded-full px-2.5 py-1 text-[11px] font-medium"
      style={{
        background: palette.background,
        border: `1px solid ${palette.border}`,
        color: palette.text,
      }}
    >
      {children}
    </span>
  )
}

export function SuperPolishKeyValueList({
  items,
}: {
  items: Array<{ label: string; value: string; tone?: SuperPolishTone }>
}) {
  return (
    <div className="space-y-2">
      {items.map((item) => (
        <div key={`${item.label}-${item.value}`} className="flex items-center justify-between gap-3 text-[12px]">
          <span style={{ color: 'var(--text-secondary)' }}>{item.label}</span>
          <span style={{ color: toneStyles(item.tone ?? 'neutral').text }} className="text-right font-medium">
            {item.value}
          </span>
        </div>
      ))}
    </div>
  )
}

export function SuperPolishField({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  return (
    <label className="min-w-0 space-y-1.5">
      <div className="truncate text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>{label}</div>
      {children}
    </label>
  )
}

export function SuperPolishInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className="min-w-0 w-full rounded-[14px] border px-2 py-2 text-[12px] outline-none transition focus:ring-2 md:px-3 md:text-[13px]"
      style={{
        borderColor: 'var(--control-border)',
        background: 'var(--surface-card)',
        color: 'var(--text-main)',
      }}
    />
  )
}

export function SuperPolishTextarea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className="w-full rounded-[14px] border px-3 py-2 text-[12px] outline-none transition focus:ring-2"
      style={{
        borderColor: 'var(--control-border)',
        background: 'var(--surface-card)',
        color: 'var(--text-main)',
        minHeight: 140,
      }}
    />
  )
}

export function SuperPolishSelect(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className="min-w-0 w-full rounded-[14px] border px-2 py-2 text-[12px] outline-none transition focus:ring-2 md:px-3 md:text-[13px]"
      style={{
        borderColor: 'var(--control-border)',
        background: 'var(--surface-card)',
        color: 'var(--text-main)',
      }}
    />
  )
}

export function SuperPolishJsonDetails({
  title,
  data,
  defaultOpen = false,
}: {
  title: string
  data: unknown
  defaultOpen?: boolean
}) {
  return (
    <details
      className="rounded-[16px] border"
      style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
      open={defaultOpen}
    >
      <summary className="cursor-pointer px-4 py-3 text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
        {title}
      </summary>
      <pre
        className="overflow-auto border-t px-4 py-3 text-[11px] leading-5"
        style={{
          borderColor: 'var(--control-border)',
          color: 'var(--text-secondary)',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
        }}
      >
        {formatJson(data) || '{}'}
      </pre>
    </details>
  )
}
