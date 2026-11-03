import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ChevronDown } from 'lucide-react'

type AccountOption = { uid: string; nick?: string }

type Props = {
  accounts: AccountOption[]
  selectedUid: string
  onChange: (uid: string) => void
  disabled?: boolean
}

type MenuPosition = { top: number; left: number; width: number; maxHeight: number }

export default function AccountScopeDropdown({ accounts, selectedUid, onChange, disabled = false }: Props) {
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState<MenuPosition>({ top: 0, left: 0, width: 240, maxHeight: 288 })
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)
  const selected = accounts.find((account) => account.uid === selectedUid)

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current
    if (!trigger) return
    const rect = trigger.getBoundingClientRect()
    const viewportPadding = 12
    const availableWidth = Math.max(0, window.innerWidth - viewportPadding * 2)
    const width = Math.min(320, availableWidth, Math.max(rect.width, Math.min(240, availableWidth)))
    const left = Math.min(Math.max(viewportPadding, rect.left), Math.max(viewportPadding, window.innerWidth - width - viewportPadding))
    const top = rect.bottom + 6
    setPosition({ top, left, width, maxHeight: Math.max(120, Math.min(288, window.innerHeight - top - viewportPadding)) })
  }, [])

  useEffect(() => {
    if (!open) return
    updatePosition()
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (!triggerRef.current?.contains(target) && !menuRef.current?.contains(target)) setOpen(false)
    }
    const handleKeyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false) }
    const handleResize = () => updatePosition()
    const handleScroll = (event: Event) => {
      const target = event.target
      if (target instanceof Node && menuRef.current?.contains(target)) return
      const trigger = triggerRef.current
      if (!trigger) { setOpen(false); return }
      const rect = trigger.getBoundingClientRect()
      if (rect.bottom <= 0 || rect.top >= window.innerHeight || rect.right <= 0 || rect.left >= window.innerWidth) {
        setOpen(false)
        return
      }
      updatePosition()
    }
    document.addEventListener('mousedown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    window.addEventListener('resize', handleResize)
    window.addEventListener('scroll', handleScroll, true)
    return () => {
      document.removeEventListener('mousedown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
      window.removeEventListener('resize', handleResize)
      window.removeEventListener('scroll', handleScroll, true)
    }
  }, [open, updatePosition])

  const select = (uid: string) => {
    onChange(uid)
    setOpen(false)
  }

  return (
    <div className="w-full sm:w-[240px] sm:shrink-0">
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="inline-flex h-9 w-full min-w-0 items-center justify-between gap-2 overflow-hidden rounded-[12px] border bg-[var(--surface-control)] px-3 py-0 text-left text-[13px] leading-none disabled:opacity-50"
        style={{ borderColor: 'var(--border-light)', color: 'var(--text-main)' }}
      >
        <span className="min-w-0 flex-1 truncate whitespace-nowrap">{selected ? (selected.nick || selected.uid) : '全部账号'}</span>
        <ChevronDown className={`h-4 w-4 shrink-0 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open ? createPortal(
        <div
          ref={menuRef}
          role="listbox"
          aria-label="账号筛选"
          className="fixed z-[120] overflow-y-auto rounded-[14px] border py-1 shadow-lg"
          style={{ top: position.top, left: position.left, width: position.width, maxHeight: position.maxHeight, background: 'var(--surface-card-strong)', borderColor: 'var(--border-light)' }}
        >
          <button type="button" role="option" aria-selected={!selectedUid} onClick={() => select('')} className="w-full px-4 py-2.5 text-left" style={{ background: !selectedUid ? 'var(--primary-light)' : 'transparent' }}>
            <div className="text-[13px] font-semibold">全部账号</div><div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>聚合查看所有自动发货配置</div>
          </button>
          {accounts.map((account) => <button key={account.uid} type="button" role="option" aria-selected={account.uid === selectedUid} onClick={() => select(account.uid)} className="w-full px-4 py-2.5 text-left" style={{ background: account.uid === selectedUid ? 'var(--primary-light)' : 'transparent' }}><div className="truncate text-[13px] font-semibold">{account.nick || account.uid}</div><div className="truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>{account.uid}</div></button>)}
        </div>,
        document.body,
      ) : null}
    </div>
  )
}
