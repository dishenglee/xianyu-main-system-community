// @ts-nocheck
// 订单页业务状态 hook —— 桌面 / 移动端共享同一份数据与 handler。
// 仅负责：列表加载 / 同步 / 发货上下文 / 改价 / 关闭 / 删除。
// UI 选项（status / search / acctFilter / shipModal 等）由各端各自的 useState 提供，
// 在调用 handler 时通过参数注入；这样上层可以自由决定弹层风格（Desktop modal / Mobile sheet）。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAccounts } from '../../../hooks/useAccounts'
import { limitConcurrency } from '../../../hooks/limitConcurrency'
import { useNotify } from '../../../components/Notify'
import { useAppDialog } from '../../../components/AppDialog'
import { listenBatchedSyncEvent, XIANYU_ORDERS_SYNCED_EVENT } from '../../runtimeEvents'
import {
  tradeSold,
  orderDelete,
  orderClose,
  orderConsign,
  orderConsignDummy,
  orderConsignRender,
  orderAdjustPrice,
  requestRedFlower,
  syncOrders,
  isSuccess,
  retMsg,
} from '../../../api/index'
import { getAuthToken } from '../../../api/client'
import {
  expressList,
  extractOrders,
  colorForAccount,
  getOverdueHours,
  getOrderPrice,
} from '../orderUtils'
import {
  isWaitSendOrder as isWaitSend,
  isRedFlowerEligibleOrder,
  orderStageForOrder as kanbanStatusForOrder,
} from '../orderStatus'
import { useAuthStore } from '../../../store/authStore'
import { filterBusinessAccounts } from '../../../utils/accountAccess'
import { emitActionFailure, emitActionLog, emitActionStarted, emitActionSuccess } from '../../../bootstrap/actionLogging'

export type ShipModalState = { uid: string; order: any } | null

export type ShipForm = {
  companyCode: string
  companyName: string
  mailNo: string
}

const initialShipForm: ShipForm = {
  companyCode: expressList[0].code,
  companyName: expressList[0].name,
  mailNo: '',
}

