// @ts-nocheck
import { useEffect, useState } from 'react'
import { RefreshCw, User } from 'lucide-react'
import { useParams } from 'react-router-dom'
import { useAccounts } from '../../hooks/useAccounts'
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { AccountDetailContent } from './AccountDetailContent'
import { useSensitiveHidden } from '../../components/SensitiveValue'

function useIsMobileViewport() {
  const [isMobileViewport, setIsMobileViewport] = useState(() => (
    typeof window !== 'undefined' ? window.innerWidth < 768 : false
  ))

  useEffect(() => {
    if (typeof window === 'undefined') return undefined

    const media = typeof window.matchMedia === 'function'
      ? window.matchMedia('(max-width: 767px)')
      : null
    const sync = () => setIsMobileViewport(media ? media.matches : window.innerWidth < 768)

    sync()
    if (media?.addEventListener) {
      media.addEventListener('change', sync)
      return () => media.removeEventListener('change', sync)
    }

    window.addEventListener('resize', sync)
    return () => window.removeEventListener('resize', sync)
  }, [])

  return isMobileViewport
}

export default function AccountDetailPage() {
  const isSensitiveHidden = useSensitiveHidden()
  const { uid: routeUid } = useParams()
  const { accounts, loadAccounts } = useAccounts()
  const [selectedUid, setSelectedUid] = useState(routeUid || '')
  const [refreshToken, setRefreshToken] = useState(0)
  const isMobileViewport = useIsMobileViewport()

  useEffect(() => { loadAccounts() }, [])

  useEffect(() => {
    if (!accounts.length) return
    if (!selectedUid || !accounts.some((a) => a.uid === selectedUid)) {
      setSelectedUid(routeUid && accounts.some((a) => a.uid === routeUid) ? routeUid : accounts[0].uid)
    }
  }, [accounts, routeUid, selectedUid])

  const currentAccount = accounts.find((account) => account.uid === selectedUid) || null
  const currentLabel = currentAccount?.nick || currentAccount?.uid || '未选择'

  return (
    <div className={isMobileViewport ? 'h-full flex flex-col mobile-page-shell' : 'contents'} style={isMobileViewport ? { backgroundColor: 'var(--bg-page)' } : undefined}>
      <TopBar
        title="账号详情"
        back
        subtitle={isMobileViewport ? 'Account profile' : undefined}
        right={(
          <button onClick={() => setRefreshToken((v) => v + 1)} disabled={!selectedUid} className="btn-ghost btn-sm disabled:opacity-40">
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        )}
      />

      <div className={isMobileViewport ? 'flex-1 overflow-y-auto mobile-page-body' : 'flex-1 overflow-y-auto px-4 py-4 md:px-6'}>
        <div className={isMobileViewport ? 'mobile-page-stack' : 'space-y-4'}>
          <div
            className={isMobileViewport ? 'mobile-surface-card p-4' : 'surface-card rounded-[18px] p-4'}
            style={isMobileViewport ? undefined : undefined}
          >
            {isMobileViewport ? (
              <div className="mobile-section-head mb-4 items-start">
                <div className="mobile-section-copy">
                  <span className="mobile-section-kicker">Switch account</span>
                  <div className="mobile-section-title">切换账号</div>
                  <div className="mobile-section-note">从当前登录账号里切换查看详情，不改任何账号状态。</div>
                </div>
                <span className="mobile-pill shrink-0">{accounts.length} 个账号</span>
              </div>
            ) : (
              <div className="text-[12px] mb-2" style={{ color: 'var(--text-muted)' }}>选择账号</div>
            )}

            <div className={isMobileViewport ? 'mobile-soft-card p-3.5' : ''}>
              {isMobileViewport ? (
                <div className="mb-2 flex items-center justify-between gap-3">
                  <div className="text-[11px] font-semibold uppercase tracking-[0.14em]" style={{ color: 'var(--primary)' }}>
                    Current
                  </div>
                  <div className="text-[12px] truncate" style={{ color: 'var(--text-secondary)' }}>
                    {currentLabel}
                  </div>
                </div>
              ) : null}
              <select
                className={`input w-full${isMobileViewport ? ' rounded-[14px] border-[var(--control-border)] px-4 py-3 text-[15px]' : ''}`}
                value={selectedUid}
                onChange={(e) => setSelectedUid(e.target.value)}
              >
                <option value="" disabled>选择账号</option>
                {accounts.map((account) => (
                  <option key={account.uid} value={account.uid}>{isSensitiveHidden ? '••••••' : account.nick || account.uid}</option>
                ))}
              </select>
            </div>
          </div>

          {!selectedUid ? (
            <EmptyState icon={User} title="请先选择账号" />
          ) : (
            <AccountDetailContent
              uid={selectedUid}
              account={currentAccount}
              refreshToken={refreshToken}
              onAccountUpdated={loadAccounts}
            />
          )}
        </div>
      </div>
    </div>
  )
}
