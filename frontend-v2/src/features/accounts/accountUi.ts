import {
  ChevronRight,
  MapPin,
  RefreshCw,
  Trash2,
} from 'lucide-react'
import { isDeleteOnlyAccount } from '../../utils/accountAccess'

export const ACCOUNT_DELETE_ONLY_MESSAGE = '此账号已被其他用户提交绑定，只能进行删除操作'

const ACCOUNT_CAPABILITY_FIELD_ALIASES = {
  autoReply: ['autoReply', 'auto_reply_enabled'],
  autoShip: ['autoShip', 'auto_ship_enabled'],
  aiEnabled: ['aiEnabled', 'ai_enabled'],
} as const

const ACCOUNT_RESTRICTION_FIELD_ALIASES = {
  deleteOnly: ['deleteOnly', 'delete_only'],
  remoteBindingState: ['remoteBindingState', 'remote_binding_state'],
  remoteBindingMessage: ['remoteBindingMessage', 'remote_binding_message'],
} as const

// Interactive account switches. Auto-ship is config-driven on the delivery page,
// so it is intentionally not a toggle here.
export function getAccountCapabilityValue(account: Record<string, any> | null | undefined, key: keyof typeof ACCOUNT_CAPABILITY_FIELD_ALIASES) {
  if (!account) return false
  const replyMode = typeof account.replyMode === 'string'
    ? String(account.replyMode).trim()
    : typeof account.reply_mode === 'string'
      ? String(account.reply_mode).trim()
      : ''
  if (key === 'autoReply' && replyMode) return replyMode !== 'off'
  if (key === 'aiEnabled' && replyMode) return replyMode === 'ai'
  for (const field of ACCOUNT_CAPABILITY_FIELD_ALIASES[key]) {
    if (field in account) return Boolean(account[field])
  }
  return false
}

export function getAccountAutoReplyStatus(account: Record<string, any> | null | undefined) {
  const enabled = getAccountCapabilityValue(account, 'autoReply')
  return {
    enabled,
    label: enabled ? '自动回复·已配置' : '自动回复·未配置',
    shortLabel: enabled ? '已配置' : '未配置',
    tone: enabled ? 'info' : 'neutral',
  }
}

/** Read-only auto-delivery status derived from the account flag (kept in sync with configs). */
export function getAccountAutoShipStatus(account: Record<string, any> | null | undefined) {
  const enabled = getAccountCapabilityValue(account, 'autoShip')
  const mismatchCount = Math.max(0, Number(
    account?.pendingDeliveryMismatchCount ?? account?.pending_delivery_mismatch_count ?? 0,
  ) || 0)
  return {
    enabled,
    label: enabled ? '自动发货·已配置' : '自动发货·未配置',
    shortLabel: enabled ? '已配置' : '未配置',
    tone: enabled ? 'info' : 'neutral',
    mismatchCount,
  }
}

function getAccountFieldValue(account: Record<string, any> | null | undefined, fields: readonly string[]) {
  if (!account) return undefined
  for (const field of fields) {
    if (field in account) return account[field]
  }
  return undefined
}

export function getAccountRestrictionState(account: Record<string, any> | null | undefined) {
  const remoteBindingState = getAccountFieldValue(account, ACCOUNT_RESTRICTION_FIELD_ALIASES.remoteBindingState)
  const remoteBindingMessage = getAccountFieldValue(account, ACCOUNT_RESTRICTION_FIELD_ALIASES.remoteBindingMessage)
  const deleteOnly = isDeleteOnlyAccount(account)
  const restrictionMessage = deleteOnly
    ? String(remoteBindingMessage || ACCOUNT_DELETE_ONLY_MESSAGE)
    : ''

  return {
    deleteOnly,
    remoteBindingState: typeof remoteBindingState === 'string' ? remoteBindingState : '',
    remoteBindingMessage: typeof remoteBindingMessage === 'string' ? remoteBindingMessage : '',
    restrictionMessage,
  }
}

type AccountActionArgs = {
  uid: string
  nick?: string
}

type AccountActionHandlers = {
  onRefreshAccount: (uid: string, nick?: string) => void
  onOpenLocation: (uid: string, nick?: string) => void
  onOpenDetail: (uid: string) => void
  onDeleteAccount: (uid: string, nick?: string) => void
}

type AccountActionOptions = {
  account: Record<string, any>
  detailLabel?: string
} & AccountActionHandlers

export function getAccountActions({
  account,
  onRefreshAccount,
  onOpenLocation,
  onOpenDetail,
  onDeleteAccount,
  detailLabel = '详情',
}: AccountActionOptions) {
  const args: AccountActionArgs = { uid: account.uid, nick: account.nick }
  const { deleteOnly } = getAccountRestrictionState(account)
  return [
    {
      key: 'refresh',
      label: '单独刷新',
      shortLabel: '刷新',
      icon: RefreshCw,
      color: 'var(--primary)',
      danger: false,
      disabled: deleteOnly,
      onClick: () => onRefreshAccount(args.uid, args.nick),
    },
    {
      key: 'location',
      label: '设置发布地',
      shortLabel: '发布地',
      icon: MapPin,
      color: 'var(--primary)',
      danger: false,
      disabled: deleteOnly,
      onClick: () => onOpenLocation(args.uid, args.nick),
    },
    {
      key: 'detail',
      label: detailLabel,
      shortLabel: detailLabel,
      icon: ChevronRight,
      color: 'var(--success)',
      danger: false,
      disabled: deleteOnly,
      onClick: () => onOpenDetail(args.uid),
    },
    {
      key: 'delete',
      label: '删除账号',
      shortLabel: '删除',
      icon: Trash2,
      color: 'var(--danger)',
      danger: true,
      disabled: false,
      onClick: () => onDeleteAccount(args.uid, args.nick),
    },
  ]
}