export function useOrdersPageState() {
  const { accounts, loadAccounts } = useAccounts()
  const notify = useNotify()
  const { showConfirm, showPrompt } = useAppDialog()
  const authToken = useAuthStore((state) => state.token)
  const effectiveAuthToken = authToken || getAuthToken()

  const [loading, setLoading] = useState(false)
  const [groups, setGroups] = useState<any[]>([])
  const [syncingUids, setSyncingUids] = useState<Record<string, boolean>>({})
  const [requestingRedFlowerIds, setRequestingRedFlowerIds] = useState<Record<string, boolean>>({})

  // 发货弹层 / 表单
  const [shipModal, setShipModal] = useState<ShipModalState>(null)
  const [shipForm, setShipForm] = useState<ShipForm>(initialShipForm)
  const [shipMode, setShipMode] = useState<'logistics' | 'dummy'>('logistics')
  const [shipContext, setShipContext] = useState<any>(null)
  const [shipContextLoading, setShipContextLoading] = useState(false)
  const [shipContextError, setShipContextError] = useState('')
  const [copiedAddress, setCopiedAddress] = useState('')
  const [shipping, setShipping] = useState(false)
  const lastLoadedAccountKeyRef = useRef('')

  const activeAccounts = useMemo(
    () => filterBusinessAccounts(accounts),
    [accounts],
  )
  // 用 ref 给 window 事件回调读取最新值，避免重复添加监听
  const activeAccountsRef = useRef(activeAccounts)
  activeAccountsRef.current = activeAccounts
  const activeAccountKey = useMemo(
    () => activeAccounts.map((account: any) => account.uid).join('|'),
    [activeAccounts],
  )
  const activeAccountKeyRef = useRef(activeAccountKey)
  activeAccountKeyRef.current = activeAccountKey

  function clearOrderState() {
    setGroups([])
    setSyncingUids({})
    setRequestingRedFlowerIds({})
    setShipModal(null)
    setShipForm(initialShipForm)
    setShipMode('logistics')
    setShipContext(null)
    setShipContextLoading(false)
    setShipContextError('')
    setCopiedAddress('')
    setShipping(false)
  }

  function orderActionContext(extra: Record<string, unknown> = {}) {
    return {
      activeAccountCount: activeAccountsRef.current.length,
      loadedOrderCount: groups.reduce((sum, group) => sum + Number((group.orders || []).length), 0),
      ...extra,
    }
  }

  const loadAllOrders = useCallback(async (force = false) => {
    const list = activeAccountsRef.current
    if (!list.length) {
      clearOrderState()
      lastLoadedAccountKeyRef.current = ''
      return
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setLoading(true)
    try {
      const results = await limitConcurrency(list, 5, async (account: any) => {
        try {
          const r = await tradeSold(account.uid, 1, 'all', { force })
          const orders = extractOrders(r)
          const lastSyncedAt = Number(r?._last_synced_at || 0)
          return { uid: account.uid, nick: account.nick, avatar: account.avatar, orders, lastSyncedAt }
        } catch {
          return { uid: account.uid, nick: account.nick, avatar: account.avatar, orders: [], lastSyncedAt: 0 }
        }
      })
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      setGroups(results.filter((g) => g.orders.length > 0 || g.lastSyncedAt > 0))
    } finally {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
        setLoading(false)
      }
    }
  }, [])

  const syncOne = useCallback(async (uid: string, opts: { silent?: boolean } = {}) => {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setSyncingUids((prev) => ({ ...prev, [uid]: true }))
    if (!opts?.silent) {
      emitActionStarted({
        module: 'orders',
        action: 'sync_account_orders',
        message: '开始同步单账号订单',
        context: orderActionContext({ uid }),
      })
    }
    try {
      const syncResult = await syncOrders(uid)
      const r = await tradeSold(uid, 1, 'all', { force: false })
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      const orders = extractOrders(r)
      const lastSyncedAt = Number(r?._last_synced_at || 0)
      setGroups((prev) => {
        const exists = prev.some((g) => g.uid === uid)
        if (!exists) {
          const account = activeAccountsRef.current.find((item: any) => item.uid === uid)
          return [...prev, { uid, nick: account?.nick || uid, avatar: account?.avatar, orders, lastSyncedAt }]
        }
        return prev.map((g) => (g.uid === uid ? { ...g, orders, lastSyncedAt } : g))
      })
      if (syncResult?.ok === false) {
        if (!opts?.silent) {
          emitActionLog({
            module: 'orders',
            action: 'sync_account_orders',
            outcome: 'warn',
            message: '单账号订单同步未成功',
            detail: syncResult?.detail || '订单同步失败',
            retryable: true,
            promoteToIncident: false,
            context: orderActionContext({ uid }),
          })
        }
        notify.error(syncResult?.detail || '订单同步失败')
      } else if (!opts?.silent) {
        notify.success('订单同步完成')
        emitActionSuccess({
          module: 'orders',
          action: 'sync_account_orders',
          message: '单账号订单同步成功',
          context: orderActionContext({ uid, orderCount: orders.length }),
        })
      }
    } catch (error) {
      if (!opts?.silent) {
        emitActionFailure({
          module: 'orders',
          action: 'sync_account_orders',
          message: '单账号订单同步失败',
          error,
          context: orderActionContext({ uid }),
        })
      }
      throw error
    } finally {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
        setSyncingUids((prev) => ({ ...prev, [uid]: false }))
      }
    }
  }, [notify])

  const syncAll = useCallback(async () => {
    const list = activeAccountsRef.current
    if (!list.length) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setLoading(true)
    emitActionStarted({
      module: 'orders',
      action: 'sync_all_orders',
      message: '开始同步全部账号订单',
      context: orderActionContext({ accountCount: list.length }),
    })
    try {
      const results = await Promise.allSettled(list.map((a: any) => syncOrders(a.uid)))
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      await loadAllOrders(false)
      if (getAuthToken() !== requestToken || activeAccountKeyRef.current !== requestAccountKey) return
      const failed = results
        .map((entry, index) => ({ entry, account: list[index] }))
        .filter(({ entry }) => entry.status === 'rejected' || (entry.status === 'fulfilled' && entry.value?.ok === false))
      if (failed.length) {
        const first = failed[0]
        const reason = first.entry.status === 'rejected'
          ? String(first.entry.reason?.message || first.entry.reason || '同步失败')
          : String(first.entry.value?.detail || '同步失败')
        notify.error(`部分账号订单同步失败：${first.account?.nick || first.account?.uid || ''}，${reason}`)
      } else {
        notify.success(`已同步 ${list.length} 个账号订单`)
      }
      emitActionLog({
        module: 'orders',
        action: 'sync_all_orders',
        outcome: failed.length ? 'warn' : 'success',
        message: failed.length ? '全部账号订单同步部分失败' : '全部账号订单同步成功',
        retryable: failed.length > 0,
        promoteToIncident: false,
        context: orderActionContext({
          accountCount: list.length,
          failureCount: failed.length,
        }),
      })
    } catch (error) {
      emitActionFailure({
        module: 'orders',
        action: 'sync_all_orders',
        message: '全部账号订单同步失败',
        error,
        context: orderActionContext({ accountCount: list.length }),
      })
      throw error
    } finally {
      if (getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) {
        setLoading(false)
      }
    }
  }, [notify, loadAllOrders])

  const removeOrderLocally = useCallback((uid: string, orderId: string) => {
    setGroups((prev) => prev.map((g) => {
      if (g.uid !== uid) return g
      return {
        ...g,
        orders: (g.orders || []).filter((order: any) =>
          String(order?.bizOrderId || order?.orderId) !== String(orderId),
        ),
      }
    }))
  }, [])

  const handleDelete = useCallback(async (uid: string, order: any) => {
    const confirmed = await showConfirm('确认删除订单？', { danger: true, confirmLabel: '删除' })
    if (!confirmed) return
    const orderId = order.bizOrderId || order.orderId
    emitActionStarted({
      module: 'orders',
      action: 'delete_order',
      message: '开始删除订单',
      context: orderActionContext({ uid, orderId }),
    })
    try {
      await orderDelete(uid, orderId)
      removeOrderLocally(uid, orderId)
      void syncOne(uid, { silent: true }).catch(() => {})
      notify.success('已删除订单')
      emitActionSuccess({
        module: 'orders',
        action: 'delete_order',
        message: '订单删除成功',
        context: orderActionContext({ uid, orderId }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'orders',
        action: 'delete_order',
        message: '订单删除失败',
        error: e,
        context: orderActionContext({ uid, orderId }),
      })
      notify.error(e?.message || '删除失败')
    }
  }, [notify, removeOrderLocally, showConfirm, syncOne])

  const handleClose = useCallback(async (uid: string, order: any) => {
    const confirmed = await showConfirm('确认关闭订单？关闭后无法恢复。', { confirmLabel: '关闭', danger: true })
    if (!confirmed) return
    const orderId = order.bizOrderId || order.orderId
    // Xianyu close API requires a close reason; empty reason is a common silent failure.
    const closeReason = '不想卖了'
    emitActionStarted({
      module: 'orders',
      action: 'close_order',
      message: '开始关闭订单',
      context: orderActionContext({ uid, orderId, closeReason }),
    })
    try {
      const result = await orderClose(uid, orderId, closeReason)
      if (!isSuccess(result)) throw new Error(retMsg(result, '关闭订单失败'))
      await syncOne(uid)
      notify.success('已关闭订单')
      emitActionSuccess({
        module: 'orders',
        action: 'close_order',
        message: '订单关闭成功',
        context: orderActionContext({ uid, orderId, closeReason }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'orders',
        action: 'close_order',
        message: '订单关闭失败',
        error: e,
        context: orderActionContext({ uid, orderId, closeReason }),
      })
      notify.error(e?.message || '关闭失败')
    }
  }, [notify, showConfirm, syncOne])

  const handleAdjustPrice = useCallback(async (uid: string, order: any) => {
    const orderId = order.bizOrderId || order.orderId
    const currentPrice = getOrderPrice(order)
    const nextPrice = await showPrompt('输入改价后的实付金额', {
      title: '订单改价',
      defaultValue: currentPrice ? String(currentPrice) : '',
      placeholder: '例如 12.34',
      confirmLabel: '确认改价',
    })
    if (nextPrice == null) return
    const price = Number(String(nextPrice).trim())
    if (!Number.isFinite(price) || price < 0) {
      emitActionLog({
        module: 'orders',
        action: 'adjust_order_price',
        outcome: 'warn',
        message: '订单改价金额无效',
        retryable: false,
        promoteToIncident: false,
        context: orderActionContext({ uid, orderId }),
      })
      notify.error('请输入有效金额')
      return
    }
    emitActionStarted({
      module: 'orders',
      action: 'adjust_order_price',
      message: '开始订单改价',
      context: orderActionContext({ uid, orderId, price }),
    })
    try {
      const result = await orderAdjustPrice(uid, orderId, price)
      if (!isSuccess(result)) throw new Error(retMsg(result, '改价失败'))
      await syncOne(uid)
      notify.success('改价成功')
      emitActionSuccess({
        module: 'orders',
        action: 'adjust_order_price',
        message: '订单改价成功',
        context: orderActionContext({ uid, orderId, price }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'orders',
        action: 'adjust_order_price',
        message: '订单改价失败',
        error: e,
        context: orderActionContext({ uid, orderId, price }),
      })
      notify.error(e?.message || '改价失败')
    }
  }, [notify, showPrompt, syncOne])

  const handleRequestRedFlower = useCallback(async (uid: string, order: any) => {
    if (!isRedFlowerEligibleOrder(order)) {
      notify.error('仅已完成且无退款订单可求小红花')
      return
    }
    const orderId = String(order?.bizOrderId || order?.orderId || '').trim()
    if (!orderId) {
      notify.error('未找到订单号')
      return
    }
    const confirmed = await showConfirm('将向买家发起求小红花请求，是否继续？', {
      title: '求小红花',
      confirmLabel: '确认请求',
    })
    if (!confirmed) return

    const requestKey = `${uid}:${orderId}`
    setRequestingRedFlowerIds((prev) => ({ ...prev, [requestKey]: true }))
    try {
      const result = await requestRedFlower(uid, orderId)
      if (result?.success === false || (result?.ret && !isSuccess(result))) {
        throw new Error(retMsg(result, '求小红花失败'))
      }
      notify.success('已发起求小红花请求')
    } catch (e: any) {
      notify.error(e?.message || '求小红花失败')
    } finally {
      setRequestingRedFlowerIds((prev) => ({ ...prev, [requestKey]: false }))
    }
  }, [notify, showConfirm])

  const handleConsignDummy = useCallback(async (uid: string, order: any) => {
    const confirmed = await showConfirm('确认无需物流发货？', { confirmLabel: '确认发货' })
    if (!confirmed) return
    setShipping(true)
    const orderId = order.bizOrderId || order.orderId
    emitActionStarted({
      module: 'orders',
      action: 'consign_order_dummy',
      message: '开始无需物流发货',
      context: orderActionContext({ uid, orderId }),
    })
    try {
      const result = await orderConsignDummy(uid, orderId)
      if (!isSuccess(result)) throw new Error(retMsg(result, '无需物流发货失败'))
      await syncOne(uid)
      setShipModal(null)
      notify.success('无需物流发货成功')
      emitActionSuccess({
        module: 'orders',
        action: 'consign_order_dummy',
        message: '无需物流发货成功',
        context: orderActionContext({ uid, orderId }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'orders',
        action: 'consign_order_dummy',
        message: '无需物流发货失败',
        error: e,
        context: orderActionContext({ uid, orderId }),
      })
      notify.error(e?.message || '无需物流发货失败')
    } finally {
      setShipping(false)
    }
  }, [notify, showConfirm, syncOne])

  const handleConsign = useCallback(async () => {
    if (!shipModal) return
    if (!shipForm.mailNo.trim()) {
      emitActionLog({
        module: 'orders',
        action: 'consign_order',
        outcome: 'warn',
        message: '订单发货缺少物流单号',
        retryable: false,
        promoteToIncident: false,
        context: orderActionContext({
          uid: shipModal.uid,
          orderId: shipModal.order.bizOrderId || shipModal.order.orderId,
          companyCode: shipForm.companyCode,
        }),
      })
      notify.error('请输入物流单号')
      return
    }
    setShipping(true)
    const orderId = shipModal.order.bizOrderId || shipModal.order.orderId
    emitActionStarted({
      module: 'orders',
      action: 'consign_order',
      message: '开始订单物流发货',
      context: orderActionContext({
        uid: shipModal.uid,
        orderId,
        companyCode: shipForm.companyCode,
        hasSenderAddress: !!(shipContext?.data?.senderAddressInfo?.addressId || shipContext?.senderAddressInfo?.addressId),
      }),
    })
    try {
      const senderAddressId = shipContext?.data?.senderAddressInfo?.addressId || shipContext?.senderAddressInfo?.addressId || ''
      const result = await orderConsign(
        shipModal.uid,
        orderId,
        shipForm.companyCode,
        shipForm.companyName,
        shipForm.mailNo.trim(),
        String(senderAddressId || ''),
      )
      if (!isSuccess(result)) throw new Error(retMsg(result, '发货失败'))
      await syncOne(shipModal.uid)
      setShipModal(null)
      notify.success('发货成功')
      emitActionSuccess({
        module: 'orders',
        action: 'consign_order',
        message: '订单物流发货成功',
        context: orderActionContext({
          uid: shipModal.uid,
          orderId,
          companyCode: shipForm.companyCode,
        }),
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'orders',
        action: 'consign_order',
        message: '订单物流发货失败',
        error: e,
        context: orderActionContext({
          uid: shipModal.uid,
          orderId,
          companyCode: shipForm.companyCode,
        }),
      })
      notify.error(e?.message || '发货失败')
    } finally {
      setShipping(false)
    }
  }, [notify, shipModal, shipForm, shipContext, syncOne])

  const handleShipmentSubmit = useCallback(async () => {
    if (!shipModal) return
    if (shipMode === 'dummy') {
      await handleConsignDummy(shipModal.uid, shipModal.order)
      return
    }
    await handleConsign()
  }, [shipModal, shipMode, handleConsignDummy, handleConsign])

  const copyAddress = useCallback(async (text: string, key: string) => {
    if (!text) return
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text)
      } else {
        const textarea = document.createElement('textarea')
        textarea.value = text
        textarea.setAttribute('readonly', 'readonly')
        textarea.style.position = 'fixed'
        textarea.style.opacity = '0'
        document.body.appendChild(textarea)
        textarea.select()
        document.execCommand('copy')
        document.body.removeChild(textarea)
      }
      setCopiedAddress(key)
      setTimeout(() => setCopiedAddress(''), 1400)
    } catch {
      notify.error('复制失败')
    }
  }, [notify])

  // 初始只拉账号；订单列表等账号快照进入 state 后再加载，避免重复全账号请求。
  useEffect(() => {
    lastLoadedAccountKeyRef.current = ''
    clearOrderState()
    void loadAccounts()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effectiveAuthToken])

  // 账号列表变更后重新加载，同一批账号只加载一次。
  useEffect(() => {
    if (!activeAccountKey) {
      clearOrderState()
      lastLoadedAccountKeyRef.current = ''
      return
    }
    const scopedAccountKey = `${effectiveAuthToken}:${activeAccountKey}`
    if (scopedAccountKey === lastLoadedAccountKeyRef.current) return
    lastLoadedAccountKeyRef.current = scopedAccountKey
    void loadAllOrders()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeAccountKey, effectiveAuthToken])

  // 监听 GlobalMessageNotify 转发的 xianyu:orders-synced；500ms 去抖批量刷新。
  useEffect(() => {
    const unsubscribe = listenBatchedSyncEvent(XIANYU_ORDERS_SYNCED_EVENT, 500, () => {
      if (activeAccountsRef.current.length) loadAllOrders(true)
    })
    return unsubscribe
  }, [loadAllOrders])

  // 打开发货弹层时拉取发货上下文（地址、合规校验）
  useEffect(() => {
    if (!shipModal) return
    let cancelled = false
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setShipForm({ ...initialShipForm })
    setShipMode('logistics')
    setCopiedAddress('')
    setShipContext(null)
    setShipContextError('')
    setShipContextLoading(true)
    const orderId = shipModal.order?.bizOrderId || shipModal.order?.orderId
    orderConsignRender(shipModal.uid, orderId)
      .then((result) => {
        if (!cancelled && getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) setShipContext(result)
      })
      .catch((error: any) => {
        if (!cancelled && getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) setShipContextError(error?.message || '发货信息加载失败')
      })
      .finally(() => {
        if (!cancelled && getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey) setShipContextLoading(false)
      })
    return () => { cancelled = true }
  }, [shipModal])

  // 派生指标
  const allOrders = useMemo(
    () => groups.flatMap((g) => g.orders.map((o: any) => ({ ...o, _uid: g.uid, _nick: g.nick, _avatar: g.avatar }))),
    [groups],
  )
  const accountStats = useMemo(
    () => activeAccounts.map((account: any) => {
      const orders = allOrders.filter((order) => order._uid === account.uid)
      const waitSendCount = orders.filter(isWaitSend).length
      const overdueCount = orders.filter((order) => getOverdueHours(order) > 0).length
      return {
        uid: account.uid,
        nick: account.nick || account.uid,
        avatar: account.avatar,
        color: colorForAccount(account.uid),
        total: orders.length,
        waitSendCount,
        overdueCount,
        lastSyncedAt: groups.find((group) => group.uid === account.uid)?.lastSyncedAt || 0,
      }
    }),
    [activeAccounts, allOrders, groups],
  )
  const totalWaitSend = useMemo(() => allOrders.filter(isWaitSend).length, [allOrders])
  const totalOverdue = useMemo(() => allOrders.filter((o) => getOverdueHours(o) > 0).length, [allOrders])

  const shipData = shipContext?.data || shipContext || {}
  const senderAddress = shipData?.senderAddressInfo
  const receiverAddress = shipData?.receiverAddressInfo
  const logisticsDisabled = shipMode === 'logistics' && (
    shipContextLoading
    || !senderAddress
    || !receiverAddress
    || !String(senderAddress?.addressId || '').trim()
  )

  // 给某组订单按 kanban 状态分桶（供 KPI / tab 显示）
  function bucketByStage(list: any[]) {
    return {
      pending: list.filter((o) => kanbanStatusForOrder(o) === 'pending'),
      tosend: list.filter((o) => kanbanStatusForOrder(o) === 'tosend'),
      sent: list.filter((o) => kanbanStatusForOrder(o) === 'sent'),
      aftersale: list.filter((o) => kanbanStatusForOrder(o) === 'aftersale'),
      done: list.filter((o) => kanbanStatusForOrder(o) === 'done'),
      closed: list.filter((o) => kanbanStatusForOrder(o) === 'closed'),
    }
  }

  return {
    // accounts + state
    accounts,
    activeAccounts,
    loading,
    groups,
    syncingUids,
    requestingRedFlowerIds,
    allOrders,
    accountStats,
    totalWaitSend,
    totalOverdue,
    // ship modal
    shipModal,
    setShipModal,
    shipForm,
    setShipForm,
    shipMode,
    setShipMode,
    shipContext,
    shipContextLoading,
    shipContextError,
    senderAddress,
    receiverAddress,
    copiedAddress,
    shipping,
    logisticsDisabled,
    // actions
    loadAllOrders,
    syncOne,
    syncAll,
    handleDelete,
    handleClose,
    handleAdjustPrice,
    handleRequestRedFlower,
    handleConsignDummy,
    handleConsign,
    handleShipmentSubmit,
    copyAddress,
    bucketByStage,
  }
}
