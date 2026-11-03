import { useState, useRef, useEffect } from 'react'
import { ChevronDown, RefreshCw } from 'lucide-react'
import { PromotionContent } from './PromotionContent'
import { usePromotionPageState } from './hooks/usePromotionPageState'
import { getCoinBalance } from './promotionUtils'
import { SensitiveVisibilityToggle } from '../../components/SensitiveValue'

export default function DesktopPromotion() {
  const state = usePromotionPageState()
  const coinBalance = getCoinBalance(state.growth, state.selectedAccount)
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setDropdownOpen(false)
      }
    }
    if (dropdownOpen) {
      document.addEventListener('mousedown', handleClickOutside)
    }
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [dropdownOpen])

  return (
    <div className="flex-1 overflow-auto scrollbar-hidden px-4 py-4 md:px-6">
      <section className="surface-card mb-4 rounded-[24px] px-5 py-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0">
            <h1 className="text-[20px] font-semibold tracking-[-0.02em]" style={{ color: 'var(--text-main)' }}>
              推广
            </h1>
            <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              {state.selectedAccount
                ? `${state.selectedAccount.nick || state.selectedAccount.uid} · 闲鱼币 ${coinBalance}`
                : state.accountsLoading
                  ? '加载账号中…'
                  : '暂无可用账号'}
            </div>
          </div>
          <div className="flex items-center gap-2">
            <SensitiveVisibilityToggle compact />
            <button
              onClick={state.refresh}
              disabled={!state.selectedUid || state.loading}
              className="btn-ghost btn-sm disabled:opacity-40"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${state.loading ? 'animate-spin' : ''}`} />
              刷新
            </button>

            <div ref={dropdownRef} className="relative">
              <button
                type="button"
                onClick={() => setDropdownOpen(!dropdownOpen)}
                disabled={state.accountsLoading || !state.activeAccounts.length}
                className="btn-secondary btn-sm disabled:opacity-40 inline-flex items-center gap-1.5"
              >
                <span className="max-w-[120px] truncate">
                  {state.selectedAccount ? (state.selectedAccount.nick || state.selectedAccount.uid) : '选择账号'}
                </span>
                <ChevronDown className={`w-3 h-3 transition-transform ${dropdownOpen ? 'rotate-180' : ''}`} />
              </button>
              {dropdownOpen && (
                <div
                  className="absolute right-0 top-full mt-1 z-50 w-64 rounded-[14px] border py-1 shadow-lg max-h-72 overflow-auto scrollbar-hidden"
                  style={{
                    background: 'var(--surface-card)',
                    borderColor: 'var(--border-light)',
                  }}
                >
                  {state.activeAccounts.map((account) => (
                    <button
                      key={account.uid}
                      type="button"
                      onClick={() => {
                        state.selectUid(account.uid)
                        setDropdownOpen(false)
                      }}
                      className="w-full text-left px-4 py-2.5 transition hover:brightness-95"
                      style={{
                        background: account.uid === state.selectedUid ? 'var(--primary-light)' : 'transparent',
                        color: 'var(--text-main)',
                      }}
                    >
                      <div className="text-[13px] font-semibold truncate">{account.nick || account.uid}</div>
                      <div className="text-[11px] mt-0.5" style={{ color: 'var(--text-secondary)' }}>{account.uid}</div>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      </section>

      <div className="mx-auto w-full max-w-7xl">
        <PromotionContent state={state} />
      </div>
    </div>
  )
}
