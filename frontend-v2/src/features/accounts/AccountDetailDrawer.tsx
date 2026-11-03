import { useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { X } from 'lucide-react'
import type { Account } from '../../api/types'
import type { AccountProfile } from '../../hooks/useAccountProfiles'
import { AccountDetailContent } from './AccountDetailContent'
import { accountRuntimeStatus } from './hooks/useAccountsPageState'
import { SensitiveValue, useSensitiveAvatarInitial, useSensitiveText } from '../../components/SensitiveValue'

interface AccountDetailDrawerProps {
  open: boolean
  account?: Account | null
  profile?: AccountProfile | null
  returnFocusRef?: { readonly current: HTMLElement | null }
  onClose: () => void
  onAccountUpdated?: (() => void | Promise<void>) | null
}

export function AccountDetailDrawer({
  open,
  account,
  profile,
  returnFocusRef,
  onClose,
  onAccountUpdated,
}: AccountDetailDrawerProps) {
  const closeButtonRef = useRef<HTMLButtonElement>(null)
  const maskedNick = useSensitiveText('account', account?.displayNick || account?.displayName || account?.snsNick || account?.accountNick || account?.nick || '未知账号')
  const avatarInitial = useSensitiveAvatarInitial(account?.displayNick || account?.displayName || account?.snsNick || account?.accountNick || account?.nick || account?.uid || '?')

  useEffect(() => {
    if (!open) return undefined

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [open, onClose])

  useEffect(() => {
    if (!open) return undefined

    return () => {
      const trigger = returnFocusRef?.current
      if (trigger?.isConnected) trigger.focus()
    }
  }, [open, returnFocusRef])

  useEffect(() => {
    if (!open || !account) return undefined

    const frame = window.requestAnimationFrame(() => closeButtonRef.current?.focus())
    return () => window.cancelAnimationFrame(frame)
  }, [open, account?.uid])

  if (!open || !account) return null

  const runtimeStatus = accountRuntimeStatus(account)
  const displayNick = account.displayNick
    || account.displayName
    || account.snsNick
    || account.accountNick
    || account.nick
    || '未知账号'
  const titleId = `account-detail-drawer-title-${account.uid}`

  return createPortal(
    <>
      <div
        className="account-detail-drawer-backdrop"
        role="presentation"
        onClick={onClose}
      />
      <aside
        className="account-detail-drawer"
        role="dialog"
        aria-labelledby={titleId}
      >
        <header className="account-detail-drawer-header">
          <div className="account-detail-drawer-avatar" aria-hidden="true">
            {account.avatar ? (
              <img src={account.avatar} alt="" />
            ) : (
              <span>{avatarInitial}</span>
            )}
          </div>
          <div className="account-detail-drawer-identity">
            <div className="account-detail-drawer-title-row">
              <h2 id={titleId} title={maskedNick}><SensitiveValue kind="account" value={displayNick} /></h2>
              <span
                className="account-detail-drawer-status"
                style={{ background: runtimeStatus.chipBg, color: runtimeStatus.chipText }}
                title={`运行状态：${runtimeStatus.label}`}
              >
                <span style={{ background: runtimeStatus.tone }} aria-hidden="true" />
                {runtimeStatus.label}
              </span>
            </div>
            <div className="account-detail-drawer-uid">UID: <SensitiveValue kind="uid" value={account.uid} /></div>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            className="account-detail-drawer-close"
            onClick={onClose}
            aria-label="关闭账号详情"
            title="关闭"
          >
            <X aria-hidden="true" />
          </button>
        </header>

        <div className="account-detail-drawer-body">
          <AccountDetailContent
            uid={account.uid}
            account={account}
            initialProfile={profile}
            showHeader={false}
            onAccountUpdated={onAccountUpdated}
          />
        </div>
      </aside>
    </>,
    document.body,
  )
}
