import { capabilityLabels, filterEnabledAccounts, getAccountUid, getCapabilityValue, nextCapabilitySelection, sanitizeSelectableUids, sanitizeSelectedUids } from './capabilities'
import type { CapabilityAccount, CapabilityKey } from './capabilities'
import { useDevice } from '../../hooks/useDevice'
import { filterBusinessAccounts } from '../../utils/accountAccess'

type AccountCapabilityPickerProps<T extends CapabilityAccount = CapabilityAccount> = {
  accounts: T[]
  capability: CapabilityKey
  selectedUids: string[]
  onChange: (selectedUids: string[]) => void
  title?: string
  description?: string
  disabled?: boolean
  allowEnableDisabledAccounts?: boolean
  singleSelect?: boolean
  disabledAccountHint?: string
  enableDisabledAccountHint?: string
  actionBar?: {
    savedCount: number
    draftCount: number
    dirty: boolean
    saving?: boolean
    onSave: () => void
    onCancel: () => void
    saveLabel?: string
    cancelLabel?: string
  }
}

export { sanitizeSelectableUids, sanitizeSelectedUids }
export type { CapabilityKey }

export function AccountCapabilityPicker<T extends CapabilityAccount = CapabilityAccount>({
  accounts,
  capability,
  selectedUids,
  onChange,
  title,
  description,
  disabled = false,
  allowEnableDisabledAccounts = false,
  singleSelect = false,
  disabledAccountHint = '账号管理未开启',
  enableDisabledAccountHint = '',
  actionBar,
}: AccountCapabilityPickerProps<T>) {
  const { isMobile } = useDevice()
  const activeAccounts = filterBusinessAccounts(accounts)
  const enabledAccounts = filterEnabledAccounts(activeAccounts, capability)
  const selectableAccounts = allowEnableDisabledAccounts ? activeAccounts : enabledAccounts
  const selectableUidSet: Set<string> = new Set(selectableAccounts.map(getAccountUid).filter((uid): uid is string => Boolean(uid)))
  const selectedSelectableUids = sanitizeSelectableUids(selectedUids, activeAccounts, capability, { allowEnableDisabledAccounts })
  const selectedSet = new Set(selectedSelectableUids)
  const selectableUids = Array.from(selectableUidSet)
  const allSelectableSelected = selectableUidSet.size > 0 && selectableUids.every((uid) => selectedSet.has(uid))
  const label = capabilityLabels[capability]

  const updateSelected = (uid: string, checked: boolean) => {
    if (disabled || !selectableUidSet.has(uid)) return

    onChange(nextCapabilitySelection({ currentUids: selectedSelectableUids, uid, checked, singleSelect }))
  }

  const selectAllEnabled = () => {
    if (disabled) return
    onChange(selectableUids)
  }

  const clearSelected = () => {
    if (disabled) return
    onChange([])
  }

  return (
    <section className={`${isMobile ? 'mobile-surface-card p-4' : 'card p-4 md:p-5'} space-y-4 ${disabled ? 'opacity-70' : ''}`}>
      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
        <div className={isMobile ? 'mobile-section-copy' : 'space-y-1'}>
          {isMobile ? <span className="mobile-section-kicker">账号范围</span> : null}
          <h3 className={isMobile ? 'mobile-section-title text-[1.15rem]' : 'text-[15px] font-semibold'} style={{ color: isMobile ? undefined : 'var(--text-primary)' }}>
            {title || `${label}账号`}
          </h3>
          {description && (
            <p className={isMobile ? 'mobile-section-note' : 'text-[12px] leading-5'} style={{ color: isMobile ? undefined : 'var(--text-secondary)' }}>
              {description}
            </p>
          )}
        </div>
        <div className={isMobile ? 'mobile-toolbar-scroll' : 'flex items-center gap-2'}>
          <button
            type="button"
            className="btn-secondary btn-sm"
            disabled={disabled || singleSelect || selectableAccounts.length === 0 || allSelectableSelected}
            onClick={selectAllEnabled}
          >
            {allowEnableDisabledAccounts ? '全选账号' : '全选可用账号'}
          </button>
          <button
            type="button"
            className="btn-ghost btn-sm"
            disabled={disabled || selectedSelectableUids.length === 0}
            onClick={clearSelected}
          >
            清空
          </button>
        </div>
      </div>

      {activeAccounts.length === 0 ? (
        <div className={isMobile ? 'mobile-divider-note' : 'rounded-xl border border-dashed p-4 text-[13px]'} style={isMobile ? undefined : { color: 'var(--text-secondary)' }}>
          暂无 active 状态账号
        </div>
      ) : (
        <div className={isMobile ? 'grid gap-2 grid-cols-2 sm:grid-cols-3' : 'grid gap-2 md:grid-cols-5'}>
          {activeAccounts.map((account) => {
            const uid = getAccountUid(account)
            const enabled = getCapabilityValue(account, capability)
            const selectable = Boolean(uid && (enabled || allowEnableDisabledAccounts))
            const checkboxDisabled = disabled || !selectable
            const displayChecked = Boolean(uid && selectedSet.has(uid) && selectable)
            const accountHint = allowEnableDisabledAccounts ? enableDisabledAccountHint : disabledAccountHint

            return (
              <label
                key={uid || String(account.nick || activeAccounts.indexOf(account))}
                className={`${isMobile ? 'mobile-soft-card' : 'input'} flex cursor-pointer items-center gap-3 px-4 py-3 ${checkboxDisabled ? 'opacity-60' : ''}`}
              >
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-2">
                    <span className="block truncate text-[13px] font-medium" style={{ color: 'var(--text-primary)' }}>
                      {String(account.nick || account.uid)}
                    </span>
                    <input
                      type="checkbox"
                      className="h-4 w-4 shrink-0 rounded accent-[var(--primary)]"
                      checked={displayChecked}
                      disabled={checkboxDisabled}
                      onChange={(event) => uid && updateSelected(uid, event.target.checked)}
                    />
                  </span>
                  {!enabled && accountHint && (
                    <span className="mt-0.5 block text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      {accountHint}
                    </span>
                  )}
                </span>
              </label>
            )
          })}
        </div>
      )}

      {actionBar && (
        <div
          className={isMobile ? 'mobile-soft-card flex flex-col gap-3 px-4 py-4 md:flex-row md:items-center md:justify-between' : 'flex flex-col gap-3 rounded-2xl border px-3 py-3 md:flex-row md:items-center md:justify-between'}
          style={isMobile ? undefined : { borderColor: 'var(--border-light)', backgroundColor: 'var(--primary-light)' }}
        >
          <div className="space-y-1">
            <div className="text-[12px] font-medium" style={{ color: actionBar.dirty ? 'var(--status-warning-text)' : 'var(--text-primary)' }}>
              {actionBar.dirty ? '有未保存变更' : '账号应用范围已保存'}
            </div>
            <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              已保存 {actionBar.savedCount} 个账号，草稿选择 {actionBar.draftCount} 个账号。
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="btn-ghost btn-sm flex-1 md:flex-none"
              disabled={disabled || actionBar.saving || !actionBar.dirty}
              onClick={actionBar.onCancel}
            >
              {actionBar.cancelLabel || '取消更改'}
            </button>
            <button
              type="button"
              className="btn-primary btn-sm flex-1 md:flex-none disabled:opacity-40"
              disabled={disabled || actionBar.saving || !actionBar.dirty}
              onClick={actionBar.onSave}
            >
              {actionBar.saving ? '保存中…' : (actionBar.saveLabel || '保存应用')}
            </button>
          </div>
        </div>
      )}
    </section>
  )
}

export default AccountCapabilityPicker
