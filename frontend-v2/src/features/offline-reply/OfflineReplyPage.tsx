// @ts-nocheck
import { useCallback, useEffect, useMemo, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { syncOfflineReply } from '../../api'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { useSafeNotify } from '../../components/Notify'
import { TopBar } from '../../components/TopBar'
import { useAuthScope } from '../../hooks/useAuthScope'
import { useAccounts } from '../../hooks/useAccounts'
import { useDevice } from '../../hooks/useDevice'
import AccountCapabilityPicker, { sanitizeSelectableUids } from '../automation/AccountCapabilityPicker'
import { filterBusinessAccounts } from '../../utils/accountAccess'

const emptyForm = {
  offlineReplyText: '你好，请描述你的需求或者直接拍下',
}

export default function OfflineReplyPage() {
  const { accounts, loading: accountsLoading, loadAccounts } = useAccounts()
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const [selectedUids, setSelectedUids] = useState([])
  const [globalForm, setGlobalForm] = useState({
    offlineReplyText: emptyForm.offlineReplyText,
  })
  const [loading, setLoading] = useState(false)
  const [globalSaving, setGlobalSaving] = useState('')
  const [batchProgress, setBatchProgress] = useState(null)
  const [error, setError] = useState('')
  const compact = isMobile || isTablet
  const activeAccounts = useMemo(() => filterBusinessAccounts(accounts), [accounts])
  const activeAccountKey = useMemo(() => activeAccounts.map((account) => account.uid).join('|'), [activeAccounts])
  const selectedAccountKey = selectedUids.join('|')
  const selectedAccounts = useMemo(
    () => activeAccounts.filter((account) => selectedUids.includes(account.uid)),
    [activeAccounts, selectedAccountKey],
  )
  const pickerShellClass = compact
    ? 'mobile-surface-card overflow-hidden [&>section]:!rounded-[26px] [&>section]:!border-0 [&>section]:!bg-transparent [&>section]:!shadow-none'
    : ''

  const clearPageState = useCallback(() => {
    setSelectedUids([])
    setGlobalForm({
      offlineReplyText: emptyForm.offlineReplyText,
    })
    setLoading(false)
    setGlobalSaving('')
    setBatchProgress(null)
    setError('')
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  useEffect(() => {
    void loadAccounts({ force: true })
  }, [authToken])

  useEffect(() => {
    setSelectedUids((uids) => {
      const sanitized = sanitizeSelectableUids(uids, activeAccounts, 'autoReply', {
        allowEnableDisabledAccounts: true,
      })
      return sanitized.length ? sanitized : activeAccounts.map((account) => account.uid)
    })
  }, [activeAccountKey])

  function handleAccountSelection(nextUids) {
    const sanitized = sanitizeSelectableUids(nextUids, activeAccounts, 'autoReply', {
      allowEnableDisabledAccounts: true,
    })
    setSelectedUids(sanitized)
  }

  useEffect(() => {
    setError('')
  }, [selectedAccountKey, authToken])

  async function refreshAccounts() {
    setLoading(true)
    setError('')
    try {
      await loadAccounts({ force: true })
    } catch (e) {
      setError(e?.message || '刷新账号失败')
    } finally {
      setLoading(false)
    }
  }

  async function saveSelectedOfflineReply() {
    if (!selectedAccounts.length) {
      notify('请先选择账号', 'error')
      return
    }
    setGlobalSaving('offline')
    startBatchProgress('所选账号不在线回复', selectedAccounts.length)
    const scope = captureScope(selectedAccountKey)
    const errors = []
    try {
      const offlineReplyText = String(globalForm.offlineReplyText || '').trim()
      for (const [index, account] of selectedAccounts.entries()) {
        const uid = account.uid
        const accountName = account.nick || uid
        updateBatchProgress({
          currentAccount: accountName,
          message: '正在处理',
          processedAccounts: index,
        })
        try {
          await syncOfflineReply(uid, { enableOfflineReply: true, offlineReplyText })
          updateBatchProgress((prev) => ({
            successItems: prev.successItems + 1,
          }))
        } catch (e) {
          errors.push(`${accountName} 账号级话术保存失败：${e?.message || e}`)
          updateBatchProgress((prev) => ({
            failedAccounts: prev.failedAccounts + 1,
            errors: errors.slice(-8),
          }))
        }
        updateBatchProgress({ processedAccounts: index + 1 })
      }
      if (!isScopeCurrent(scope, selectedAccountKey)) return
      updateBatchProgress({
        status: errors.length ? 'partial' : 'done',
        message: errors.length ? `已完成，${errors.length} 项失败` : '已完成',
        processedAccounts: selectedAccounts.length,
        currentAccount: '',
        errors: errors.slice(-8),
      })
      notify(
        errors.length
          ? `不在线回复已处理 ${selectedAccounts.length} 个账号，存在 ${errors.length} 个账号失败`
          : `已保存 ${selectedAccounts.length} 个账号的不在线回复`,
        errors.length ? 'error' : 'success',
      )
    } catch (e) {
      if (isScopeCurrent(scope, selectedAccountKey)) {
        updateBatchProgress({
          status: 'failed',
          message: e?.message || '保存所选账号不在线回复失败',
          errors: [e?.message || '保存所选账号不在线回复失败'],
        })
        notify(e?.message || '保存所选账号不在线回复失败', 'error')
      }
    } finally {
      if (isScopeCurrent(scope, selectedAccountKey)) setGlobalSaving('')
    }
  }

  function startBatchProgress(title, totalAccounts = activeAccounts.length) {
    setBatchProgress({
      title,
      status: 'running',
      message: '准备开始',
      totalAccounts,
      processedAccounts: 0,
      currentAccount: '',
      successItems: 0,
      failedItems: 0,
      failedAccounts: 0,
      errors: [],
    })
  }

  function updateBatchProgress(patch) {
    setBatchProgress((prev) => {
      if (!prev) return prev
      return {
        ...prev,
        ...(typeof patch === 'function' ? patch(prev) : patch),
      }
    })
  }

  async function applyOfflineReplyToAllAccounts() {
    if (!activeAccounts.length) {
      notify('暂无可配置账号', 'error')
      return
    }
    setGlobalSaving('offline-all-accounts')
    startBatchProgress('全局不在线回复', activeAccounts.length)
    const scope = captureScope(activeAccountKey)
    const errors = []
    try {
      const offlineReplyText = String(globalForm.offlineReplyText || '').trim()
      for (const [index, account] of activeAccounts.entries()) {
        const uid = account.uid
        const accountName = account.nick || uid
        updateBatchProgress({
          currentAccount: accountName,
          message: '正在处理',
          processedAccounts: index,
        })
        try {
          await syncOfflineReply(uid, { enableOfflineReply: true, offlineReplyText })
          updateBatchProgress((prev) => ({
            successItems: prev.successItems + 1,
          }))
        } catch (e) {
          errors.push(`${accountName} 账号级话术保存失败：${e?.message || e}`)
          updateBatchProgress((prev) => ({
            failedAccounts: prev.failedAccounts + 1,
            errors: errors.slice(-8),
          }))
        }
        updateBatchProgress({ processedAccounts: index + 1 })
      }
      if (!isScopeCurrent(scope, activeAccountKey)) return
      updateBatchProgress((prev) => ({
        status: errors.length ? 'partial' : 'done',
        message: errors.length ? `已完成，${errors.length} 项失败` : '已完成',
        processedAccounts: activeAccounts.length,
        currentAccount: '',
        errors: errors.slice(-8),
      }))
      notify(
        errors.length
          ? `不在线回复已处理 ${activeAccounts.length} 个账号，存在 ${errors.length} 个账号失败`
          : `已应用不在线回复到 ${activeAccounts.length} 个账号`,
        errors.length ? 'error' : 'success',
      )
    } catch (e) {
      if (isScopeCurrent(scope, activeAccountKey)) {
        updateBatchProgress({
          status: 'failed',
          message: e?.message || '应用全部账号不在线回复失败',
          errors: [e?.message || '应用全部账号不在线回复失败'],
        })
        notify(e?.message || '应用全部账号不在线回复失败', 'error')
      }
    } finally {
      if (isScopeCurrent(scope, activeAccountKey)) setGlobalSaving('')
    }
  }

  const progressPercent = batchProgress
    ? Math.max(
      0,
      Math.min(
        100,
        Math.round((batchProgress.processedAccounts / Math.max(1, batchProgress.totalAccounts)) * 100),
      ),
    )
    : 0

  return (
    <div className={compact ? 'mobile-page mobile-page-stack' : 'page-root'}>
      {compact ? <TopBar title="不在线回复" /> : null}
      {!compact && <HubTabs tabs={HUB_PRESETS.automation} />}

      <main className={compact ? 'mobile-content-stack' : 'p-4 md:p-6 space-y-4'}>
        <section className={compact ? 'mobile-surface-card p-4 space-y-4' : 'surface-card rounded-2xl p-4 md:p-5 space-y-4'}>
          <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
            <div>
              <div className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>不在线回复配置</div>
            </div>
            <button
              type="button"
              className="btn-secondary btn-sm"
              onClick={refreshAccounts}
              disabled={loading}
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
              刷新
            </button>
          </div>

          <div className={pickerShellClass}>
            <AccountCapabilityPicker
              accounts={activeAccounts}
              capability="autoReply"
              selectedUids={selectedUids}
              onChange={handleAccountSelection}
              title="选择配置账号"
              description="勾选账号后可把不在线回复话术写入这些账号，也可以用下方按钮批量应用到全部账号。"
              disabled={loading || accountsLoading || Boolean(globalSaving)}
              allowEnableDisabledAccounts
            />
          </div>

          <div className={compact ? 'mobile-soft-card px-4 py-3 text-[12px]' : 'surface-card rounded-2xl px-4 py-3 text-[12px]'} style={{ color: 'var(--text-secondary)' }}>
            当前将配置 {selectedUids.length} 个账号。不在线回复是账号级配置，保存后会打开所选账号的不在线回复开关并写入话术。
          </div>

          {batchProgress ? (
            <div className={compact ? 'mobile-soft-card p-3.5 space-y-3' : 'rounded-xl border p-3.5 space-y-3'} style={compact ? undefined : { borderColor: 'var(--border-light)' }}>
              <div className="flex flex-col gap-1 md:flex-row md:items-center md:justify-between">
                <div>
                  <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{batchProgress.title}</div>
                  <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                    {batchProgress.message}{batchProgress.currentAccount ? `：${batchProgress.currentAccount}` : ''}
                  </div>
                </div>
                <div className="text-[12px] font-semibold" style={{ color: batchProgress.status === 'failed' || batchProgress.status === 'partial' ? 'var(--danger)' : 'var(--primary)' }}>
                  {progressPercent}%
                </div>
              </div>
              <div className="h-2 overflow-hidden rounded-full" style={{ background: 'var(--surface-sunken)' }}>
                <div className="h-full rounded-full transition-all" style={{ width: `${progressPercent}%`, background: batchProgress.status === 'failed' || batchProgress.status === 'partial' ? 'var(--danger)' : 'var(--primary)' }} />
              </div>
              <div className="grid gap-2 text-[12px] md:grid-cols-4" style={{ color: 'var(--text-secondary)' }}>
                <div>账号：{batchProgress.processedAccounts}/{batchProgress.totalAccounts}</div>
                <div>成功：{batchProgress.successItems}</div>
                <div>失败：{batchProgress.failedItems + batchProgress.failedAccounts}</div>
              </div>
              {batchProgress.errors?.length ? (
                <div className="rounded-lg border border-dashed p-2 text-[12px] space-y-1" style={{ borderColor: 'var(--danger)', color: 'var(--danger)' }}>
                  {batchProgress.errors.map((entry, index) => (
                    <div key={`${entry}-${index}`}>{entry}</div>
                  ))}
                </div>
              ) : null}
            </div>
          ) : null}

          <div className={compact ? 'mobile-soft-card p-3.5 space-y-3' : 'rounded-xl border p-3.5 space-y-3'} style={compact ? undefined : { borderColor: 'var(--border-light)' }}>
            <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
              <div>
                <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>全局不在线回复</div>
                <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                  不在线回复是账号级配置，可保存所选账号，也可批量应用到全部账号。
                </div>
              </div>
            </div>
            <textarea
              className="input min-h-[92px] resize-y"
              value={globalForm.offlineReplyText}
              onChange={(event) => setGlobalForm((prev) => ({ ...prev, offlineReplyText: event.target.value }))}
              placeholder="账号级不在线回复话术"
            />
            <div className="flex flex-col gap-2 md:flex-row md:justify-end">
              <button
                type="button"
                className="btn-secondary btn-sm disabled:opacity-40"
                onClick={saveSelectedOfflineReply}
                disabled={!selectedUids.length || loading || Boolean(globalSaving)}
              >
                {globalSaving === 'offline' ? '保存中…' : '保存所选账号话术'}
              </button>
              <button
                type="button"
                className="btn-primary btn-sm disabled:opacity-40"
                onClick={applyOfflineReplyToAllAccounts}
                disabled={accountsLoading || loading || activeAccounts.length === 0 || Boolean(globalSaving)}
              >
                {globalSaving === 'offline-all-accounts' ? '应用中…' : '应用到全部账号'}
              </button>
            </div>
          </div>

          {error ? (
            <div className="rounded-xl border border-dashed p-3 text-[12px]" style={{ borderColor: 'var(--danger)', color: 'var(--danger)' }}>
              {error}
            </div>
          ) : null}
        </section>
      </main>
    </div>
  )
}
