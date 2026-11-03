import { useEffect } from 'react'
import { Database, MessageSquare, PackageCheck, Wifi } from 'lucide-react'
import { Link } from 'react-router-dom'
import { useAccounts } from '../../hooks/useAccounts'
import { accountRuntimeStatus } from '../accounts/hooks/useAccountsPageState'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import { SensitiveValue } from '../../components/SensitiveValue'

export default function LocalProfilePage() {
  const { accounts, loadAccounts } = useAccounts()
  const businessAccounts = filterBusinessAccounts(accounts)

  useEffect(() => {
    void loadAccounts()
  }, [loadAccounts])

  return (
    <div className="h-full overflow-y-auto mobile-page-shell px-4 py-5">
      <div className="mx-auto w-full max-w-3xl space-y-4">
        <section className="mobile-surface-card p-5">
          <h1 className="text-[18px] font-semibold" style={{ color: 'var(--text-main)' }}>本地运行</h1>
          <p className="mt-1 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            单用户模式，账号与业务数据保存在这台设备上。
          </p>
          <div className="mt-4 flex flex-wrap gap-2">
            <span className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12px]" style={{ background: 'var(--status-success-bg)', color: 'var(--status-success-text)' }}>
              <Wifi className="h-3.5 w-3.5" /> 本地服务
            </span>
            <span className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12px]" style={{ background: 'var(--surface-control)', color: 'var(--text-secondary)' }}>
              <Database className="h-3.5 w-3.5" /> {accounts.length} 个账号
            </span>
          </div>
        </section>

        <section className="mobile-surface-card p-5">
          <h2 className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>闲鱼账号</h2>
          {businessAccounts.length === 0 ? (
            <p className="mt-3 text-[13px]" style={{ color: 'var(--text-secondary)' }}>还没有添加账号。</p>
          ) : (
            <div className="mt-3 divide-y" style={{ borderColor: 'var(--control-border)' }}>
              {businessAccounts.map((account) => {
                const runtime = accountRuntimeStatus(account)
                return (
                  <Link key={account.uid} to={`/accounts/${account.uid}`} className="flex items-center gap-3 py-3">
                    <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full" style={{ background: 'var(--surface-control)', color: 'var(--text-secondary)' }}>
                      <Database className="h-4 w-4" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
                        <SensitiveValue kind="account" value={account.nick || '闲鱼账号'} />
                      </p>
                      <p className="mt-0.5 text-[11px]" style={{ color: 'var(--text-tertiary)' }}>{runtime.label}</p>
                    </div>
                    <div className="flex items-center gap-3 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                      <span className="inline-flex items-center gap-1"><MessageSquare className="h-3 w-3" /> {account.aiEnabled ? 'AI回复' : 'AI关闭'}</span>
                      <span className="inline-flex items-center gap-1"><PackageCheck className="h-3 w-3" /> {account.autoShip ? '自动发货' : '手动发货'}</span>
                    </div>
                  </Link>
                )
              })}
            </div>
          )}
        </section>
      </div>
    </div>
  )
}
