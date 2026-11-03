// @ts-nocheck
// 移动端账号操作弹层 — 替代桌面端行内 Action 按钮 + 行展开。
// 包含：发布地 / 详情（跳转 /accounts/:uid）/ 重新扫码 / 三个 toggle。
import {
  ChevronRight, Power,
} from 'lucide-react'
import { BottomSheet } from '../../../components/Modal'
import {
  accountRuntimeStatus,
  accountRuntimeReason,
  formatLiveConnectionLabel,
  formatRefreshedAt,
} from '../hooks/useAccountsPageState'
import { AccountOnlineAge } from '../AccountOnlineAge'
import { getAccountActions, getAccountAutoReplyStatus, getAccountAutoShipStatus, getAccountRestrictionState } from '../accountUi'
import { SensitiveValue, useSensitiveAvatarInitial, useSensitiveText } from '../../../components/SensitiveValue'

interface Props {
  open: boolean
  account: any | null
  saving?: boolean
  onClose: () => void
  onOpenLocation: (uid: string, nick?: string) => void
  onOpenDetail: (uid: string) => void
  onRefreshAccount: (uid: string, nick?: string) => void
  onDeleteAccount: (uid: string, nick?: string) => void
  onRelogin: (uid: string, nick?: string) => void
  onToggle: (uid: string, key: string, value: boolean) => void
}

export function MobileAccountSheet({
  open, account, saving, onClose,
  onOpenLocation, onOpenDetail, onRefreshAccount, onDeleteAccount, onRelogin, onToggle,
}: Props) {
  const sheetTitle = useSensitiveText(account?.nick ? 'account' : 'uid', account?.nick || account?.uid)
  const avatarInitial = useSensitiveAvatarInitial(account?.nick || account?.uid || '?')
  if (!account) return null
  const runtime = accountRuntimeStatus(account)
  const reason = accountRuntimeReason(account)
  const autoReplyStatus = getAccountAutoReplyStatus(account)
  const autoShipStatus = getAccountAutoShipStatus(account)
  const restriction = getAccountRestrictionState(account)
  const accountActions = getAccountActions({
    account,
    onRefreshAccount,
    onOpenLocation,
    onOpenDetail,
    onDeleteAccount,
    detailLabel: '账号详情',
  })

  const wrap = (fn: () => void) => () => { onClose(); fn() }

  return (
    <BottomSheet open={open} onClose={onClose} title={sheetTitle}>
      {/* 账号头部 */}
      <div className="flex items-center gap-3 mb-4 pb-3 border-b" style={{ borderColor: 'var(--border-light)' }}>
        {account.avatar ? (
          <img src={account.avatar} alt="" className="w-12 h-12 rounded-full object-cover" style={{ border: '1px solid var(--glass-border)' }} />
        ) : (
          <div className="liquid-avatar w-12 h-12 rounded-full flex items-center justify-center text-[18px] font-semibold">
            {avatarInitial}
          </div>
        )}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            <span className="text-[15px] font-semibold truncate" style={{ color: 'var(--text-main)' }}>
              <SensitiveValue kind="account" value={account.nick || '未知'} />
            </span>
            <span
              className="text-[10px] px-1.5 py-[1px] rounded shrink-0"
              style={{ background: runtime.chipBg, color: runtime.chipText }}
            >
              {runtime.label}
            </span>
          </div>
          <div className="text-[11px] mt-0.5 font-mono truncate" style={{ color: 'var(--text-muted)' }}>
            <SensitiveValue kind="uid" value={account.uid} />
          </div>
          <div className="text-[11px] mt-1 flex items-center gap-2" style={{ color: 'var(--text-secondary)' }}>
            <AccountOnlineAge raw={account.added_at ?? account.addedAt ?? account.bound_at ?? account.boundAt} />
            <span aria-hidden style={{ color: 'var(--text-tertiary)' }}>·</span>
            <span>🍪 {formatRefreshedAt(account.cookie_refreshed_at ?? account.cookieRefreshedAt)}</span>
          </div>
          <div className="text-[11px] mt-1" style={{ color: account.liveStatus === 'connected' || account.live_status === 'connected' ? 'var(--success)' : 'var(--text-muted)' }}>
            {formatLiveConnectionLabel(account)}
          </div>
        </div>
      </div>

      {/* 异常时的重新扫码入口 */}
      {runtime.key === 'session_error' || runtime.key === 'offline' ? (
        <button
          onClick={wrap(() => onRelogin(account.uid, account.nick))}
          disabled={restriction.deleteOnly}
          className="w-full flex items-center justify-between px-4 py-3 mb-3 rounded-xl active:opacity-80 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
          style={{ background: 'var(--status-danger-bg)', border: '1px solid color-mix(in srgb, var(--danger) 20%, transparent)' }}
        >
          <span className="flex items-center gap-2.5 text-[13.5px] font-medium" style={{ color: 'var(--status-danger-text)' }}>
            <Power className="w-4 h-4" />
            {reason || '重新扫码登录'}
          </span>
          <ChevronRight className="w-4 h-4" style={{ color: 'var(--status-danger-text)' }} />
        </button>
      ) : null}

      {restriction.deleteOnly ? (
        <div
          className="rounded-xl px-4 py-3 mb-3 text-[13px]"
          style={{
            background: 'var(--status-danger-bg)',
            border: '1px solid color-mix(in srgb, var(--danger) 18%, transparent)',
            color: 'var(--status-danger-text)',
          }}
        >
          {restriction.restrictionMessage}
        </div>
      ) : null}

      {/* 自动回复与自动发货只读配置状态 */}
      <div className="rounded-xl overflow-hidden mb-3" style={{ background: 'var(--surface-soft)' }}>
        <ConfigurationStatusRow label="自动回复" description="在「自动回复」页管理配置" status={autoReplyStatus} />
        <AutoShipStatusRow status={autoShipStatus} last />
      </div>

      {/* 操作行 */}
      <div className="rounded-xl overflow-hidden" style={{ background: 'var(--surface-soft)' }}>
        {accountActions.map((action, index) => (
          <ActionRow
            key={action.key}
            icon={action.icon}
            color={action.color}
            label={action.label}
            onClick={wrap(action.onClick)}
            disabled={action.disabled}
            last={index === accountActions.length - 1}
          />
        ))}
      </div>
    </BottomSheet>
  )
}

