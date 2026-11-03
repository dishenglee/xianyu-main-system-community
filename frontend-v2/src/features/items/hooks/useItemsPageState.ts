// @ts-nocheck
// Shared business state + handlers for the items page.
// Both DesktopItems and MobileItems consume this hook so logic lives in one place;
// the two pages only differ in their render layer.
//
// Returned shape:
//   {
//     // raw state
//     accounts, activeAccounts, groups, allItems, filtered, counts,
//     loading, syncing, syncingUids, polishingUids, polishedUids, batchRunning, batchProgress,
//     // filter state
//     accountFilter, statusFilter, searchQuery, sortKey,
//     setAccountFilter, setStatusFilter, setSearchQuery, setSortKey,
//     // select mode + dialog state
//     selectMode, selectedKeys, ..., dialog, grouponSheet, editingItem, publishOpen,
//     // setters / openers
//     enterSelectMode, exitSelectMode, toggleSelect, toggleSelectAll, itemKey,
//     openPublish, closePublish, openEdit, closeEdit, openGroupon, closeGrouponSheet,
//     showAlert, showConfirm, showPrompt, showChoice,
//     setGrouponSheet,
//     // actions
//     loadAll, doSync, doSyncAll, doPolish, doPolishAll, doReducePrice, doDownshelf,
//     doUpshelf, doDelete, doShare, doToggleCoin, doToggleGroupon, submitGrouponSheet,
//     doBatchDelete, doBatchDownshelf, doBatchUpshelf, doBatchReducePrice, doBatchCoinOpen,
//     runBatchPublish, doEditItem,
//   }
import { useEffect, useMemo, useRef, useState } from 'react'
import { useAccounts } from '../../../hooks/useAccounts'
import { limitConcurrency } from '../../../hooks/limitConcurrency'
import { listenBatchedSyncEvent, XIANYU_ITEMS_SYNCED_EVENT } from '../../runtimeEvents'
import {
  itemAllList, itemSoldOutList, itemPolish, itemDownshelf, republishItem,
  itemDelete, syncItems, syncAllItems, reducePrice,
  itemShare, itemCoinDeductionOpen, itemCoinDeductionClose,
  itemGrouponOpen, itemGrouponClose, itemGrouponStatus, itemGrouponGuide,
  coinDeductionRatio, enableCoinDeduction, enableGroupon,
  refreshAccount,
} from '../../../api/index'
import { getAuthToken } from '../../../api/client'
import { useSafeNotify } from '../../../components/Notify'
import { useTaskProgressStore } from '../../../store/taskProgressStore'
import { useAuthStore } from '../../../store/authStore'
import {
  extractItems, colorForAccount,
  normalizePriceText, normalizeGrouponSubmitPrice,
  buildGrouponOptions, grouponPriceValidation,
  coinRatioOptionsFrom, GROUPON_FIXED_AMOUNTS, coinCanClose, coinCloseRemainingText,
  markdownAmountValidation, itemPublishTimeMs,
} from '../itemUtils'
import { statusOf, isOnSaleListed, cleanMtopErr, isCoinAgreementError, mtopOutcome, runBatchPublishJobs } from '../itemActions'
import { isSuccess, retMsg } from '../../../api/client'
import { filterBusinessAccounts } from '../../../utils/accountAccess'
import { emitActionFailure, emitActionLog, emitActionStarted, emitActionSuccess } from '../../../bootstrap/actionLogging'

