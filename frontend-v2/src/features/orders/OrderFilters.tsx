// @ts-nocheck
import { useEffect, useRef, useState } from 'react'
import { ChevronDown } from 'lucide-react'
import { SensitiveValue, useSensitiveHidden } from '../../components/SensitiveValue'

export function OrdKpi({ label, value, danger = false }) {
  return (
    <div className="rounded-[14px] px-3.5 py-3" style={{ background: 'var(--surface-card)', border: '1px solid var(--control-border)' }}>
      <div className="text-[12.5px]" style={{ color: 'var(--text-muted)' }}>{label}</div>
      <div className="text-[20px] font-semibold font-mono leading-tight mt-1" style={{ color: danger ? 'var(--danger)' : 'var(--text-main)' }}>
        {value}
      </div>
    </div>
  )
}

export function OrdStatusTab({ label, value, count, active, onChange, tone }) {
  return (
    <button
      onClick={() => onChange(value)}
      className="px-3 py-[5px] rounded-[7px] text-[13px] font-medium cursor-pointer transition-all flex items-center gap-1.5"
      style={{
        background: active ? 'var(--surface-control)' : 'transparent',
        color: active ? 'var(--text-main)' : 'var(--text-secondary)',
        boxShadow: active ? 'inset 0 1px 0 rgba(255,255,255,0.62), 0 8px 18px rgba(25,35,55,0.08)' : 'none',
      }}
    >
      {tone ? <span className="w-1.5 h-1.5 rounded-full" style={{ background: tone }} /> : null}
      {label}
      <span className="font-mono opacity-60">{count}</span>
    </button>
  )
}

export function AccountMultiSelect({ accountStats, selectedUids, onChange }) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef(null)
  const triggerRef = useRef(null)
  const isSensitiveHidden = useSensitiveHidden()
  const selectedSet = new Set(selectedUids)
  const selectedAccounts = accountStats.filter((account) => selectedSet.has(account.uid))
  const selectedAccountLabel = selectedAccounts.length === 1
    ? (isSensitiveHidden ? '••••••' : selectedAccounts[0].nick)
    : ''
  const summary = selectedUids.length === 0
    ? '全部账号'
    : selectedAccounts.length === 1
      ? selectedAccountLabel
      : `已选 ${selectedAccounts.length} 个账号`

  useEffect(() => {
    if (!open) return undefined

    const closeOnOutsideClick = (event) => {
      if (!rootRef.current?.contains(event.target)) setOpen(false)
    }
    const closeOnEscape = (event) => {
      if (event.key === 'Escape') {
        setOpen(false)
        triggerRef.current?.focus()
      }
    }

    document.addEventListener('mousedown', closeOnOutsideClick)
    document.addEventListener('touchstart', closeOnOutsideClick)
    document.addEventListener('keydown', closeOnEscape)
    return () => {
      document.removeEventListener('mousedown', closeOnOutsideClick)
      document.removeEventListener('touchstart', closeOnOutsideClick)
      document.removeEventListener('keydown', closeOnEscape)
    }
  }, [open])

  const toggleAccount = (uid) => {
    const next = selectedSet.has(uid)
      ? selectedUids.filter((selectedUid) => selectedUid !== uid)
      : [...selectedUids, uid]
    onChange(next)
  }

  return (
    <div ref={rootRef} className="relative shrink-0">
      <button
        ref={triggerRef}
        type="button"
        aria-label={`账号选择：${summary}`}
        aria-haspopup="true"
        aria-expanded={open}
        title={summary}
        onClick={() => setOpen((current) => !current)}
        className="glass-control flex h-[34px] w-[168px] max-w-[168px] items-center justify-between gap-2 rounded-[12px] px-3 text-[13px] font-medium cursor-pointer"
        style={{ color: 'var(--text-main)' }}
      >
        <span className="min-w-0 flex-1 truncate text-left">{summary}</span>
        <ChevronDown
          aria-hidden="true"
          className={`h-3.5 w-3.5 shrink-0 transition-transform ${open ? 'rotate-180' : ''}`}
          style={{ color: 'var(--text-muted)' }}
        />
      </button>

      {open ? (
        <div
          role="group"
          aria-label="账号选项"
          className="absolute right-0 top-[calc(100%+6px)] z-40 w-[240px] overflow-hidden rounded-[12px] py-1.5"
          style={{ background: 'var(--surface-card-strong)', border: '1px solid var(--control-border)', boxShadow: 'var(--glass-shadow-float)' }}
        >
          <label className="flex min-h-[36px] items-center gap-2.5 px-3 py-2 cursor-pointer hover:bg-[var(--surface-control)]">
            <input
              type="checkbox"
              aria-label="全部账号"
              checked={selectedUids.length === 0}
              onChange={() => onChange([])}
              className="h-4 w-4 shrink-0 accent-[var(--primary)]"
            />
            <span className="min-w-0 flex-1 text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
              全部账号
            </span>
          </label>

          <div className="mx-3 h-px" style={{ background: 'var(--border-subtle)' }} />

          <div className="max-h-[280px] overflow-y-auto py-1">
            {accountStats.map((account) => {
              const accountLabel = isSensitiveHidden ? '••••••' : account.nick
              return (
                <label
                  key={account.uid}
                  className="flex min-h-[40px] items-center gap-2.5 px-3 py-2 cursor-pointer hover:bg-[var(--surface-control)]"
                >
                  <input
                    type="checkbox"
                    aria-label={`选择账号 ${accountLabel}`}
                    checked={selectedSet.has(account.uid)}
                    onChange={() => toggleAccount(account.uid)}
                    className="h-4 w-4 shrink-0 accent-[var(--primary)]"
                  />
                  <span
                    className="h-2.5 w-2.5 shrink-0 rounded-full"
                    aria-hidden="true"
                    style={{ background: account.color }}
                  />
                  <SensitiveValue
                    kind="account"
                    className="min-w-0 flex-1 truncate text-[13px]"
                    style={{ color: 'var(--text-main)' }}
                    value={account.nick}
                  />
                  <span className="shrink-0 text-[11px] font-mono" style={{ color: 'var(--text-muted)' }}>
                    {account.total}
                  </span>
                </label>
              )
            })}
          </div>
        </div>
      ) : null}
    </div>
  )
}
