import { accountOnlineDays } from './hooks/useAccountsPageState'

interface AccountOnlineAgeProps {
  raw: unknown
  className?: string
}

export function AccountOnlineAge({ raw, className = '' }: AccountOnlineAgeProps) {
  const days = accountOnlineDays(raw)
  const value = days == null ? '—' : String(days)
  const label = days == null ? '已在线 —' : `已在线 ${value} 天`

  return (
    <span className={`account-online-age ${className}`.trim()} title="当前账号已在线时长">
      <span className="account-online-age-calendar" aria-hidden="true">
        <span className="account-online-age-calendar-value">{value}</span>
      </span>
      <span>{label}</span>
    </span>
  )
}