export function useItemsPageState() {
  const { accounts, loadAccounts } = useAccounts()
  const notify = useSafeNotify()
  const taskProgressStore = useTaskProgressStore()
  const authToken = useAuthStore((state) => state.token)
  const effectiveAuthToken = authToken || getAuthToken()

  const [loading, setLoading] = useState(false)
  const [groups, setGroups] = useState<any[]>([])
  const [syncing, setSyncing] = useState(false)
  const [syncingUids, setSyncingUids] = useState<Record<string, boolean>>({})
  const [soldOutItems, setSoldOutItems] = useState<Record<string, any[]>>({})
  const [loadingSoldOut, setLoadingSoldOut] = useState<Record<string, boolean>>({})
  const [polishingUids, setPolishingUids] = useState<Record<string, boolean>>({})
  const [polishedUids, setPolishedUids] = useState<Record<string, boolean>>({})
  const [editingItem, setEditingItem] = useState<any>(null)
  const [publishOpen, setPublishOpen] = useState(false)
  const lastLoadedAccountKeyRef = useRef('')

  // 批量选择
  const [selectMode, setSelectMode] = useState(false)
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set())
  const [batchRunning, setBatchRunning] = useState(false)
  const [batchProgress, setBatchProgress] = useState<null | { total: number; message: string }>(null)

  // 筛选
  const [accountFilter, setAccountFilter] = useState<string | null>(null)
  const [desktopAccountSelection, setDesktopAccountSelection] = useState<null | { accountKey: string; uids: string[] }>(null)
  const [statusFilter, setStatusFilter] = useState('onsale')
  const [searchQuery, setSearchQuery] = useState('')
  const [sortKey, setSortKey] = useState('published_desc')

  // 对话框（WKWebView 不支持原生 confirm/prompt/alert）
  const [dialog, setDialog] = useState<any>(null)
  const [grouponSheet, setGrouponSheet] = useState<any>(null)
  const grouponSettingsBusyRef = useRef<Set<string>>(new Set())

  const activeAccounts = useMemo(
    () => filterBusinessAccounts(accounts),
    [accounts],
  )
  const activeAccountKey = useMemo(
    () => activeAccounts.map((account: any) => account.uid).join('|'),
    [activeAccounts],
  )
  const desktopAccountKey = useMemo(
    () => activeAccounts.map((account: any) => account.uid).sort().join('|'),
    [activeAccounts],
  )
  const desktopAccountFilterUids = desktopAccountSelection?.accountKey === desktopAccountKey
    ? desktopAccountSelection.uids
    : null
  const accountScopeUids = desktopAccountFilterUids ?? (accountFilter ? [accountFilter] : [])
  const accountScopeKey = [...accountScopeUids].sort().join('|')
  const activeAccountKeyRef = useRef(activeAccountKey)
  activeAccountKeyRef.current = activeAccountKey

  function setDesktopAccountFilterUids(uids: string[]) {
    setDesktopAccountSelection({ accountKey: desktopAccountKey, uids: [...uids] })
  }

  function clearItemsState() {
    setGroups([])
    setSyncing(false)
    setSyncingUids({})
    setSoldOutItems({})
    setLoadingSoldOut({})
    setPolishingUids({})
    setPolishedUids({})
    setEditingItem(null)
    setPublishOpen(false)
    setSelectMode(false)
    setSelectedKeys(new Set())
    setBatchRunning(false)
    setBatchProgress(null)
    setGrouponSheet(null)
  }

  function itemKey(it: any) { return `${it._uid}:${it.itemId}` }
  function toggleSelect(it: any) {
    const k = itemKey(it)
    setSelectedKeys((prev) => {
      const next = new Set(prev)
      next.has(k) ? next.delete(k) : next.add(k)
      return next
    })
  }
  function toggleSelectAll() {
    if (selectedKeys.size === filtered.length) setSelectedKeys(new Set())
    else setSelectedKeys(new Set(filtered.map(itemKey)))
  }
  function exitSelectMode() { setSelectMode(false); setSelectedKeys(new Set()) }
  function enterSelectMode() { setSelectMode(true) }
  function selectedItemsList() {
    return filtered.filter((it: any) => selectedKeys.has(itemKey(it)))
  }

  const showAlert = (message: string) => {
    const text = String(message || '操作提示')
    const type = /失败|错误|异常|无效|不能|不合法|未拿到|没有|请输入|请选择|请先|仍有/.test(text)
      ? 'error'
      : /成功|完成|已开启|已关闭|已修改|已更新|已复制|下架成功|删除成功/.test(text)
        ? 'success'
        : 'info'
    notify(text, type, type === 'error' ? 4200 : 3000)
    return Promise.resolve()
  }
  const showConfirm = (message: string, opts: any = {}) =>
    new Promise<boolean>((res) => setDialog({
      kind: 'confirm', title: opts.title, message,
      danger: opts.danger, confirmLabel: opts.confirmLabel,
      resolve: (v: any) => { setDialog(null); res(v === true) },
    }))
  const showPrompt = (message: string, defaultValue = '', opts: any = {}) =>
    new Promise<string | null>((res) => setDialog({
      kind: 'prompt', title: opts.title, message, defaultValue,
      placeholder: opts.placeholder, confirmLabel: opts.confirmLabel,
      resolve: (v: any) => { setDialog(null); res(typeof v === 'string' ? v : null) },
    }))
  const showChoice = (message: string, options: any[], opts: any = {}) =>
    new Promise<string | null>((res) => setDialog({
      kind: 'choice', title: opts.title, message, options,
      resolve: (v: any) => { setDialog(null); res(typeof v === 'string' ? v : null) },
    }))
  const closeGrouponSheet = () => setGrouponSheet(null)

  function batchItemLabel(it: any) {
    const nick = String(it?._nick || '').trim()
    const title = String(it?.title || it?.itemId || '商品').trim()
    return nick ? `${nick} · ${title}` : title
  }

  function itemActionSummary(extra: Record<string, unknown> = {}) {
    return {
      activeAccountCount: activeAccounts.length,
      totalItemCount: allItems.length,
      selectedCount: selectedKeys.size,
      accountFilter: accountFilter || '',
      statusFilter,
      ...extra,
    }
  }

  async function refreshSuspectBatchAccounts(items: any[]) {
    const suspectUids = [...new Set(
      items
        .map((it: any) => String(it?._uid || ''))
        .filter(Boolean)
        .filter((uid) => {
          const account = activeAccounts.find((a: any) => a.uid === uid)
          const lastStatus = String(account?.last_session_status || '').trim().toLowerCase()
          const sessionReason = String(
            account?.last_session_error
            || account?.last_error
            || account?.last_check_message
            || '',
          ).toUpperCase()
          return lastStatus === 'error'
            || sessionReason.includes('SESSION_EXPIRED')
            || sessionReason.includes('NOT_LOGIN')
            || sessionReason.includes('TOKEN')
        }),
    )]
    if (!suspectUids.length) return
    setBatchProgress({
      total: items.length,
      message: `正在预刷新 ${suspectUids.length} 个会话异常账号…`,
    })
    await Promise.allSettled(suspectUids.map((uid) => refreshAccount(uid)))
    await loadAccounts()
  }

  async function runSequentialItemBatch({
    title,
    items,
    run,
    successText,
    onSuccess,
    afterSync = true,
    interItemDelayMs = 0,
    stopOnError,
    blockUidOnError,
    preserveUnsuccessfulSelection = false,
  }: {
    title: string
    items: any[]
    run: (it: any) => Promise<any>
    successText: string
    onSuccess?: (it: any, resp: any) => void
    afterSync?: boolean
    interItemDelayMs?: number
    stopOnError?: (error: any) => string | false
    blockUidOnError?: (error: any) => string | false
    preserveUnsuccessfulSelection?: boolean
  }) {
    if (!items.length) return { ok: 0, fail: 0, skipped: 0, results: [] as any[] }
    emitActionStarted({
      module: 'items',
      action: 'batch_item_operation',
      message: `开始${title}`,
      context: itemActionSummary({
        operation: title,
        itemCount: items.length,
        accountCount: new Set(items.map((it: any) => String(it?._uid || ''))).size,
      }),
    })
    setBatchRunning(true)
    setBatchProgress({ total: items.length, message: `${title}中 0/${items.length}` })
    taskProgressStore.start(items.length, title)
    const progressResults: { uid: string; label: string; ok: boolean; msg: string; item?: any }[] = []
    const affectedUids = new Set<string>()
    let ok = 0
    let fail = 0
    let skipped = 0
    let stopped = false
    const blockedUids = new Map<string, string>()
    try {
      await refreshSuspectBatchAccounts(items)
      let done = 0
      for (let index = 0; index < items.length; index += 1) {
        if (stopped) break
        const it = items[index]
        const uid = String(it?._uid || '')
        const label = batchItemLabel(it)
        const blockedReason = blockedUids.get(uid)
        if (blockedReason) {
          skipped += 1
          done += 1
          progressResults.push({
            uid,
            label,
            ok: false,
            skipped: true,
            msg: blockedReason,
            item: it,
          })
          setBatchProgress({
            total: items.length,
            message: `${title}中 ${done}/${items.length}，成功 ${ok}，失败 ${fail}，跳过 ${skipped}`,
          })
          taskProgressStore.tick(done, [...progressResults])
          continue
        }
        try {
          const resp: any = await run(it)
          const outcome = mtopOutcome(resp)
          if (!outcome.ok) {
            const error: any = new Error(outcome.msg || '执行失败')
            error.code = outcome.code || ''
            throw error
          }
          ok += 1
          affectedUids.add(uid)
          onSuccess?.(it, resp)
          progressResults.push({ uid, label, ok: true, msg: '成功', item: it })
        } catch (e: any) {
          fail += 1
          progressResults.push({ uid, label, ok: false, msg: cleanMtopErr(e?.message || '失败'), item: it })
          const blockReason = blockUidOnError?.(e)
          if (blockReason && uid) blockedUids.set(uid, blockReason)
          const stopReason = stopOnError?.(e)
          if (stopReason) {
            done += 1
            setBatchProgress({ total: items.length, message: `${title}已停止：成功 ${ok}，失败 ${fail}，跳过 ${items.length - index - 1}` })
            taskProgressStore.tick(done, [...progressResults])
            for (const remaining of items.slice(index + 1)) {
              skipped += 1
              done += 1
              progressResults.push({
                uid: String(remaining?._uid || ''),
                label: batchItemLabel(remaining),
                ok: false,
                skipped: true,
                msg: stopReason,
                item: remaining,
              })
              taskProgressStore.tick(done, [...progressResults])
            }
            stopped = true
          }
        } finally {
          if (stopped) continue
          done += 1
          setBatchProgress({
            total: items.length,
            message: `${title}中 ${done}/${items.length}，成功 ${ok}，失败 ${fail}`,
          })
          taskProgressStore.tick(done, [...progressResults])
        }
        if (interItemDelayMs > 0 && index < items.length - 1) {
          await new Promise((resolve) => window.setTimeout(resolve, interItemDelayMs))
        }
      }

      if (afterSync) {
        setBatchProgress({
          total: items.length,
          message: `正在同步 ${affectedUids.size} 个受影响账号…`,
        })
        for (const uid of affectedUids) await doSync(uid, { silent: true }).catch(() => {})
      }
      if (preserveUnsuccessfulSelection) {
        const retryKeys = progressResults
          .filter((row: any) => !row?.ok && row?.item)
          .map((row: any) => itemKey(row.item))
        setSelectedKeys(new Set(retryKeys))
        setSelectMode(retryKeys.length > 0)
      } else {
        exitSelectMode()
      }
      setBatchProgress(null)
      taskProgressStore.finish([...progressResults])
      const firstFail = progressResults.find((row: any) => !row?.ok)
      if (firstFail) {
        await showAlert(`${successText}：${ok} 成功，${fail} 失败，${skipped} 跳过\n首个失败：${firstFail.label}：${cleanMtopErr(firstFail?.msg || '执行失败')}`)
      } else {
        notify(`${successText}：${ok} 成功，${fail} 失败，${skipped} 跳过`, fail > 0 ? 'error' : 'success', 4200)
      }
      emitActionLog({
        module: 'items',
        action: 'batch_item_operation',
        outcome: fail > 0 ? 'warn' : 'success',
        message: `${title}完成`,
        retryable: fail > 0,
        promoteToIncident: false,
        context: itemActionSummary({
          operation: title,
          itemCount: items.length,
          successCount: ok,
          failureCount: fail,
          skippedCount: skipped,
          affectedAccountCount: affectedUids.size,
        }),
      })
      return { ok, fail, skipped, results: progressResults }
    } catch (e: any) {
      if (!progressResults.length) {
        taskProgressStore.finish(items.map((it: any) => ({
          uid: String(it?._uid || ''),
          label: batchItemLabel(it),
          ok: false,
          msg: cleanMtopErr(e?.message || '执行失败'),
        })))
      } else {
        taskProgressStore.finish([...progressResults])
      }
      emitActionFailure({
        module: 'items',
        action: 'batch_item_operation',
        message: `${title}异常中断`,
        error: e,
        context: itemActionSummary({
          operation: title,
          itemCount: items.length,
          completedCount: progressResults.length,
        }),
      })
      throw e
    } finally {
      setBatchRunning(false)
      setBatchProgress(null)
    }
  }

  // ---- 业务方法 ----
  async function loadAll() {
    if (!activeAccounts.length) {
      clearItemsState()
      lastLoadedAccountKeyRef.current = ''
      return
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setLoading(true)
    try {
      const results = await limitConcurrency(activeAccounts, 5, async (account: any) => {
        try {
          const r = await itemAllList(account.uid)
          const items = extractItems(r)
          const lastSyncedAt = Number(r?._last_synced_at || 0)
          return { uid: account.uid, nick: account.nick, avatar: account.avatar, items, lastSyncedAt }
        } catch {
          return { uid: account.uid, nick: account.nick, avatar: account.avatar, items: [], lastSyncedAt: 0 }
        }
      })
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      setGroups(results)
    } finally {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
        setLoading(false)
      }
    }
  }

  async function doSyncAll() {
    setSyncing(true)
    emitActionStarted({
      module: 'items',
      action: 'sync_all_items',
      message: '开始同步全部账号商品',
      context: itemActionSummary({ accountCount: activeAccounts.length }),
    })
    try {
      const summary = await syncAllItems()
      await loadAll()
      const failed = (summary.accounts || []).filter((a: any) => !a.ok)
      if (failed.length) {
        const first = failed[0]
        const acc = activeAccounts.find((a: any) => a.uid === first.uid)
        const reason = String(first.detail || '同步失败')
        notify(
          `已同步 ${summary.success_count}/${summary.total} 个账号，` +
            `失败：${acc?.nick || first.uid}，${cleanMtopErr(reason)}`,
          'error', 4200,
        )
      } else {
        notify(`已同步 ${summary.success_count} 个账号商品`, 'success')
      }
      emitActionLog({
        module: 'items',
        action: 'sync_all_items',
        outcome: failed.length ? 'warn' : 'success',
        message: failed.length ? '商品同步部分失败' : '全部账号商品同步成功',
        retryable: failed.length > 0,
        promoteToIncident: false,
        context: itemActionSummary({
          total: summary.total,
          successCount: summary.success_count,
          failureCount: failed.length,
        }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'items',
        action: 'sync_all_items',
        message: '全部账号商品同步失败',
        error: e,
        context: itemActionSummary({ accountCount: activeAccounts.length }),
      })
      notify(cleanMtopErr(e?.message || '同步失败'), 'error', 4200)
    } finally { setSyncing(false) }
  }

  async function doSync(uid: string, opts: { silent?: boolean } = {}) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setSyncingUids((p) => ({ ...p, [uid]: true }))
    if (!opts?.silent) {
      emitActionStarted({
        module: 'items',
        action: 'sync_account_items',
        message: '开始同步单账号商品',
        context: itemActionSummary({ uid }),
      })
    }
    try {
      const result: any = await syncItems(uid)
      // 已下架列表是独立缓存。只刷新 ALL 会让旧 SOLD_OUT 记录在聚合时覆盖
      // 刚同步到的 ON_SALE 记录，表现为“重新上架成功但仍留在下架列表”。
      const shouldRefreshSoldOut = soldOutItems[uid] !== undefined || statusFilter === 'off'
      const [r, soldOutResp] = await Promise.all([
        itemAllList(uid),
        shouldRefreshSoldOut ? itemSoldOutList(uid).catch(() => null) : Promise.resolve(null),
      ])
      const items = extractItems(r)
      const refreshedSoldOutItems = soldOutResp ? extractItems(soldOutResp) : null
      const lastSyncedAt = Number(r?._last_synced_at || 0)
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      setGroups((prev) => prev.map((g) => g.uid === uid ? { ...g, items, lastSyncedAt } : g))
      if (refreshedSoldOutItems) {
        setSoldOutItems((prev) => ({ ...prev, [uid]: refreshedSoldOutItems }))
      }
      if (result?.ok === false) {
        if (!opts?.silent) {
          emitActionLog({
            module: 'items',
            action: 'sync_account_items',
            outcome: 'warn',
            message: '单账号商品同步未成功',
            detail: cleanMtopErr(result?.detail || '商品同步失败'),
            retryable: true,
            promoteToIncident: false,
            context: itemActionSummary({ uid }),
          })
        }
        notify(cleanMtopErr(result?.detail || '商品同步失败'), 'error', 4200)
      } else if (!opts?.silent) {
        notify('商品同步完成', 'success')
        emitActionSuccess({
          module: 'items',
          action: 'sync_account_items',
          message: '单账号商品同步成功',
          context: itemActionSummary({ uid, itemCount: items.length }),
        })
      }
    } catch (e: any) {
      if (!opts?.silent) {
        emitActionFailure({
          module: 'items',
          action: 'sync_account_items',
          message: '单账号商品同步失败',
          error: e,
          context: itemActionSummary({ uid }),
        })
      }
      throw e
    } finally {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
        setSyncingUids((p) => ({ ...p, [uid]: false }))
      }
    }
  }

  async function doSyncCurrentAccounts() {
    if (!accountScopeUids.length) {
      await doSyncAll()
      return
    }
    await Promise.all(accountScopeUids.map((uid) => doSync(uid, { silent: true })))
  }

  async function runBatchPublish(params: any) {
    setPublishOpen(false)
    await runBatchPublishJobs(
      params,
      activeAccounts,
      taskProgressStore,
      (uid: string) => doSync(uid, { silent: true }),
    )
  }

  function polishableItemsForUids(uids: string[]) {
    const uidSet = new Set(uids)
    return allItems.filter((it: any) => {
      if (!uidSet.has(String(it?._uid || ''))) return false
      if (!it?.itemId) return false
      if (!isOnSaleListed(it)) return false
      if (it?.canPolish === false || String(it?.canPolish).toLowerCase() === 'false') return false
      return true
    })
  }

  async function runPolishItems(targets: any[], title = '一键擦亮') {
    const uids = targets.map((a: any) => a.uid)
    const items = polishableItemsForUids(uids)
    if (!items.length) {
      emitActionLog({
        module: 'items',
        action: 'polish_items',
        outcome: 'warn',
        message: '没有可擦亮的在售商品',
        retryable: false,
        promoteToIncident: false,
        context: itemActionSummary({ accountCount: targets.length }),
      })
      await showAlert('当前没有可擦亮的在售商品')
      return
    }
    emitActionStarted({
      module: 'items',
      action: 'polish_items',
      message: `开始${title}`,
      context: itemActionSummary({
        operation: title,
        accountCount: targets.length,
        itemCount: items.length,
      }),
    })
    setPolishingUids((p) => { const n = { ...p }; for (const u of uids) n[u] = true; return n })
    setPolishedUids((p) => { const n = { ...p }; for (const u of uids) delete n[u]; return n })
    taskProgressStore.start(items.length, title)
    const progressResults: { uid: string; label: string; ok: boolean; msg: string }[] = []
    const affectedUids = new Set<string>()
    const failedUids = new Set<string>()
    try {
      let done = 0
      for (const item of items) {
        const uid = String(item._uid || '')
        const account = targets.find((a: any) => a.uid === uid)
        const label = `${account?.nick || item._nick || uid} · ${item.title || item.itemId}`
        try {
          const resp: any = await itemPolish(uid, item.itemId)
          const outcome = mtopOutcome(resp)
          if (!outcome.ok) throw new Error(outcome.msg || '擦亮失败')
          progressResults.push({ uid, label, ok: true, msg: '成功' })
          affectedUids.add(uid)
        } catch (e: any) {
          progressResults.push({ uid, label, ok: false, msg: cleanMtopErr(e?.message || '失败') })
          failedUids.add(uid)
        } finally {
          done += 1
          taskProgressStore.tick(done, [...progressResults])
        }
      }
      const completedUids = uids.filter((uid) => affectedUids.has(uid) && !failedUids.has(uid))
      setPolishedUids((p) => {
        const next = { ...p }
        for (const uid of completedUids) next[uid] = true
        return next
      })
      for (const uid of affectedUids) await doSync(uid, { silent: true }).catch(() => {})
      taskProgressStore.finish([...progressResults])
      emitActionLog({
        module: 'items',
        action: 'polish_items',
        outcome: failedUids.size > 0 ? 'warn' : 'success',
        message: `${title}完成`,
        retryable: failedUids.size > 0,
        promoteToIncident: false,
        context: itemActionSummary({
          operation: title,
          accountCount: targets.length,
          itemCount: items.length,
          successCount: progressResults.filter((row) => row.ok).length,
          failureCount: progressResults.filter((row) => !row.ok).length,
        }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'items',
        action: 'polish_items',
        message: `${title}异常中断`,
        error: e,
        context: itemActionSummary({
          operation: title,
          accountCount: targets.length,
          itemCount: items.length,
          completedCount: progressResults.length,
        }),
      })
      throw e
    } finally {
      setPolishingUids((p) => { const n = { ...p }; for (const u of uids) n[u] = false; return n })
    }
  }

  async function doPolish(uid: string) {
    const target = activeAccounts.find((account: any) => account.uid === uid)
    if (!target) return
    await runPolishItems([target], '擦亮')
  }

  async function doPolishAll() {
    const selectedUidSet = new Set(accountScopeUids)
    const targets = accountScopeUids.length
      ? activeAccounts.filter((a: any) => selectedUidSet.has(a.uid))
      : activeAccounts
    if (!targets.length) return
    const items = polishableItemsForUids(targets.map((a: any) => a.uid))
    if (!items.length) {
      await showAlert('当前没有可擦亮的在售商品')
      return
    }
    if (!(await showConfirm(`确认对 ${targets.length} 个账号的 ${items.length} 件在售商品逐个擦亮？`, { title: '一键擦亮' }))) return
    await runPolishItems(targets, '一键擦亮')
  }

  async function doReducePrice(uid: string, item: any) {
    const raw = await showPrompt(
      `${item.title || '商品'} 当前 ¥${item.price}\n降价金额（元，整数或小数，例如 50 / 5.5）：`,
      '10', { title: '降价', placeholder: '降价金额', confirmLabel: '降价' },
    )
    if (raw == null) return
    const amount = parseFloat(raw)
    if (!amount || amount <= 0) { await showAlert('降价金额无效'); return }
    const validation = markdownAmountValidation(String(amount), item?.price, item?.grouponOpen ? item?.grouponPrice : '')
    if (!validation.ok) { await showAlert(validation.message); return }
    try {
      emitActionStarted({
        module: 'items',
        action: 'reduce_item_price',
        message: '开始单商品降价',
        context: itemActionSummary({ uid, itemId: item.itemId, amount }),
      })
      const resp: any = await reducePrice(uid, item.itemId, amount)
      const outcome = mtopOutcome(resp)
      if (!outcome.ok) {
        emitActionLog({
          module: 'items',
          action: 'reduce_item_price',
          outcome: 'warn',
          message: '单商品降价未成功',
          detail: outcome.msg || '降价失败',
          retryable: true,
          promoteToIncident: false,
          context: itemActionSummary({ uid, itemId: item.itemId, amount }),
        })
        await showAlert(outcome.msg || '降价失败')
        return
      }
      await doSync(uid, { silent: true })
      notify(`已降价 ¥${amount.toFixed(2)}`, 'success')
      emitActionSuccess({
        module: 'items',
        action: 'reduce_item_price',
        message: '单商品降价成功',
        context: itemActionSummary({ uid, itemId: item.itemId, amount }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'items',
        action: 'reduce_item_price',
        message: '单商品降价失败',
        error: e,
        context: itemActionSummary({ uid, itemId: item.itemId, amount }),
      })
      await showAlert(cleanMtopErr(e.message || '降价失败'))
    }
  }

  function doEditItem(item: any) {
    if (!item?.itemId || !item?._uid) return
    setEditingItem({ uid: item._uid, item, mode: 'edit' })
  }

  async function loadSoldOut(uid: string) {
    if (soldOutItems[uid] !== undefined) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setLoadingSoldOut((p) => ({ ...p, [uid]: true }))
    try {
      const r = await itemSoldOutList(uid)
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      setSoldOutItems((p) => ({ ...p, [uid]: extractItems(r) }))
    } catch {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
        setSoldOutItems((p) => ({ ...p, [uid]: [] }))
      }
    }
    finally {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
        setLoadingSoldOut((p) => ({ ...p, [uid]: false }))
      }
    }
  }

  async function doDownshelf(uid: string, item: any) {
    if (!(await showConfirm(`确认下架商品「${item.title || item.itemId}」？`, { title: '下架商品' }))) return
    try {
      const r: any = await itemDownshelf(uid, item.itemId)
      const o = mtopOutcome(r)
      await showAlert(o.ok ? `下架成功：${o.msg}` : `下架失败：${o.msg}`)
      if (o.ok) await doSync(uid, { silent: true })
    } catch (e: any) { await showAlert(`下架失败：${cleanMtopErr(e?.message || '网络错误')}`) }
  }
  async function doUpshelf(uid: string, item: any) {
    setEditingItem({ uid, item, mode: 'republish' })
  }
  async function doDelete(uid: string, item: any) {
    if (!(await showConfirm(`删除后无法恢复，确认删除「${item.title || item.itemId}」？`, { title: '删除商品', danger: true, confirmLabel: '删除' }))) return
    try {
      const r: any = await itemDelete(uid, item.itemId)
      const o = mtopOutcome(r)
      if (o.ok) {
        removeItemLocally(uid, item.itemId)
      }
      await showAlert(o.ok ? `删除成功：${o.msg}` : `删除失败：${o.msg}`)
    } catch (e: any) { await showAlert(`删除失败：${cleanMtopErr(e?.message || '网络错误')}`) }
  }

  // ========== 批量操作 ==========
  async function doBatchAction(
    label: string,
    action: (uid: string, itemId: string) => Promise<any>,
    opts?: { danger?: boolean; prompt?: boolean },
  ) {
    const items = selectedItemsList()
    if (!items.length) return
    const confirmMsg = opts?.danger
      ? `确认对 ${items.length} 个商品执行「${label}」？此操作不可撤销。`
      : `确认对 ${items.length} 个商品执行「${label}」？`
    if (!(await showConfirm(confirmMsg, { title: `批量${label}`, danger: opts?.danger, confirmLabel: label }))) return

    let promptValue = ''
    if (opts?.prompt) {
      const raw = await showPrompt(`${items.length} 个商品统一降价金额（元）：`, '10', { title: `批量${label}`, placeholder: '降价金额', confirmLabel: label })
      if (raw == null) return
      const v = parseFloat(raw)
      if (!v || v <= 0) { await showAlert('降价金额无效'); return }
      promptValue = String(v)
    }

    setBatchRunning(true)
    let ok = 0, fail = 0
    const affectedUids = new Set<string>()
    for (const it of items) {
      try {
        const r: any = opts?.prompt
          ? await reducePrice(it._uid, it.itemId, parseFloat(promptValue))
          : await action(it._uid, it.itemId)
        const o = mtopOutcome(r)
        if (o.ok) { ok++; affectedUids.add(it._uid) } else { fail++ }
      } catch { fail++ }
    }
    for (const uid of affectedUids) await doSync(uid, { silent: true }).catch(() => {})
    setBatchRunning(false)
    exitSelectMode()
    notify(`批量${label}完成：${ok} 成功，${fail} 失败`, fail > 0 ? 'error' : 'success', 4200)
  }

  function selectedItemsWithPredicate(
    predicate: (it: any) => boolean,
  ) {
    return selectedItemsList().filter(predicate)
  }

  async function doBatchDelete() {
    const items = selectedItemsList()
    if (!items.length) return
    if (!(await showConfirm(
      `删除后无法恢复，确认删除 ${items.length} 个商品？`,
      { title: '批量删除', danger: true, confirmLabel: '删除' },
    ))) return

    await runSequentialItemBatch({
      title: '批量删除商品',
      items,
      run: (it) => itemDelete(it._uid, it.itemId),
      onSuccess: (it) => removeItemLocally(it._uid, it.itemId),
      successText: '批量删除完成',
      afterSync: false,
    }).catch((e: any) => showAlert(cleanMtopErr(e?.message || '批量删除失败')))
  }
  async function doBatchDownshelf() {
    const items = selectedItemsList()
    if (!items.length) return
    if (!(await showConfirm(`确认对 ${items.length} 个商品执行「下架」？`, { title: '批量下架', confirmLabel: '下架' }))) return
    await runSequentialItemBatch({
      title: '批量下架商品',
      items,
      run: (it) => itemDownshelf(it._uid, it.itemId),
      successText: '批量下架完成',
    }).catch((e: any) => showAlert(cleanMtopErr(e?.message || '批量下架失败')))
  }
  async function doBatchUpshelf() {
    const items = selectedItemsList()
    if (!items.length) return
    if (!(await showConfirm(`确认对 ${items.length} 个商品执行「上架」？`, { title: '批量上架', confirmLabel: '上架' }))) return
    await runSequentialItemBatch({
      title: '批量上架商品',
      items,
      run: (it) => republishItem(it._uid, it.itemId, it),
      onSuccess: (it) => removeItemLocally(it._uid, it.itemId),
      successText: '批量上架完成',
      interItemDelayMs: 1800,
      blockUidOnError: (error) => `${error?.code || ''} ${error?.message || error || ''}`.toUpperCase().includes('FAIL_SYS_USER_VALIDATE')
        ? '该账号需要先在闲鱼完成人工验证；同账号后续商品已暂停，其他账号继续。'
        : false,
      preserveUnsuccessfulSelection: true,
    }).catch((e: any) => showAlert(cleanMtopErr(e?.message || '批量上架失败')))
  }
  async function doBatchReducePrice() {
    const items = selectedItemsList()
    if (!items.length) return
    const raw = await showPrompt(
      `${items.length} 个商品统一降价额度（元，支持 0.01 - 1 这类小额）：`,
      '0.10',
      { title: '批量降价', placeholder: '降价额度', confirmLabel: '降价' },
    )
    if (raw == null) return
    const amount = parseFloat(raw)
    if (!amount || amount <= 0) {
      await showAlert('降价金额无效')
      return
    }
    const invalid = items.find((it: any) => {
      const validation = markdownAmountValidation(String(amount), it?.price, it?.grouponOpen ? it?.grouponPrice : '')
      return !validation.ok
    })
    if (invalid) {
      const validation = markdownAmountValidation(String(amount), invalid?.price, invalid?.grouponOpen ? invalid?.grouponPrice : '')
      await showAlert(`${validation.message}\n「${invalid.title || invalid.itemId}」当前售价 ¥${invalid.price}${invalid?.grouponOpen && invalid?.grouponPrice ? `，小刀价 ¥${invalid.grouponPrice}` : ''}`)
      return
    }
    if (!(await showConfirm(`确认对 ${items.length} 个商品统一降价 ¥${amount.toFixed(2)}？`, { title: '批量降价', confirmLabel: '降价' }))) return
    await runSequentialItemBatch({
      title: '批量降价',
      items,
      run: (it) => reducePrice(it._uid, it.itemId, amount),
      successText: '批量降价完成',
    }).catch((e: any) => showAlert(cleanMtopErr(e?.message || '批量降价失败')))
  }
  async function doBatchCoinOpen() {
    const items = selectedItemsWithPredicate((it) => !it?.coinOpen)
    if (!items.length) {
      await showAlert('所选商品都已经开启闲鱼币抵扣')
      return
    }
    const raw = await showPrompt(`为 ${items.length} 个未开启商品开启闲鱼币抵扣，选择比例（%）：`, '5', { title: '批量开启闲鱼币', placeholder: '比例，如 5', confirmLabel: '开启' })
    if (raw == null) return
    const pct = parseInt(raw, 10)
    if (!pct || pct <= 0) { await showAlert('比例无效'); return }
    const result = await runSequentialItemBatch({
      title: '批量开启闲鱼币抵扣',
      items,
      run: (it) => itemCoinDeductionOpen(it._uid, it.itemId, String(pct)),
      onSuccess: (it) => patchItemLocally(it._uid, it.itemId, {
        coinOpen: true,
        coinRatio: String(pct),
        coinOpenedAt: it.coinOpenedAt || (Date.now() / 1000),
      }),
      successText: '批量开启闲鱼币完成',
    }).catch((e: any) => {
      void showAlert(cleanMtopErr(e?.message || '批量开启闲鱼币失败'))
      return { results: [] }
    })
    const agreementLabels = new Set<string>()
    for (const row of result.results || []) {
      if (!row?.ok && isCoinAgreementError(row?.msg || '')) {
        agreementLabels.add(row?.item?._nick || row?.uid || '')
      }
    }
    if (agreementLabels.size > 0) {
      await showAlert(`仍有账号返回抵扣协议相关错误：${Array.from(agreementLabels).filter(Boolean).join('、')}`)
    }
  }
  async function doBatchCoinClose() {
    const items = selectedItemsWithPredicate((it) => !!it?.coinOpen)
    if (!items.length) {
      await showAlert('所选商品没有已开启闲鱼币抵扣的商品')
      return
    }
    if (!(await showConfirm(`确认关闭 ${items.length} 个商品的闲鱼币抵扣？`, { title: '批量关闭闲鱼币', confirmLabel: '关闭' }))) return
    await runSequentialItemBatch({
      title: '批量关闭闲鱼币抵扣',
      items,
      run: (it) => itemCoinDeductionClose(it._uid, it.itemId),
      onSuccess: (it) => patchItemLocally(it._uid, it.itemId, { coinOpen: false, coinRatio: '', coinOpenedAt: 0 }),
      successText: '批量关闭闲鱼币完成',
    }).catch((e: any) => showAlert(cleanMtopErr(e?.message || '批量关闭闲鱼币失败')))
  }
  async function doBatchGrouponOpen() {
    const items = selectedItemsWithPredicate((it) => !it?.grouponOpen)
    if (!items.length) {
      await showAlert('所选商品都已经开启 2人小刀')
      return
    }
    const raw = await showPrompt(
      `为 ${items.length} 个未开启商品设置统一小刀金额（元）：`,
      '6.66',
      { title: '批量开启小刀', placeholder: '如 6.66', confirmLabel: '开启' },
    )
    if (raw == null) return
    const price = normalizeGrouponSubmitPrice(raw)
    if (!price) {
      await showAlert('请输入有效的小刀金额')
      return
    }
    const statusResults = await Promise.all(items.map(async (it: any) => {
      try {
        const status = await itemGrouponStatus(it._uid, it.itemId)
        return { item: it, status, mode: String(status?.data?.drawer?.mode || '').trim() || 'amount' }
      } catch {
        return { item: it, status: null, mode: 'amount' }
      }
    }))
    const amountModeItems = statusResults
      .filter((entry: any) => entry.mode !== 'discount')
      .map((entry: any) => entry.item)
    const invalid = amountModeItems.find((it: any) => !grouponPriceValidation(price, String(it?.price || '')).ok)
    if (invalid) {
      await showAlert(`小刀金额不合法。\n「${invalid.title || invalid.itemId}」当前售价 ¥${invalid.price}`)
      return
    }
    await runSequentialItemBatch({
      title: '批量开启2人小刀',
      items,
      run: (it) => {
        const row = statusResults.find((entry: any) => entry.item === it)
        const selectedMode = row?.mode === 'discount' ? 'discount' : 'amount'
        const selectedOption = selectedMode === 'discount'
          ? (row?.status?.data?.drawer?.options || []).find((option: any) => option?.value === '85' || option?.label === '85折')
          : null
        if (selectedMode === 'discount') {
          return itemGrouponOpen(it._uid, it.itemId, String(selectedOption?.price || selectedOption?.finalPrice || ''), {
            mode: 'discount',
            discount_rate: String(selectedOption?.discountRate || selectedOption?.value || '85'),
          })
        }
        return itemGrouponOpen(it._uid, it.itemId, price, { mode: 'amount' })
      },
      onSuccess: (it) => patchItemLocally(it._uid, it.itemId, { grouponOpen: true, grouponPrice: price }),
      successText: '批量开启小刀完成',
    }).catch((e: any) => showAlert(cleanMtopErr(e?.message || '批量开启小刀失败')))
  }
  async function doBatchGrouponClose() {
    const items = selectedItemsWithPredicate((it) => !!it?.grouponOpen)
    if (!items.length) {
      await showAlert('所选商品没有已开启 2人小刀的商品')
      return
    }
    if (!(await showConfirm(`确认关闭 ${items.length} 个商品的 2人小刀？`, { title: '批量关闭小刀', confirmLabel: '关闭' }))) return
    await runSequentialItemBatch({
      title: '批量关闭2人小刀',
      items,
      run: (it) => itemGrouponClose(it._uid, it.itemId),
      onSuccess: (it) => patchItemLocally(it._uid, it.itemId, { grouponOpen: false, grouponPrice: '' }),
      successText: '批量关闭小刀完成',
    }).catch((e: any) => showAlert(cleanMtopErr(e?.message || '批量关闭小刀失败')))
  }

  async function doShare(uid: string, item: any) {
    try {
      const r: any = await itemShare(uid, item.itemId)
      const link = r?.data?.shareUrl || r?.data?.url || r?.shareUrl || ''
      const text = r?.data?.shareText || r?.data?.text || ''
      if (!link && !text) { await showAlert('未拿到分享链接'); return }
      try { await navigator.clipboard.writeText(text || link) } catch {}
      await showAlert(`分享内容已复制：\n${text || link}`)
    } catch (e: any) { await showAlert(e.message || '获取分享链接失败') }
  }

  function patchItemLocally(uid: string, itemId: string, patch: Record<string, unknown>) {
    setGroups((prev) => prev.map((g) => {
      if (g.uid !== uid) return g
      return {
        ...g,
        items: (g.items || []).map((it: any) =>
          String(it?.itemId) === String(itemId) ? { ...it, ...patch } : it,
        ),
      }
    }))
    setSoldOutItems((prev) => {
      const arr = prev[uid]
      if (!Array.isArray(arr)) return prev
      return {
        ...prev,
        [uid]: arr.map((it: any) =>
          String(it?.itemId) === String(itemId) ? { ...it, ...patch } : it,
        ),
      }
    })
  }

  function removeItemLocally(uid: string, itemId: string) {
    setGroups((prev) => prev.map((g) => {
      if (g.uid !== uid) return g
      return {
        ...g,
        items: (g.items || []).filter((it: any) => String(it?.itemId) !== String(itemId)),
      }
    }))
    setSoldOutItems((prev) => {
      const arr = prev[uid]
      if (!Array.isArray(arr)) return prev
      return {
        ...prev,
        [uid]: arr.filter((it: any) => String(it?.itemId) !== String(itemId)),
      }
    })
  }

  async function doToggleCoin(uid: string, item: any) {
    const isOn = !!item.coinOpen
    let options = coinRatioOptionsFrom(item, item?.detail)
    try {
      const ratioResp = await coinDeductionRatio(uid, String(item.price || '100'), item.itemId)
      options = coinRatioOptionsFrom(ratioResp, item, item?.detail)
    } catch {}
    const canClose = coinCanClose(item)
    const closeOption = {
      value: '__close__',
      label: '关闭抵扣',
      description: canClose ? '关闭闲鱼币抵扣' : `暂时不能关闭，${coinCloseRemainingText(item) || '开启 24 小时后可关闭'}`,
      disabled: !canClose,
    }
    const choiceOptions = isOn ? [...options, closeOption] : options
    const availableOptions = choiceOptions.filter((option) => !option.disabled && option.value !== '__close__')
    if (!isOn && !availableOptions.length) {
      await showAlert('没有从该商品返回数据里拿到可开启的闲鱼币抵扣比例，请先同步商品或稍后再试')
      return
    }
    const r = await showChoice(
      isOn
        ? `请选择「${item.title || item.itemId}」新的闲鱼币抵扣比例`
        : `请选择「${item.title || item.itemId}」可开启的闲鱼币抵扣比例`,
      choiceOptions,
      { title: isOn ? '修改闲鱼币抵扣' : '开启闲鱼币抵扣' },
    )
    if (r == null) return
    if (r === '__close__') {
      try {
        const resp = await itemCoinDeductionClose(uid, item.itemId)
        if (isSuccess(resp)) {
          patchItemLocally(uid, item.itemId, { coinOpen: false, coinRatio: '', coinOpenedAt: 0 })
          await doSync(uid, { silent: true })
          await showAlert('闲鱼币抵扣已关闭')
        } else {
          const msg = retMsg(resp, '关闭失败')
          if (msg.includes('24 小时后') || msg.includes('次日操作关闭') || msg.includes('暂时不能关闭')) {
            patchItemLocally(uid, item.itemId, { coinOpen: true })
          }
          await showAlert(msg)
        }
      } catch (e: any) { await showAlert(e.message || '关闭失败') }
      return
    }
    try {
      const resp = await itemCoinDeductionOpen(uid, item.itemId, r)
      if (!isSuccess(resp)) {
        await showAlert(retMsg(resp, '开启失败'))
        return
      }
      patchItemLocally(uid, item.itemId, {
        coinOpen: true,
        coinRatio: r,
        coinRatioList: options,
        coinOpenedAt: isOn ? (item.coinOpenedAt || 0) : (item.coinOpenedAt || (Date.now() / 1000)),
      })
      await doSync(uid, { silent: true })
      await showAlert(isOn ? `已修改为 ${r}% 闲鱼币抵扣` : `已开启 ${r}% 闲鱼币抵扣`)
    } catch (e: any) { await showAlert(e.message || '开启失败') }
  }

  async function doToggleGroupon(uid: string, item: any) {
    const requestKey = `${uid}:${item?.itemId || ''}`
    if (grouponSettingsBusyRef.current.has(requestKey)) return
    grouponSettingsBusyRef.current.add(requestKey)

    const isOn = !!item?.grouponOpen
    let statusResp: any = null
    let guideResp: any = null
    try {
      statusResp = await itemGrouponStatus(uid, item.itemId).catch(() => null)
      const statusOptions = statusResp?.data?.drawer?.options
      const statusHasFormData = isSuccess(statusResp) && Array.isArray(statusOptions) && statusOptions.length > 0
      if (!statusHasFormData) {
        guideResp = await itemGrouponGuide(uid, item.itemId).catch(() => null)
      }
      const itemPrice = normalizePriceText(statusResp?.data?.price || guideResp?.data?.price || item?.price) || '100'
      const currentGrouponPrice = normalizeGrouponSubmitPrice(item?.grouponPrice)
      const options = buildGrouponOptions(itemPrice, statusResp, guideResp, item, item?.detail)
      const mode = options[0]?.mode === 'discount' ? 'discount' : 'amount'
      const availableOptions = options.filter((o) => !o.disabled)
      if (!availableOptions.length) {
        showAlert(mode === 'discount' ? '当前没有可用的小刀折扣档位' : '当前售价过低，没有可用的小刀金额')
        return
      }
      const fixedValues = options.map((o) => o.value)
      const defaultFixed = normalizePriceText(GROUPON_FIXED_AMOUNTS[1] || GROUPON_FIXED_AMOUNTS[0])
      const useCustom = mode === 'amount' && !!currentGrouponPrice && !fixedValues.includes(currentGrouponPrice) && isOn
      const currentDiscount = options.find((o) => normalizePriceText(o.finalPrice || o.price || o.grouponPrice) === currentGrouponPrice)?.value || ''
      const recommendedOption = availableOptions[0]
      setGrouponSheet({
        uid, item, currentlyOpen: isOn,
        title: '设置小刀优惠', itemPrice, mode,
        selectedPrice: mode === 'discount'
          ? (currentDiscount || recommendedOption?.value || '')
          : (useCustom
          ? '__custom__'
          : (currentGrouponPrice && fixedValues.includes(currentGrouponPrice) ? currentGrouponPrice : defaultFixed || availableOptions[0]?.value || '')),
        recommendedPrice: mode === 'discount'
          ? normalizePriceText(recommendedOption?.finalPrice || '')
          : (defaultFixed || availableOptions[0]?.value || ''),
        selectedDiscount: '', recommendedDiscount: '',
        helperText: isOn
          ? (mode === 'discount'
            ? '当前已开启 2人小刀，可修改折扣档位，或选择"不开"关闭'
            : '当前已开启 2人小刀，可修改优惠金额，或选择"不开"关闭')
          : (mode === 'discount'
            ? '请选择折扣档位，买家下单前将按折扣价参与 2人小刀'
            : '请选择小刀优惠金额，买家下单前可随机获得不高于该金额的优惠'),
        buttonText: statusResp?.data?.drawer?.buttonText || '确定',
        closeOptionLabel: '不开',
        backgroundImageUrl: statusResp?.data?.drawer?.backgroundImageUrl || '',
        descLines: statusResp?.data?.drawer?.descLines || [],
        headerTips: statusResp?.data?.drawer?.headerTips || [],
        freeShipping: !!statusResp?.data?.drawer?.freeShipping,
        historyGroupon: !!statusResp?.data?.drawer?.historyGroupon,
        options,
        customPrice: useCustom ? (currentGrouponPrice ? String(Number(currentGrouponPrice).toFixed(2)) : '') : '',
        inlineError: '',
      })
    } finally {
      grouponSettingsBusyRef.current.delete(requestKey)
    }
  }

  async function submitGrouponSheet() {
    if (!grouponSheet || grouponSheet.loading) return
    const { uid, item, selectedPrice, currentlyOpen, mode } = grouponSheet
    const customPrice = normalizeGrouponSubmitPrice(grouponSheet.customPrice)
    const finalPrice = mode === 'discount'
      ? normalizeGrouponSubmitPrice(grouponSheet.options.find((option: any) => option.value === selectedPrice)?.finalPrice || '')
      : selectedPrice === '__custom__'
      ? customPrice
      : normalizeGrouponSubmitPrice(selectedPrice)
    if (!selectedPrice) { closeGrouponSheet(); return }
    if (selectedPrice !== '__close__') {
      const validation = grouponPriceValidation(finalPrice, grouponSheet.itemPrice || item?.price)
      if (!validation.ok) {
        setGrouponSheet((prev: any) => prev ? { ...prev, inlineError: validation.message } : prev)
        await showAlert(validation.message)
        return
      }
    }
    if (selectedPrice !== '__close__' && !finalPrice) {
      setGrouponSheet((prev: any) => prev ? { ...prev, inlineError: mode === 'discount' ? '请选择有效的折扣档位' : '请输入有效的小刀金额' } : prev)
      await showAlert(mode === 'discount' ? '请选择有效的折扣档位' : '请输入有效的小刀金额')
      return
    }
    setGrouponSheet(null)
    try {
      if (selectedPrice === '__close__') {
        await itemGrouponClose(uid, item.itemId)
        patchItemLocally(uid, item.itemId, { grouponOpen: false, grouponPrice: '' })
        await doSync(uid, { silent: true })
        await showAlert('2人小刀已关闭')
        return
      }
      await itemGrouponOpen(uid, item.itemId, finalPrice, {
        mode,
        discount_rate: mode === 'discount' ? String(selectedPrice || '') : '',
      })
      patchItemLocally(uid, item.itemId, { grouponOpen: true, grouponPrice: finalPrice })
      const label = mode === 'discount'
        ? grouponSheet.options.find((option: any) => option.value === selectedPrice)?.label || `¥${finalPrice}`
        : `¥${finalPrice}`
      await showAlert(`${currentlyOpen ? '已更新' : '已开启'} 2人小刀，${mode === 'discount' ? `当前档位 ${label}` : `最高优惠 ${label}`}`)
      void doSync(uid, { silent: true }).catch(() => {})
    } catch (e: any) {
      await showAlert(e.message || '2人小刀操作失败')
    }
  }

  // 初次只拉账号；商品列表等账号快照进入 state 后再加载，避免
  // loadAccounts().then(loadAll) 读到旧 activeAccounts 后又被 accounts effect 重跑。
  useEffect(() => {
    lastLoadedAccountKeyRef.current = ''
    clearItemsState()
    void loadAccounts()
  }, [effectiveAuthToken])
  useEffect(() => {
    if (!activeAccountKey) {
      clearItemsState()
      lastLoadedAccountKeyRef.current = ''
      return
    }
    const scopedAccountKey = `${effectiveAuthToken}:${activeAccountKey}`
    if (scopedAccountKey === lastLoadedAccountKeyRef.current) return
    lastLoadedAccountKeyRef.current = scopedAccountKey
    void loadAll()
  }, [activeAccountKey, effectiveAuthToken])
  useEffect(() => {
    const unsubscribe = listenBatchedSyncEvent(XIANYU_ITEMS_SYNCED_EVENT, 300, async (uids) => {
      if (uids.length > 0) {
        for (const uid of uids) {
          await doSync(uid, { silent: true }).catch(() => {})
        }
      } else if (activeAccounts.length) {
        await loadAll()
      }
    })
    return unsubscribe
  }, [activeAccounts.length])
  useEffect(() => {
    if (statusFilter !== 'off' || !groups.length) return
    for (const g of groups) {
      if (soldOutItems[g.uid] === undefined) loadSoldOut(g.uid)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [statusFilter, groups.length])

  // 聚合 + 筛选 + 排序
  const allItems = useMemo(() => {
    const byKey = new Map<string, any>()
    const push = (candidate: any) => {
      const key = `${candidate?._uid || ''}:${candidate?.itemId || ''}`
      if (!key || key === ':') return
      const prev = byKey.get(key)
      if (!prev) {
        byKey.set(key, candidate)
        return
      }
      const prevOff = statusOf(prev) === 'off'
      const nextOff = statusOf(candidate) === 'off'
      const prevConfirmedOnSale = String(prev?.status || '').toUpperCase() === 'ON_SALE' && prev?.online === true
      const nextConfirmedOnSale = String(candidate?.status || '').toUpperCase() === 'ON_SALE' && candidate?.online === true
      // 两个上游列表短暂不一致时，确认在售的记录优先于旧的下架缓存。
      if (prevConfirmedOnSale && nextOff) return
      if (nextConfirmedOnSale && prevOff) {
        byKey.set(key, { ...prev, ...candidate, _fromSoldOut: false, _sold: false })
        return
      }
      const prevFromSoldOut = !!prev?._fromSoldOut
      const nextFromSoldOut = !!candidate?._fromSoldOut
      const prevDetailKeys = prev?.detail && typeof prev.detail === 'object' ? Object.keys(prev.detail).length : 0
      const nextDetailKeys = candidate?.detail && typeof candidate.detail === 'object' ? Object.keys(candidate.detail).length : 0
      if (
        (nextOff && !prevOff)
        || (nextFromSoldOut && !prevFromSoldOut)
        || (nextOff === prevOff && nextFromSoldOut === prevFromSoldOut && nextDetailKeys > prevDetailKeys)
      ) {
        byKey.set(key, { ...prev, ...candidate })
      }
    }
    for (const g of groups) {
      const _color = colorForAccount(g.uid)
      const _avatar = g.avatar
      for (const it of g.items) {
        const sold = String(it?.status || '').toUpperCase() === 'SOLD_OUT'
        push({ ...it, _uid: g.uid, _nick: g.nick || g.uid, _color, _avatar, _sold: sold })
      }
      const soldOutList = Array.isArray(soldOutItems[g.uid]) ? soldOutItems[g.uid] : []
      for (const it of soldOutList) {
        const sold = String(it?.status || '').toUpperCase() === 'SOLD_OUT'
        push({
          ...it,
          _uid: g.uid,
          _nick: g.nick || g.uid,
          _color,
          _avatar,
          _sold: sold,
          _fromSoldOut: true,
        })
      }
    }
    return [...byKey.values()]
  }, [groups, soldOutItems])

  const accountScopedItems = useMemo(() => {
    if (!accountScopeUids.length) return allItems
    const uidSet = new Set(accountScopeUids)
    return allItems.filter((item) => uidSet.has(item._uid))
    // accountScopeKey is the stable identity for the selected UID collection.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allItems, accountScopeKey])
  const counts = {
    all: accountScopedItems.length,
    onsale: accountScopedItems.filter(isOnSaleListed).length,
    off: accountScopedItems.filter((i) => statusOf(i) === 'off').length,
  }

  const filtered = useMemo(() => {
    let list = accountScopedItems
    if (statusFilter === 'onsale') list = list.filter(isOnSaleListed)
    else if (statusFilter !== 'all') list = list.filter((i) => statusOf(i) === statusFilter)
    if (searchQuery.trim()) {
      const q = searchQuery.trim().toLowerCase()
      list = list.filter((i) =>
        (i.title || '').toLowerCase().includes(q) ||
        String(i.itemId || '').toLowerCase().includes(q)
      )
    }
    const sorted = [...list]
    if (sortKey === 'published_desc') sorted.sort((a, b) => itemPublishTimeMs(b) - itemPublishTimeMs(a))
    else if (sortKey === 'views') sorted.sort((a, b) => (Number(b.browseCount ?? b.viewCount ?? 0)) - (Number(a.browseCount ?? a.viewCount ?? 0)))
    else if (sortKey === 'likes') sorted.sort((a, b) => (Number(b.wantCount ?? b.favorCount ?? 0)) - (Number(a.wantCount ?? a.favorCount ?? 0)))
    else if (sortKey === 'price_desc') sorted.sort((a, b) => parseFloat(b.price || '0') - parseFloat(a.price || '0'))
    else if (sortKey === 'price_asc') sorted.sort((a, b) => parseFloat(a.price || '0') - parseFloat(b.price || '0'))
    return sorted
  }, [accountScopedItems, statusFilter, searchQuery, sortKey])

  return {
    // accounts
    accounts, activeAccounts,
    // groups / lists
    groups, allItems, accountScopedItems, filtered, counts,
    // flags
    loading, syncing, syncingUids, polishingUids, polishedUids, loadingSoldOut,
    batchRunning, batchProgress,
    // filter
    accountFilter, setAccountFilter,
    desktopAccountFilterUids, setDesktopAccountFilterUids,
    statusFilter, setStatusFilter,
    searchQuery, setSearchQuery,
    sortKey, setSortKey,
    // selection
    selectMode, enterSelectMode, exitSelectMode,
    selectedKeys, toggleSelect, toggleSelectAll, itemKey,
    // modals
    editingItem, setEditingItem,
    publishOpen, setPublishOpen,
    dialog, grouponSheet, setGrouponSheet, closeGrouponSheet,
    // dialog helpers
    notify, showAlert, showConfirm, showPrompt, showChoice,
    // actions
    loadAll, doSync, doSyncAll, doSyncCurrentAccounts, doPolish, doPolishAll,
    doReducePrice, doDownshelf, doUpshelf, doDelete, doShare,
    doToggleCoin, doToggleGroupon, submitGrouponSheet,
    doBatchDelete, doBatchDownshelf, doBatchUpshelf,
    doBatchReducePrice, doBatchCoinOpen, doBatchCoinClose,
    doBatchGrouponOpen, doBatchGrouponClose,
    runBatchPublish, doEditItem,
  }
}
