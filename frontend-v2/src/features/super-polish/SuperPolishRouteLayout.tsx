import { useEffect, useMemo, useState } from 'react'
import { Navigate, NavLink, Outlet, useLocation, useSearchParams } from 'react-router-dom'
import { RefreshCw, Sparkles } from 'lucide-react'
import { useAccounts } from '../../hooks/useAccounts'
import { getSuperPolishAccountInfo, unwrapMtopData } from '../../api/superPolish'
import { SuperPolishSection } from './components/SuperPolishUi'
import type { SuperPolishScope } from './hooks/useSuperPolishScope'
import { asRecord, formatMoney } from './model'
import { assertSuperPolishRequestSuccess } from './mtop'

const TABS = [
  {
    to: '/super-polish/campaigns',
    label: '已建计划',
    match: (pathname: string) => pathname === '/super-polish/campaigns' || /^\/super-polish\/campaigns\/[^/]+$/.test(pathname),
  },
  { to: '/super-polish/campaigns/new', label: '新建计划' },
  { to: '/super-polish/reports', label: '计划报表' },
]

export function SuperPolishRouteLayout({ mobile = false }: { mobile?: boolean }) {
  const { activeAccounts, loading: accountsLoading, loadAccounts } = useAccounts()
  const location = useLocation()
  const [searchParams, setSearchParams] = useSearchParams()
  const [initialScopePending, setInitialScopePending] = useState(() => activeAccounts.length === 0)
  const [balanceReloadTick, setBalanceReloadTick] = useState(0)
  const [accountBalance, setAccountBalance] = useState<{
    key: string
    usableBalance: number | null
    totalBalance: number | null
    error: string
  }>({
    key: '',
    usableBalance: null,
    totalBalance: null,
    error: '',
  })

  useEffect(() => {
    let cancelled = false
    if (!initialScopePending) return () => {
      cancelled = true
    }

    Promise.resolve(loadAccounts())
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setInitialScopePending(false)
      })

    return () => {
      cancelled = true
    }
  }, [initialScopePending, loadAccounts])

  const scopePending = initialScopePending && activeAccounts.length === 0

  const uidParam = searchParams.get('uid') ?? ''
  const selectedUid = useMemo(() => {
    if (!activeAccounts.length) return ''
    if (uidParam && activeAccounts.some((account) => account.uid === uidParam)) {
      return uidParam
    }
    return activeAccounts[0]?.uid ?? ''
  }, [activeAccounts, uidParam])

  useEffect(() => {
    if (!selectedUid) return
    if (uidParam === selectedUid) return
    const next = new URLSearchParams()
    next.set('uid', selectedUid)
    setSearchParams(next, { replace: true })
  }, [selectedUid, uidParam, setSearchParams])

  const selectedAccount = activeAccounts.find((account) => account.uid === selectedUid) ?? null
  const balanceRequestKey = `${selectedUid}||${balanceReloadTick}`

  useEffect(() => {
    if (!selectedUid) return

    let active = true

    getSuperPolishAccountInfo(selectedUid)
      .then((payload) => {
        if (!active) return
        const data = asRecord(unwrapMtopData(assertSuperPolishRequestSuccess(payload)))
        const usableBalance = Number(data.usableBalance ?? data.cashBalance)
        const totalBalance = Number(data.totalBalance ?? data.usableBalance ?? data.cashBalance)
        setAccountBalance({
          key: balanceRequestKey,
          usableBalance: Number.isFinite(usableBalance) ? usableBalance : 0,
          totalBalance: Number.isFinite(totalBalance) ? totalBalance : 0,
          error: '',
        })
      })
      .catch((reason: unknown) => {
        if (!active) return
        setAccountBalance({
          key: balanceRequestKey,
          usableBalance: null,
          totalBalance: null,
          error: reason instanceof Error ? reason.message : '余额读取失败',
        })
      })

    return () => {
      active = false
    }
  }, [balanceRequestKey, selectedUid])

  function setUid(uid: string) {
    const next = new URLSearchParams()
    next.set('uid', uid)
    setSearchParams(next, { replace: true })
  }

  function buildPath(path: string) {
    if (!selectedUid) return path
    return `${path}?uid=${encodeURIComponent(selectedUid)}`
  }

  const outletContext: SuperPolishScope = {
    uid: selectedUid,
    account: selectedAccount,
    accounts: activeAccounts,
    accountsLoading,
    setUid,
    buildPath,
    mobile,
  }

  if (location.pathname === '/super-polish' || location.pathname === '/super-polish/account') {
    return <Navigate to={buildPath('/super-polish/campaigns')} replace />
  }

  const hasResolvedAccountBalance = Boolean(selectedUid) && accountBalance.key === balanceRequestKey
  const usableBalanceLabel = hasResolvedAccountBalance && accountBalance.usableBalance !== null
    ? formatMoney(accountBalance.usableBalance)
    : '¥0'
  const totalBalanceLabel = hasResolvedAccountBalance && accountBalance.totalBalance !== null
    ? formatMoney(accountBalance.totalBalance)
    : '¥0'

  return (
    <div className={mobile ? 'mobile-page-body space-y-4 pb-8' : 'mx-auto w-full max-w-7xl space-y-5'}>
      <SuperPolishSection
        title="超级擦亮"
        subtitle="当前只保留已建计划、新建计划和计划报表三个工作区。"
        actions={(
          <button
            onClick={() => {
              setBalanceReloadTick((value) => value + 1)
              void loadAccounts({ force: true })
            }}
            className="btn-ghost btn-sm"
            disabled={accountsLoading}
          >
            <RefreshCw className={`h-3.5 w-3.5 ${accountsLoading ? 'animate-spin' : ''}`} />
            刷新账号
          </button>
        )}
      >
        <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_260px]">
          <div
            className="rounded-[18px] px-4 py-3"
            style={{ background: 'var(--surface-soft)', border: '1px solid var(--control-border)' }}
          >
            <div className="flex items-center gap-2 text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
              <Sparkles className="h-4 w-4" style={{ color: 'var(--primary)' }} />
              {selectedAccount ? (selectedAccount.nick || selectedAccount.uid) : (scopePending || accountsLoading ? '加载账号中…' : '暂无可用账号')}
            </div>
            {selectedAccount ? (
              <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                <span>{selectedAccount.uid}</span>
                {!hasResolvedAccountBalance ? (
                  <span>余额加载中…</span>
                ) : accountBalance.error ? (
                  <span>{accountBalance.error}</span>
                ) : (
                  <>
                    <span>
                      可用余额 <span style={{ color: 'var(--text-main)' }}>{usableBalanceLabel}</span>
                    </span>
                    <span>
                      总余额 <span style={{ color: 'var(--text-main)' }}>{totalBalanceLabel}</span>
                    </span>
                  </>
                )}
              </div>
            ) : (
              <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                {scopePending || accountsLoading ? '正在读取可用账号，完成后会自动进入工作区。' : '先选择一个已激活账号，再进入计划与报表工作区。'}
              </div>
            )}
          </div>
          <label className="space-y-1.5">
            <div className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>当前账号</div>
            <select
              value={selectedUid}
              onChange={(event) => setUid(event.target.value)}
              disabled={scopePending || accountsLoading || activeAccounts.length === 0}
              className="w-full rounded-[14px] border px-3 py-2 text-[13px] outline-none"
              style={{
                borderColor: 'var(--control-border)',
                background: 'var(--surface-card)',
                color: 'var(--text-main)',
              }}
            >
              {activeAccounts.length === 0 ? <option value="">暂无账号</option> : null}
              {activeAccounts.map((account) => (
                <option key={account.uid} value={account.uid}>
                  {account.nick || account.uid}
                </option>
              ))}
            </select>
          </label>
        </div>
      </SuperPolishSection>

      <nav
        className="flex gap-2 overflow-x-auto rounded-[18px] border px-2 py-2"
        style={{
          borderColor: 'var(--glass-border)',
          background: 'var(--surface-card)',
          scrollbarWidth: 'none',
        }}
      >
        {TABS.map((tab) => (
          <NavLink
            key={tab.to}
            to={buildPath(tab.to)}
            end={!('match' in tab)}
            className={() => [
              'shrink-0 rounded-[12px] px-3.5 py-2 text-[13px] font-medium transition',
              (tab.match ? tab.match(location.pathname) : location.pathname === tab.to) ? 'shadow-sm' : '',
            ].join(' ')}
            style={() => ({
              background: (tab.match ? tab.match(location.pathname) : location.pathname === tab.to) ? 'var(--primary-light)' : 'transparent',
              color: (tab.match ? tab.match(location.pathname) : location.pathname === tab.to) ? 'var(--text-main)' : 'var(--text-secondary)',
            })}
          >
            {tab.label}
          </NavLink>
        ))}
      </nav>

      {scopePending ? (
        <div
          className="rounded-[18px] border px-4 py-10 text-center text-[13px]"
          style={{
            borderColor: 'var(--control-border)',
            background: 'var(--surface-soft)',
            color: 'var(--text-secondary)',
          }}
        >
          正在加载可用账号，完成后会自动进入超级擦亮工作区。
        </div>
      ) : (
        <Outlet context={outletContext} />
      )}
    </div>
  )
}