function AutoShipStatusRow({ status, last }: { status: { enabled: boolean; label: string; mismatchCount?: number }; last?: boolean }) {
  return (
    <div style={last ? undefined : { borderBottom: '1px solid var(--control-border)' }}>
      <ConfigurationStatusRow label="自动发货" description="在「自动发货」页配置商品后自动开启" status={status} />
      {status.mismatchCount ? (
        <a
          href="/delivery"
          className="mx-4 mb-3 flex items-center justify-between rounded-lg px-3 py-2 text-[12px] font-medium"
          style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
          title="前往自动发货处理待发订单商品未匹配的配置"
        >
          <span>商品不匹配（{status.mismatchCount}笔）</span>
          <span>去处理</span>
        </a>
      ) : null}
    </div>
  )
}

function ConfigurationStatusRow({ label, description, status, last }: any) {
  return (
    <div
      className="flex items-center justify-between px-4 py-3"
      style={last ? undefined : { borderBottom: '1px solid var(--control-border)' }}
    >
      <div className="min-w-0">
        <div className="text-[14px] font-medium" style={{ color: 'var(--text-main)' }}>{label}</div>
        <div className="mt-0.5 text-[11px]" style={{ color: 'var(--text-muted)' }}>{description}</div>
      </div>
      <span
        className="shrink-0 rounded-full px-2.5 py-1 text-[11px] font-semibold"
        style={{
          background: status.enabled ? 'var(--status-info-bg)' : 'var(--surface-card)',
          color: status.enabled ? 'var(--status-info-text)' : 'var(--text-muted)',
        }}
      >
        {status.enabled ? '已配置' : '未配置'}
      </span>
    </div>
  )
}

function ActionRow({ icon: Icon, color, label, onClick, disabled, last }: any) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="w-full flex items-center gap-3 px-4 py-3 cursor-pointer active:bg-black/[0.04] disabled:opacity-40 disabled:cursor-not-allowed"
      style={{ borderBottom: last ? 'none' : '1px solid var(--control-border)' }}
    >
      <div className="w-7 h-7 rounded-full flex items-center justify-center" style={{ backgroundColor: `${color}1A` }}>
        <Icon className="w-4 h-4" style={{ color }} />
      </div>
      <span className="flex-1 text-left text-[14px]" style={{ color: 'var(--text-main)' }}>{label}</span>
      <ChevronRight className="w-4 h-4" style={{ color: 'var(--text-tertiary)' }} />
    </button>
  )
}
