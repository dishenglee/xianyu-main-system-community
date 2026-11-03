import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ChevronDown } from 'lucide-react'
import { DeliverySelectionCheckbox } from './DeliverySelectionCheckbox'
import { toggleDeliveryAccountSelection } from './deliveryMultiAccountCreate'

type DeliveryAccountOption = { uid: string; nick?: string }

type Props = {
  accounts: DeliveryAccountOption[]
  selectedUids: string[]
  onChange: (uids: string[]) => void
  disabled?: boolean
}

type MenuPosition = { top: number; left: number; width: number; maxHeight: number }

export default function DeliveryAccountMultiSelect({
  accounts,
  selectedUids,
  onChange,
  disabled = false,
}: Props) {
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState<MenuPosition>({ top: 0, left: 0, width: 280, maxHeight: 320 })
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)
  const selectedSet = useMemo(() => new Set(selectedUids), [selectedUids])
  const selectedAccounts = accounts.filter((account) => selectedSet.has(account.uid))
  const summary = selectedAccounts.length === 0
    ? '请选择账号'
    : selectedAccounts.length === 1
      ? selectedAccounts[0].nick || selectedAccounts[0].uid
      : `已选 ${selectedAccounts.length} 个账号`

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current
    if (!trigger) return
    const rect = trigger.getBoundingClientRect()
    const padding = 12
    const availableWidth = Math.max(0, window.innerWidth - padding * 2)
    const width = Math.min(340, availableWidth, Math.max(rect.width, Math.min(280, availableWidth)))
    const left = Math.min(Math.max(padding, rect.left), Math.max(padding, window.innerWidth - width - padding))
    const top = rect.bottom + 6
    setPosition({
      top,
      left,
      width,
      maxHeight: Math.max(140, Math.min(320, window.innerHeight - top - padding)),
    })
  }, [])

  useEffect(() => {
    if (!open) return undefined
    updatePosition()
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (!triggerRef.current?.contains(target) && !menuRef.current?.contains(target)) setOpen(false)
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setOpen(false)
      triggerRef.current?.focus()
    }
    const handleViewportChange = () => updatePosition()
    document.addEventListener('mousedown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    window.addEventListener('resize', handleViewportChange)
    window.addEventListener('scroll', handleViewportChange, true)
    return () => {
      document.removeEventListener('mousedown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
      window.removeEventListener('resize', handleViewportChange)
      window.removeEventListener('scroll', handleViewportChange, true)
    }
  }, [open, updatePosition])

  return (
    <div className="w-full">
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        aria-expanded={open}
        aria-controls={open ? 'delivery-create-account-options' : undefined}
        aria-label={`新增配置账号：${summary}`}
        title={summary}
        onClick={() => setOpen((current) => !current)}
        className="input inline-flex min-h-10 w-full min-w-0 items-center justify-between gap-2 text-left disabled:opacity-50"
      >
        <span className="min-w-0 flex-1 truncate">{summary}</span>
        <ChevronDown className={`h-4 w-4 shrink-0 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open ? createPortal(
        <div
          id="delivery-create-account-options"
          ref={menuRef}
          role="group"
          aria-label="新增配置账号选项"
          className="fixed z-[140] overflow-y-auto rounded-[14px] border py-1.5 shadow-lg"
          style={{
            top: position.top,
            left: position.left,
            width: position.width,
            maxHeight: position.maxHeight,
            background: 'var(--surface-card-strong)',
            borderColor: 'var(--border-light)',
          }}
        >
          {!accounts.length ? (
            <div className="px-4 py-5 text-center text-[12px]" style={{ color: 'var(--text-secondary)' }}>暂无可配置账号</div>
          ) : accounts.map((account) => {
            const checked = selectedSet.has(account.uid)
            const label = account.nick || account.uid
            return (
              <label
                key={account.uid}
                className="flex cursor-pointer items-center gap-3 px-3.5 py-2.5 hover:bg-[var(--surface-control)]"
              >
                <DeliverySelectionCheckbox
                  checked={checked}
                  disabled={disabled}
                  label={`选择自动发货账号 ${label}`}
                  onChange={() => onChange(toggleDeliveryAccountSelection(selectedUids, account.uid))}
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px] font-semibold" title={label}>{label}</span>
                  <span className="block truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>{account.uid}</span>
                </span>
              </label>
            )
          })}
        </div>,
        document.body,
      ) : null}
    </div>
  )
}
