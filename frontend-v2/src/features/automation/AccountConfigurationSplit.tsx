import { Trash2 } from 'lucide-react'

type AccountLike = {
  uid: string
  nick?: string | null
  status?: string | null
}

type Props = {
  accounts: AccountLike[]
  configuredUids: string[]
  selectedPendingUids: string[]
  onSelectionChange: (uids: string[]) => void
  onSave: () => void
  onRemove: (uid: string) => void
  saving?: boolean
  disabled?: boolean
  removingUid?: string
  pendingDescription?: string
  configuredDescription?: string
  saveLabel?: string
}

export function AccountConfigurationSplit({
  accounts,
  configuredUids,
  selectedPendingUids,
  onSelectionChange,
  onSave,
  onRemove,
  saving = false,
  disabled = false,
  removingUid = '',
  pendingDescription = '选择尚未配置的活跃账号。',
  configuredDescription = '已保存到当前功能配置范围的账号。',
  saveLabel = '保存配置',
}: Props) {
  const configuredSet = new Set(configuredUids)
  const pendingAccounts = accounts.filter(
    (account) => account.status === 'active' && !configuredSet.has(account.uid),
  )
  const configuredAccounts = accounts.filter((account) => configuredSet.has(account.uid))
  const selectedSet = new Set(selectedPendingUids)
  const busy = disabled || saving || Boolean(removingUid)
  const setSelected = (uid: string, checked: boolean) => {
    if (busy) return
    onSelectionChange(
      checked
        ? [...new Set([...selectedPendingUids, uid])]
        : selectedPendingUids.filter((item) => item !== uid),
    )
  }

  const renderAccount = (account: AccountLike, configured: boolean) => (
    <div
      key={account.uid}
      className="flex min-w-0 items-center gap-2 rounded-lg border px-3 py-2.5"
      style={{
        borderColor: !configured && selectedSet.has(account.uid)
          ? 'var(--primary)'
          : 'var(--border-light)',
        background: !configured && selectedSet.has(account.uid)
          ? 'var(--primary-light)'
          : 'var(--surface-card)',
      }}
    >
      {!configured ? (
        <label className="flex min-h-11 min-w-11 shrink-0 cursor-pointer items-center justify-center">
          <input
            type="checkbox"
            aria-label={`选择待配置账号 ${account.nick || account.uid}`}
            checked={selectedSet.has(account.uid)}
            disabled={busy}
            onChange={(event) => setSelected(account.uid, event.target.checked)}
            className="h-4 w-4 shrink-0"
            style={{ accentColor: 'var(--primary)' }}
          />
        </label>
      ) : null}
      <span className="min-w-0 flex-1">
        <b className="block truncate text-[12px]" style={{ color: 'var(--text-main)' }}>
          {account.nick || account.uid}
        </b>
        <span className="block truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>
          {account.uid}
        </span>
      </span>
      {configured ? (
        <button
          type="button"
          className="btn-ghost btn-sm inline-flex min-h-11 min-w-11 shrink-0 items-center justify-center gap-1 px-1.5"
          aria-label={`移除已配置账号 ${account.nick || account.uid}`}
          disabled={busy}
          onClick={() => onRemove(account.uid)}
          style={{ color: 'var(--danger)' }}
        >
          <Trash2 className="h-3.5 w-3.5" />
          <span className="hidden sm:inline">{removingUid === account.uid ? '移除中…' : '移除'}</span>
        </button>
      ) : null}
    </div>
  )

  return (
    <div data-testid="account-configuration-split" className="grid gap-4 lg:grid-cols-2">
      <section className="rounded-xl border p-3.5 space-y-3" style={{ borderColor: 'var(--border-light)' }}>
        <div className="flex items-start justify-between gap-2">
          <div>
            <h3 className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>待配置账号</h3>
            <p className="mt-1 text-[11px] leading-5" style={{ color: 'var(--text-secondary)' }}>{pendingDescription}</p>
          </div>
          {pendingAccounts.length ? (
            <div className="flex shrink-0 gap-1">
              <button type="button" className="btn-ghost btn-sm min-h-11 px-2" disabled={busy} onClick={() => onSelectionChange(pendingAccounts.map((account) => account.uid))}>全选</button>
              <button type="button" className="btn-ghost btn-sm min-h-11 px-2" disabled={busy} onClick={() => onSelectionChange([])}>清空</button>
            </div>
          ) : null}
        </div>
        {pendingAccounts.length ? (
          <div data-testid="pending-account-grid" className="grid grid-cols-2 gap-2">
            {pendingAccounts.map((account) => renderAccount(account, false))}
          </div>
        ) : (
          <div className="rounded-lg border border-dashed px-3 py-5 text-center text-[11px]" style={{ borderColor: 'var(--border-light)', color: 'var(--text-secondary)' }}>所有活跃账号都已配置</div>
        )}
        <div className="flex items-center justify-between gap-2">
          <span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>已选择 {selectedPendingUids.length} 个</span>
          <button type="button" className="btn-primary btn-sm min-h-11" disabled={busy || !selectedPendingUids.length} onClick={onSave}>{saving ? '保存中…' : saveLabel}</button>
        </div>
      </section>

      <section className="rounded-xl border p-3.5 space-y-3" style={{ borderColor: 'var(--border-light)' }}>
        <div>
          <h3 className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>已配置账号</h3>
          <p className="mt-1 text-[11px] leading-5" style={{ color: 'var(--text-secondary)' }}>{configuredDescription}</p>
        </div>
        {configuredAccounts.length ? (
          <div data-testid="configured-account-grid" className="grid grid-cols-2 gap-2">
            {configuredAccounts.map((account) => renderAccount(account, true))}
          </div>
        ) : (
          <div className="rounded-lg border border-dashed px-3 py-5 text-center text-[11px]" style={{ borderColor: 'var(--border-light)', color: 'var(--text-secondary)' }}>尚未配置账号</div>
        )}
      </section>
    </div>
  )
}
