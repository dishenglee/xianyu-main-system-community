import { SuperPolishBadge } from '../../components/SuperPolishUi'
import type { AreaTargetSupportSummary } from '../areaTargetSupport'

export function AreaTargetSupportNotice({
  summary,
  subject,
}: {
  summary: AreaTargetSupportSummary
  subject: string
}) {
  if (summary.totalCount === 0) return null

  if (!summary.available) {
    return (
      <div
        className="rounded-[18px] border px-4 py-4 text-[12px] leading-5"
        style={{
          borderColor: 'var(--control-border)',
          background: 'var(--surface-soft)',
          color: 'var(--text-secondary)',
        }}
      >
        {subject}里的 {summary.totalCount} 个宝贝都不支持城市定向，本次只能按全部地域投放。
      </div>
    )
  }

  if (!summary.partiallySupported) return null

  return (
    <div
      className="rounded-[18px] border px-4 py-4 text-[12px] leading-5"
      style={{
        borderColor: 'color-mix(in srgb, var(--status-info-text) 22%, var(--control-border))',
        background: 'color-mix(in srgb, var(--status-info-bg) 68%, white)',
        color: 'var(--text-secondary)',
      }}
    >
      <div className="font-semibold" style={{ color: 'var(--text-main)' }}>
        支持城市定向 {summary.supportedCount}/{summary.totalCount}
      </div>
      <div className="mt-1">支持定向的宝贝：</div>
      <div className="mt-2 flex flex-wrap gap-2">
        {summary.supportedTitles.map((title) => (
          <SuperPolishBadge key={title} tone="info">
            {title}
          </SuperPolishBadge>
        ))}
      </div>
      <div className="mt-2">
        其余 {summary.unsupportedCount} 个宝贝仍按全部地域投放。
      </div>
    </div>
  )
}
