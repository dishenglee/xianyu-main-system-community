// @ts-nocheck

function formatFishShopLevel(level?: string) {
  const raw = String(level || '').trim()
  if (!raw) return ''
  return raw.replace(/^lv\./i, 'L').replace(/\s+/g, '')
}

interface FishShopBadgeProps {
  level?: string
  className?: string
}

export function FishShopBadge({ level, className = '' }: FishShopBadgeProps) {
  const displayLevel = formatFishShopLevel(level)
  const levelText = displayLevel || '已开通'

  return (
    <span
      className={`inline-flex items-center gap-0.5 rounded-full px-[3px] py-[2px] shrink-0 ${className}`.trim()}
      style={{
        background: 'linear-gradient(180deg, var(--surface-card-strong) 0%, rgba(244, 255, 253, 0.90) 100%)',
        border: '1px solid rgba(11, 191, 165, 0.24)',
        boxShadow: '0 1px 2px rgba(17, 24, 39, 0.06)',
      }}
    >
      <span
        className="rounded-full px-1.5 py-[1px] text-[10px] font-semibold leading-none"
        style={{
          background: 'linear-gradient(135deg, #2DD4BF 0%, #0EA5E9 100%)',
          color: 'var(--liquid-highlight)',
        }}
      >
        鱼小铺
      </span>
      <span
        className="rounded-full px-1.5 py-[1px] text-[10px] font-semibold leading-none tracking-[0.01em]"
        style={{
          background: 'var(--status-warning-bg)',
          color: 'var(--status-warning-text)',
        }}
      >
        {levelText}
      </span>
    </span>
  )
}

export function hasFishShop(profile?: any) {
  return profile?.fishShopEnabled === true
}
