// @ts-nocheck
import { useEffect, useMemo, useState } from 'react'
import { CalendarClock, ChevronDown, ChevronUp, Clock3, Play, Power, Save, Settings2, Trash2 } from 'lucide-react'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { useAccounts } from '../../hooks/useAccounts'
import { useDevice } from '../../hooks/useDevice'
import { useNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import {
  deleteAutomationSchedule,
  listAutomationSchedules,
  runAutomationScheduleOnce,
  saveAutomationSchedule,
} from '../../api/otherAutomation'
import { getItems } from '../../api/market'
import {
  getExposureAmountOptions,
  getExposureItemRendering,
  getPromotionPage,
  getPromotionView,
  unwrapMtopData,
} from '../../api/shopGrowth'
import {
  asRecord,
  getExposureAmountList,
  getPromotionPackages,
  getPromotionRecordList,
  pickNumber,
  pickString,
} from '../promotions/promotionUtils'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import {
  emitActionFailure,
  emitActionLog,
  emitActionStarted,
  emitActionSuccess,
} from '../../bootstrap/actionLogging'

const ACTIONS = [
  { value: 'batch_reduce_price', label: '定时所有商品批量降价', needsItems: true, needsAmount: true },
  { value: 'batch_restore_price', label: '定时所有商品批量涨价', needsItems: true, needsAmount: true },
  { value: 'batch_polish', label: '定时一键擦亮所有账号', needsItems: true },
  { value: 'coin_signin', label: '定时闲鱼币签到' },
  { value: 'coin_tasks', label: '定时完成闲鱼币任务' },
  { value: 'exposure_tasks', label: '定时曝光任务' },
  { value: 'coin_promotion', label: '定时抢闲鱼币推广', needsSingleItem: true, needsCoinCost: true },
  { value: 'exposure_promotion', label: '定时曝光推广', needsSingleItem: true, needsExposure: true },
]

const emptyForm = {
  id: '',
  name: '定时所有商品批量降价',
  action: 'batch_reduce_price',
  enabled: true,
  timeOfDay: '09:00',
  accountScope: 'all',
  accountUids: [],
  itemScope: 'all',
  itemIdsText: '',
  amount: '1',
  itemId: '',
  costCoinNum: '',
  promotionNum: '',
  exposureNum: '100',
  accountPromotions: {},
}

function scheduleLogContext(payload) {
  return {
    scheduleId: payload?.id || '',
    name: payload?.name || '',
    action: payload?.action || '',
    enabled: Boolean(payload?.enabled),
    timeOfDay: payload?.timeOfDay || '',
    accountScope: payload?.accountScope || '',
    accountCount: Array.isArray(payload?.accountUids) ? payload.accountUids.length : 0,
    itemScope: payload?.itemScope || '',
    itemCount: Array.isArray(payload?.itemIds) ? payload.itemIds.length : 0,
    configKeys: Object.keys(payload?.config || {}).sort(),
  }
}

const DAILY_FREQUENCY = 'daily'

function formatTime(ts) {
  const n = Number(ts)
  if (!Number.isFinite(n) || n <= 0) return '未排程'
  return new Date(n * 1000).toLocaleString('zh-CN', { hour12: false })
}

function formatCreatedTime(ts) {
  const n = Number(ts)
  if (!Number.isFinite(n) || n <= 0) return '未知'
  return new Date(n * 1000).toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })
}

function shortTaskId(id) {
  const value = String(id || '').trim()
  if (!value) return '—'
  if (value.length <= 18) return value
  return `${value.slice(0, 8)}...${value.slice(-6)}`
}

function accountScopeText(item) {
  if (item.accountScope === 'selected') return `${(item.accountUids || []).length} 个账号`
  return '所有启用账号'
}

function itemScopeText(item) {
  if (item.itemScope === 'selected') return `${(item.itemIds || []).length} 个商品`
  return '所有在售商品'
}

function runStatusLabel(status) {
  if (status === 'success') return '成功'
  if (status === 'failed') return '失败'
  if (status === 'running') return '执行中'
  return '未执行'
}

function runStatusTone(status) {
  if (status === 'success') return { background: 'var(--status-success-bg)', color: 'var(--status-success-text)' }
  if (status === 'failed') return { background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }
  if (status === 'running') return { background: 'var(--primary-light)', color: 'var(--primary)' }
  return { background: 'var(--status-neutral-bg)', color: 'var(--text-secondary)' }
}

function actionMeta(action) {
  return ACTIONS.find((item) => item.value === action) || ACTIONS[0]
}

function scheduleToForm(item) {
  const config = item.config || {}
  return {
    ...emptyForm,
    id: item.id,
    name: item.name,
    action: item.action,
    enabled: true,
    timeOfDay: item.timeOfDay || '09:00',
    accountScope: item.accountScope || 'all',
    accountUids: item.accountUids || [],
    itemScope: item.itemScope || 'all',
    itemIdsText: (item.itemIds || []).join('\n'),
    amount: String(config.amount ?? '1'),
    itemId: String(config.itemId ?? ''),
    costCoinNum: String(config.costCoinNum ?? ''),
    promotionNum: String(config.promotionNum ?? ''),
    exposureNum: String(config.exposureNum ?? '100'),
    accountPromotions: config.accountPromotions || {},
  }
}

function itemIdOf(item) {
  const record = asRecord(item)
  return String(record.item_id ?? record.itemId ?? record.id ?? '').trim()
}

function itemTitleOf(item) {
  const record = asRecord(item)
  return pickString(record, ['title', 'itemTitle', 'name', 'desc'], itemIdOf(item) || '未命名商品')
}

function itemPriceOf(item) {
  const record = asRecord(item)
  return pickString(record, ['price', 'priceText', 'soldPrice'], '')
}

function itemImageOf(item) {
  const record = asRecord(item)
  return pickString(record, ['picUrl', 'pic_url', 'imageUrl', 'imageUrl', 'coverUrl', 'cover'], '')
}

function promotionItemIdOf(record) {
  const itemVO = asRecord(record.itemVO)
  const itemDTO = asRecord(record.itemDTO)
  const detail = asRecord(record.detail)
  return pickString(record, ['itemId', 'item_id'], '')
    || pickString(itemVO, ['itemId', 'item_id'], '')
    || pickString(itemDTO, ['itemId', 'item_id'], '')
    || pickString(detail, ['itemId', 'item_id'], '')
}

function packageLabel(pkg) {
  const promotionNum = pickString(pkg, ['promotionNum', 'exposureNum', 'maxPageView'], '')
  const cost = pickNumber(pkg, ['costCoinNum', 'cost', 'coinNum', 'nowPrice'], '')
  const exposure = pickNumber(pkg, ['exposureNum', 'maxPageView'], '')
  const parts = []
  if (promotionNum) parts.push(`${promotionNum}档`)
  if (cost) parts.push(`${cost}币`)
  if (exposure) parts.push(`${exposure}曝光`)
  return parts.join(' · ') || '推广套餐'
}

function extractAutomationItems(payload) {
  const result = asRecord(payload)
  const data = Array.isArray(result.data) ? result.data : asRecord(result.data)
  const candidates = [
    result.items,
    Array.isArray(data) ? data : data.itemList,
    Array.isArray(data) ? [] : data.items,
    Array.isArray(data) ? [] : data.list,
  ]
  return candidates.find(Array.isArray) || []
}

function shouldForceSyncItems(payload, items, force) {
  if (force || items.length > 0) return false
  const result = asRecord(payload)
  const lastSyncedAt = Number(result._last_synced_at ?? 0)
  return lastSyncedAt <= 0 || result._last_sync_ok === false
}

export default function OtherAutomationPage() {
  const { accounts, loadAccounts } = useAccounts()
  const { isMobile } = useDevice()
  const notify = useNotify()
  const { showConfirm } = useAppDialog()
  const [items, setItems] = useState([])
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [deletingId, setDeletingId] = useState('')
  const [togglingId, setTogglingId] = useState('')
  const [expandedResults, setExpandedResults] = useState({})
  const [form, setForm] = useState(emptyForm)
  const [promotionLoading, setPromotionLoading] = useState(false)
  const [promotionItemsByUid, setPromotionItemsByUid] = useState({})
  const [coinPromotedItemIdsByUid, setCoinPromotedItemIdsByUid] = useState({})
  const [exposureBlockedItemIdsByUid, setExposureBlockedItemIdsByUid] = useState({})
  const [coinPackagesByKey, setCoinPackagesByKey] = useState({})
  const [exposureAmountsByUid, setExposureAmountsByUid] = useState({})

  const activeAccounts = useMemo(() => filterBusinessAccounts(accounts), [accounts])
  const selectedAction = actionMeta(form.action)
  const enabledCount = useMemo(() => items.filter((item) => item.enabled).length, [items])
  const failedCount = useMemo(() => items.filter((item) => item.lastStatus === 'failed').length, [items])
  const scheduleAccounts = useMemo(() => {
    if (form.accountScope !== 'selected') return activeAccounts
    const selected = new Set(form.accountUids)
    return activeAccounts.filter((account) => selected.has(account.uid))
  }, [activeAccounts, form.accountScope, form.accountUids])

  async function loadSchedules() {
    setLoading(true)
    try {
      const payload = await listAutomationSchedules()
      setItems(payload.items || [])
    } catch (error) {
      notify.error(error?.message || '加载自动化失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadAccounts()
    loadSchedules()
  }, [])

  useEffect(() => {
    if (selectedAction.value !== 'coin_promotion' && selectedAction.value !== 'exposure_promotion') return
    if (!scheduleAccounts.length) return
    void loadPromotionConfigData(scheduleAccounts)
  }, [selectedAction.value, scheduleAccounts.map((account) => account.uid).join('|')])

  function patch(next) {
    setForm((current) => ({ ...current, ...next }))
  }

  function toggleResult(id) {
    setExpandedResults((current) => ({ ...current, [id]: !current[id] }))
  }

  function handleActionChange(action) {
    patch({ action, name: actionMeta(action).label })
  }

  function patchAccountPromotion(uid, next) {
    setForm((current) => ({
      ...current,
      accountPromotions: {
        ...(current.accountPromotions || {}),
        [uid]: {
          ...((current.accountPromotions || {})[uid] || {}),
          ...next,
        },
      },
    }))
  }

  async function loadPromotionConfigData(accountsToLoad = scheduleAccounts) {
    if (!accountsToLoad.length) return
    setPromotionLoading(true)
    try {
      const nextItems = {}
      const nextPromoted = {}
      const nextExposureBlocked = {}
      const nextPackages = {}
      const nextExposureAmounts = {}
      for (const account of accountsToLoad) {
        const uid = account.uid
        let itemsPayload = await getItems(uid, 1, 50, 'ON_SALE', { force: false })
        let rawItems = extractAutomationItems(itemsPayload)
        if (shouldForceSyncItems(itemsPayload, rawItems, false)) {
          itemsPayload = await getItems(uid, 1, 50, 'ON_SALE', { force: true })
          rawItems = extractAutomationItems(itemsPayload)
        }
        const validItems = rawItems.filter((item) => itemIdOf(item))
        nextItems[uid] = validItems

        if (selectedAction.value === 'coin_promotion') {
          try {
            const page = await getPromotionPage(uid, 1, 50, { timeoutMs: 8000, timeoutMessage: '已购推广加载超时' })
            nextPromoted[uid] = getPromotionRecordList(page).map((record) => promotionItemIdOf(record)).filter(Boolean)
          } catch {
            nextPromoted[uid] = []
          }
          for (const item of validItems.slice(0, 12)) {
            const itemId = itemIdOf(item)
            if (!itemId || nextPromoted[uid].includes(itemId)) continue
            try {
              const view = await getPromotionView(uid, itemId, { timeoutMs: 8000, timeoutMessage: '推广套餐加载超时' })
              nextPackages[`${uid}:${itemId}`] = getPromotionPackages(asRecord(unwrapMtopData(view)))
            } catch {
              nextPackages[`${uid}:${itemId}`] = []
            }
          }
        }

        if (selectedAction.value === 'exposure_promotion') {
          try {
            const options = await getExposureAmountOptions(uid, { timeoutMs: 8000, timeoutMessage: '曝光套餐加载超时' })
            nextExposureAmounts[uid] = getExposureAmountList(options)
          } catch {
            nextExposureAmounts[uid] = []
          }
          const ids = validItems.map((item) => itemIdOf(item)).filter(Boolean)
          try {
            const rendering = ids.length ? await getExposureItemRendering(uid, ids, { timeoutMs: 8000, timeoutMessage: '曝光状态加载超时' }) : null
            const data = asRecord(unwrapMtopData(rendering))
            const instances = []
            const walk = (node) => {
              if (Array.isArray(node)) {
                node.forEach(walk)
                return
              }
              if (!node || typeof node !== 'object') return
              if (Array.isArray(node.itemActivityInstanceVOList)) instances.push(...node.itemActivityInstanceVOList)
              Object.values(node).forEach(walk)
            }
            walk(data)
            nextExposureBlocked[uid] = instances
              .filter((record) => record?.isPolishing === true || record?.isPolishing === 1 || String(record?.isPolishing) === 'true')
              .map((record) => pickString(asRecord(record), ['itemId', 'item_id'], ''))
              .filter(Boolean)
          } catch {
            nextExposureBlocked[uid] = []
          }
        }
      }
      setPromotionItemsByUid(nextItems)
      setCoinPromotedItemIdsByUid(nextPromoted)
      setExposureBlockedItemIdsByUid(nextExposureBlocked)
      setCoinPackagesByKey(nextPackages)
      setExposureAmountsByUid(nextExposureAmounts)
    } catch (error) {
      notify.error(error?.message || '推广配置加载失败')
    } finally {
      setPromotionLoading(false)
    }
  }

  function selectedUids() {
    if (form.accountScope !== 'selected') return []
    const activeUidSet = new Set(activeAccounts.map((account) => account.uid))
    return form.accountUids.filter((uid) => activeUidSet.has(uid))
  }

  function buildConfig() {
    if (form.action === 'batch_reduce_price' || form.action === 'batch_restore_price') return { amount: Number(form.amount) || 0 }
    if (form.action === 'coin_promotion') {
      return {
        frequency: DAILY_FREQUENCY,
        accountPromotions: form.accountPromotions || {},
        itemId: form.itemId.trim(),
        costCoinNum: Number(form.costCoinNum) || 0,
        expectedCostCoinNum: Number(form.costCoinNum) || 0,
        ...(form.promotionNum.trim() ? { promotionNum: form.promotionNum.trim() } : {}),
      }
    }
    if (form.action === 'exposure_promotion') {
      return {
        frequency: DAILY_FREQUENCY,
        accountPromotions: form.accountPromotions || {},
        itemId: form.itemId.trim(),
        itemIds: form.itemId.trim() ? [form.itemId.trim()] : [],
        exposureNum: Number(form.exposureNum) || 100,
        expectedExposureNum: Number(form.exposureNum) || 100,
      }
    }
    return {}
  }

  async function submit() {
    const taskName = actionMeta(form.action).label
    if (selectedAction.value === 'coin_promotion' || selectedAction.value === 'exposure_promotion') {
      const missing = scheduleAccounts.find((account) => !((form.accountPromotions || {})[account.uid]?.itemId))
      if (missing) {
        emitActionLog({
          module: 'other-automation',
          action: 'save_schedule_validation',
          outcome: 'warn',
          message: '保存定时任务校验失败：账号未选择商品',
          context: { action: selectedAction.value, missingUid: missing.uid },
          promoteToIncident: false,
        })
        return notify.error(`请为账号 ${missing.nick || missing.uid} 选择一个商品`)
      }
    } else if (selectedAction.needsSingleItem && !form.itemId.trim()) {
      emitActionLog({
        module: 'other-automation',
        action: 'save_schedule_validation',
        outcome: 'warn',
        message: '保存定时任务校验失败：缺少商品 ID',
        context: { action: selectedAction.value },
        promoteToIncident: false,
      })
      return notify.error('请填写商品 ID')
    }
    if (selectedAction.needsCoinCost && selectedAction.value !== 'coin_promotion' && !Number(form.costCoinNum)) {
      emitActionLog({
        module: 'other-automation',
        action: 'save_schedule_validation',
        outcome: 'warn',
        message: '保存定时任务校验失败：缺少闲鱼币消耗',
        context: { action: selectedAction.value },
        promoteToIncident: false,
      })
      return notify.error('请填写闲鱼币消耗')
    }
    if (selectedAction.needsAmount && !Number(form.amount)) {
      emitActionLog({
        module: 'other-automation',
        action: 'save_schedule_validation',
        outcome: 'warn',
        message: '保存定时任务校验失败：缺少调价金额',
        context: { action: selectedAction.value },
        promoteToIncident: false,
      })
      return notify.error(selectedAction.value === 'batch_restore_price' ? '请填写涨价金额' : '请填写降价金额')
    }
    setSaving(true)
    const payload = {
      id: form.id,
      name: taskName,
      action: form.action,
      enabled: true,
      timeOfDay: form.timeOfDay,
      timezone: 'Asia/Shanghai',
      accountScope: form.accountScope,
      accountUids: selectedUids(),
      itemScope: selectedAction.needsSingleItem ? 'selected' : form.itemScope,
      itemIds: selectedAction.needsSingleItem
        ? [form.itemId.trim()]
        : form.itemIdsText.split(/\n|,/).map((value) => value.trim()).filter(Boolean),
      config: buildConfig(),
    }
    emitActionStarted({
      module: 'other-automation',
      action: 'save_schedule',
      message: '开始保存定时任务',
      context: scheduleLogContext(payload),
    })
    try {
      const response = await saveAutomationSchedule(payload)
      if (response.item) {
        setItems((current) => {
          const withoutSaved = current.filter((item) => item.id !== response.item.id)
          return [response.item, ...withoutSaved]
        })
      }
      emitActionSuccess({
        module: 'other-automation',
        action: 'save_schedule',
        message: '定时任务保存成功',
        context: {
          ...scheduleLogContext(payload),
          savedScheduleId: response.item?.id || '',
        },
      })
      notify.success(`${response.item?.name || taskName} 保存成功`)
      setForm(emptyForm)
      void loadSchedules()
    } catch (error) {
      emitActionFailure({
        module: 'other-automation',
        action: 'save_schedule',
        message: '定时任务保存失败',
        error,
        context: scheduleLogContext(payload),
      })
      notify.error(error?.message || '保存失败')
    } finally {
      setSaving(false)
    }
  }

  async function remove(id) {
    const target = items.find((item) => item.id === id)
    const name = target?.name || '自动化'
    const confirmed = await showConfirm(`确认删除「${name}」吗？删除后不会再自动执行。`, {
      title: '删除自动化',
      confirmLabel: '删除',
      cancelLabel: '取消',
      danger: true,
    })
    if (!confirmed) return
    setDeletingId(id)
    emitActionStarted({
      module: 'other-automation',
      action: 'delete_schedule',
      message: '开始删除定时任务',
      context: scheduleLogContext(target || { id, name }),
    })
    try {
      await deleteAutomationSchedule(id)
      setItems((current) => current.filter((item) => item.id !== id))
      emitActionSuccess({
        module: 'other-automation',
        action: 'delete_schedule',
        message: '定时任务删除成功',
        context: scheduleLogContext(target || { id, name }),
      })
      notify.success(`${name} 删除成功`)
      void loadSchedules()
    } catch (error) {
      emitActionFailure({
        module: 'other-automation',
        action: 'delete_schedule',
        message: '定时任务删除失败',
        error,
        context: scheduleLogContext(target || { id, name }),
      })
      notify.error(error?.message || '删除失败')
    } finally {
      setDeletingId('')
    }
  }

  function scheduleSavePayload(item, enabled = item.enabled) {
    return {
      id: item.id,
      name: item.name,
      action: item.action,
      enabled,
      timeOfDay: item.timeOfDay || '09:00',
      timezone: item.timezone || 'Asia/Shanghai',
      accountScope: item.accountScope || 'all',
      accountUids: item.accountUids || [],
      itemScope: item.itemScope || 'all',
      itemIds: item.itemIds || [],
      config: item.config || {},
    }
  }

  async function toggleScheduleEnabled(item) {
    const nextEnabled = !item.enabled
    setTogglingId(item.id)
    setItems((current) => current.map((row) => row.id === item.id ? { ...row, enabled: nextEnabled } : row))
    emitActionStarted({
      module: 'other-automation',
      action: nextEnabled ? 'enable_schedule' : 'disable_schedule',
      message: nextEnabled ? '开始启用定时任务' : '开始停用定时任务',
      context: scheduleLogContext(scheduleSavePayload(item, nextEnabled)),
    })
    try {
      const response = await saveAutomationSchedule(scheduleSavePayload(item, nextEnabled))
      if (response.item) {
        setItems((current) => current.map((row) => row.id === item.id ? response.item : row))
      }
      emitActionSuccess({
        module: 'other-automation',
        action: nextEnabled ? 'enable_schedule' : 'disable_schedule',
        message: nextEnabled ? '定时任务启用成功' : '定时任务停用成功',
        context: scheduleLogContext(response.item || scheduleSavePayload(item, nextEnabled)),
      })
      notify.success(`${item.name} 已${nextEnabled ? '启用' : '停用'}`)
      void loadSchedules()
    } catch (error) {
      setItems((current) => current.map((row) => row.id === item.id ? { ...row, enabled: item.enabled } : row))
      emitActionFailure({
        module: 'other-automation',
        action: nextEnabled ? 'enable_schedule' : 'disable_schedule',
        message: nextEnabled ? '定时任务启用失败' : '定时任务停用失败',
        error,
        context: scheduleLogContext(scheduleSavePayload(item, nextEnabled)),
      })
      notify.error(error?.message || `${nextEnabled ? '启用' : '停用'}失败`)
    } finally {
      setTogglingId('')
    }
  }

  async function runNow(id) {
    const target = items.find((item) => item.id === id)
    setItems((current) => current.map((item) => item.id === id ? { ...item, lastStatus: 'running', lastMessage: '正在执行...' } : item))
    emitActionStarted({
      module: 'other-automation',
      action: 'run_schedule_now',
      message: '开始立即执行定时任务',
      context: scheduleLogContext(target || { id }),
    })
    try {
      const result = await runAutomationScheduleOnce(id)
      const message = result.message || '执行完成'
      setItems((current) => current.map((item) => item.id === id ? {
        ...item,
        lastStatus: result.success === false ? 'failed' : 'success',
        lastMessage: message,
        lastRunAt: Date.now() / 1000,
      } : item))
      emitActionLog({
        module: 'other-automation',
        action: 'run_schedule_now',
        outcome: result.success === false ? 'failed' : 'success',
        message: result.success === false ? '定时任务立即执行失败' : '定时任务立即执行完成',
        detail: message,
        context: {
          ...scheduleLogContext(target || { id }),
          success: result.success !== false,
        },
      })
      notify.success(message)
      void loadSchedules()
    } catch (error) {
      setItems((current) => current.map((item) => item.id === id ? {
        ...item,
        lastStatus: 'failed',
        lastMessage: error?.message || '执行失败',
        lastRunAt: Date.now() / 1000,
      } : item))
      emitActionFailure({
        module: 'other-automation',
        action: 'run_schedule_now',
        message: '定时任务立即执行异常',
        error,
        context: scheduleLogContext(target || { id }),
      })
      notify.error(error?.message || '执行失败')
    }
  }

  return (
    <div className="h-full flex flex-col" style={{ background: 'var(--bg-page)' }}>
      <HubTabs tabs={HUB_PRESETS.automation} />
      <div className={isMobile ? 'flex-1 overflow-y-auto px-3 py-3 space-y-3' : 'flex-1 overflow-y-auto px-5 py-4 space-y-4'}>
        <section className="card p-4">
          <div className={isMobile ? 'space-y-3' : 'flex items-start justify-between gap-4'}>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span
                  className="inline-flex h-8 w-8 items-center justify-center rounded-[10px]"
                  style={{ background: 'var(--control-active)', color: 'var(--primary)' }}
                >
                  <CalendarClock className="h-4 w-4" />
                </span>
                <div>
                  <h1 className="text-[17px] font-semibold leading-tight" style={{ color: 'var(--text-main)' }}>定时任务</h1>
                  <div className="mt-0.5 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                    定时任务 · 商品 / 闲鱼币 / 曝光
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div className={isMobile ? 'mt-4 grid grid-cols-3 gap-2' : 'mt-4 grid grid-cols-4 gap-3'}>
            <SummaryCell label="已配置" value={items.length} />
            <SummaryCell label="启用中" value={enabledCount} tone="var(--success)" />
            <SummaryCell label="异常" value={failedCount} tone={failedCount ? 'var(--danger)' : 'var(--text-main)'} />
            {!isMobile ? <SummaryCell label="可用账号" value={activeAccounts.length} /> : null}
          </div>
        </section>

        <div className={isMobile ? 'grid gap-3' : 'grid grid-cols-[minmax(0,0.95fr)_minmax(520px,1.35fr)] gap-4 items-start'}>
          <section className="card p-4">
            <div className="mb-4 flex items-center justify-between gap-3">
              <div>
                <h2 className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>
                  {form.id ? '编辑配置' : '新建配置'}
                </h2>
                <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>{selectedAction.label}</div>
              </div>
              <span
                className="inline-flex items-center gap-1 rounded-full px-2 py-1 text-[11px]"
                style={{ background: 'var(--status-success-bg)', color: 'var(--status-success-text)' }}
              >
                <span className="h-1.5 w-1.5 rounded-full" style={{ background: 'var(--success)' }} />
                保存后启用
              </span>
            </div>

            <div className="grid gap-3">
              <label className="block">
                <span className="label">任务名称</span>
              <select className="input w-full" value={form.action} onChange={(event) => handleActionChange(event.target.value)}>
                {ACTIONS.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}
              </select>
            </label>
              <div className="grid gap-3">
                <label className="block">
                  <span className="label">执行时间</span>
                  <input className="input w-full" type="time" value={form.timeOfDay} onChange={(event) => patch({ timeOfDay: event.target.value })} />
                  <span className="mt-1 block text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    默认每天执行，执行时间使用 24 小时制。
                  </span>
                </label>
              </div>
            </div>

            <div className="mt-4 rounded-[14px] border p-3" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}>
              <div className="mb-2 flex items-center gap-2 text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                <Settings2 className="h-4 w-4" style={{ color: 'var(--primary)' }} /> 账号范围
              </div>
              <select className="input w-full" value={form.accountScope} onChange={(event) => patch({ accountScope: event.target.value })}>
                <option value="all">所有启用账号</option>
                <option value="selected">指定账号</option>
              </select>
              {form.accountScope === 'selected' ? (
                <div className="mt-3 grid max-h-[180px] gap-1.5 overflow-y-auto pr-1">
                  {activeAccounts.map((account) => (
                    <label key={account.uid} className="flex items-center gap-2 rounded-[10px] px-2 py-1.5 text-[13px]" style={{ color: 'var(--text-main)', background: 'rgba(255,255,255,0.58)' }}>
                      <input
                        type="checkbox"
                        checked={form.accountUids.includes(account.uid)}
                        onChange={(event) => patch({
                          accountUids: event.target.checked
                            ? [...form.accountUids, account.uid]
                            : form.accountUids.filter((uid) => uid !== account.uid),
                        })}
                      />
                      <span className="min-w-0 truncate">{account.nick || account.uid}</span>
                    </label>
                  ))}
                </div>
              ) : null}
            </div>

            <div className="mt-3 rounded-[14px] border p-3" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}>
              <div className="mb-2 flex items-center gap-2 text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                <Settings2 className="h-4 w-4" style={{ color: 'var(--primary)' }} /> 商品与参数
              </div>
              <div className="grid gap-2">
                {selectedAction.needsItems && !selectedAction.needsSingleItem ? (
                  <>
                    <select className="input w-full" value={form.itemScope} onChange={(event) => patch({ itemScope: event.target.value })}>
                      <option value="all">所有在售商品</option>
                      <option value="selected">指定商品 ID</option>
                    </select>
                    {form.itemScope === 'selected' ? (
                      <textarea className="input min-h-[86px] w-full" value={form.itemIdsText} onChange={(event) => patch({ itemIdsText: event.target.value })} placeholder="一行一个商品 ID" />
                    ) : null}
                  </>
                ) : null}
                {(selectedAction.value === 'coin_promotion' || selectedAction.value === 'exposure_promotion') ? (
                  <PromotionAccountPicker
                    action={selectedAction.value}
                    accounts={scheduleAccounts}
                    loading={promotionLoading}
                    itemsByUid={promotionItemsByUid}
                    coinPromotedItemIdsByUid={coinPromotedItemIdsByUid}
                    exposureBlockedItemIdsByUid={exposureBlockedItemIdsByUid}
                    coinPackagesByKey={coinPackagesByKey}
                    exposureAmountsByUid={exposureAmountsByUid}
                    selections={form.accountPromotions || {}}
                    onChange={patchAccountPromotion}
                    onRefresh={() => loadPromotionConfigData(scheduleAccounts)}
                  />
                ) : null}
                {selectedAction.needsAmount ? (
                  <label className="block">
                    <span className="label">{selectedAction.value === 'batch_restore_price' ? '每个商品涨价金额' : '每个商品降价金额'}</span>
                    <input className="input w-full" type="number" min="0" step="0.01" value={form.amount} onChange={(event) => patch({ amount: event.target.value })} placeholder="例如 1" />
                  </label>
                ) : null}
                {selectedAction.needsSingleItem && selectedAction.value !== 'coin_promotion' && selectedAction.value !== 'exposure_promotion' ? (
                  <input className="input w-full" value={form.itemId} onChange={(event) => patch({ itemId: event.target.value })} placeholder="商品 ID" />
                ) : null}
                {selectedAction.needsCoinCost && selectedAction.value !== 'coin_promotion' ? (
                  <div className="grid grid-cols-2 gap-2">
                    <input className="input w-full" value={form.costCoinNum} onChange={(event) => patch({ costCoinNum: event.target.value })} placeholder="消耗闲鱼币" />
                    <input className="input w-full" value={form.promotionNum} onChange={(event) => patch({ promotionNum: event.target.value })} placeholder="曝光档位" />
                  </div>
                ) : null}
                {selectedAction.needsExposure && selectedAction.value !== 'exposure_promotion' ? (
                  <input className="input w-full" value={form.exposureNum} onChange={(event) => patch({ exposureNum: event.target.value })} placeholder="曝光量" />
                ) : null}
                {!selectedAction.needsItems && !selectedAction.needsSingleItem && !selectedAction.needsAmount && !selectedAction.needsCoinCost && !selectedAction.needsExposure ? (
                  <div className="rounded-[12px] px-3 py-2 text-[12px]" style={{ background: 'rgba(255,255,255,0.58)', color: 'var(--text-secondary)' }}>
                    当前动作不需要额外商品参数。
                  </div>
                ) : null}
              </div>
            </div>

            <div className={isMobile ? 'mt-4 grid grid-cols-2 gap-2' : 'mt-4 flex justify-end gap-2'}>
              <button className="btn-secondary" onClick={() => setForm(emptyForm)}>重置</button>
              <button className="btn-primary" disabled={saving} onClick={submit}>
                <Save className="h-4 w-4" /> {saving ? '保存中' : '保存自动化'}
              </button>
            </div>
          </section>

          <section className="card overflow-hidden">
            <div className="flex items-center justify-between border-b px-4 py-3" style={{ borderColor: 'var(--border-light)' }}>
              <div>
                <h2 className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>已配置</h2>
                <div className="mt-0.5 text-[12px]" style={{ color: 'var(--text-secondary)' }}>{loading ? '加载中' : `${items.length} 个自动化`}</div>
              </div>
              <button className="btn-secondary btn-sm" onClick={loadSchedules}>刷新</button>
            </div>
            <div className="space-y-3 p-3">
              {items.map((item) => {
                const resultExpanded = Boolean(expandedResults[item.id])
                const resultText = item.lastMessage || '暂无执行结果'
                const canExpandResult = resultText.length > 42 || resultText.includes('\n')
                return (
                  <div
                    key={item.id}
                    className="rounded-[14px] border px-3 py-3"
                    style={{ borderColor: 'var(--control-border)', background: 'rgba(255,255,255,0.62)' }}
                  >
                    <div className={isMobile ? 'space-y-3' : 'grid grid-cols-[minmax(0,1fr)_auto] gap-4'}>
                      <div className="min-w-0">
                        <div className="flex min-w-0 flex-wrap items-center gap-2">
                          <span className="max-w-full truncate text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>{item.name}</span>
                          <StatusPill enabled={item.enabled} />
                          <span className="rounded-full px-2 py-0.5 text-[11px]" style={runStatusTone(item.lastStatus)}>
                            {runStatusLabel(item.lastStatus)}
                          </span>
                        </div>
                        <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                          <span title={item.id}>任务ID：<span className="font-mono">{shortTaskId(item.id)}</span></span>
                          <span>添加：{formatCreatedTime(item.createdAt)}</span>
                          <span>{accountScopeText(item)}</span>
                          <span>{itemScopeText(item)}</span>
                        </div>
                      </div>
                      <div className={isMobile ? 'grid grid-cols-2 gap-2' : 'flex items-start justify-end gap-2'}>
                        <button
                          className={item.enabled ? 'btn-secondary btn-sm' : 'btn-primary btn-sm'}
                          title={item.enabled ? '停用定时执行' : '启用定时执行'}
                          disabled={togglingId === item.id}
                          onClick={() => toggleScheduleEnabled(item)}
                        >
                          <Power className="h-4 w-4" />
                          {togglingId === item.id ? '处理中' : item.enabled ? '停用' : '启用'}
                        </button>
                        <button className="btn-secondary btn-sm" title="立即执行" onClick={() => runNow(item.id)}><Play className="h-4 w-4" />执行</button>
                        <button className="btn-secondary btn-sm" onClick={() => setForm(scheduleToForm(item))}>编辑</button>
                        <button className="btn-secondary btn-sm" title="删除" disabled={deletingId === item.id} onClick={() => remove(item.id)}>
                          <Trash2 className="h-4 w-4" />{deletingId === item.id ? '删除中' : ''}
                        </button>
                      </div>
                    </div>

                    <div className={isMobile ? 'mt-3 grid gap-2' : 'mt-3 grid grid-cols-4 gap-2'}>
                      <RunMetric label="执行时间" value={`${item.timeOfDay || '09:00'} · 每天`} icon={<Clock3 className="h-3.5 w-3.5" />} />
                      <RunMetric label="下次执行" value={formatTime(item.nextRunAt)} />
                      <RunMetric label="上次执行" value={formatTime(item.lastRunAt)} />
                      <RunMetric label="执行状态" value={runStatusLabel(item.lastStatus)} tone={runStatusTone(item.lastStatus)} />
                    </div>

                    <div className="mt-3 rounded-[12px] px-3 py-2 text-[12px]" style={{ background: 'var(--surface-soft)' }}>
                      <div className="mb-1 flex items-center justify-between gap-2">
                        <span className="font-medium" style={{ color: 'var(--text-main)' }}>执行结果</span>
                        {canExpandResult ? (
                          <button className="btn-secondary btn-sm" onClick={() => toggleResult(item.id)}>
                            {resultExpanded ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
                            {resultExpanded ? '收起' : '展开'}
                          </button>
                        ) : null}
                      </div>
                      <div
                        className={resultExpanded ? 'whitespace-pre-wrap break-words leading-5' : 'line-clamp-2 break-words leading-5'}
                        style={{ color: item.lastStatus === 'failed' ? 'var(--danger)' : 'var(--text-secondary)' }}
                      >
                        {resultText}
                      </div>
                    </div>
                  </div>
                )
              })}
              {!items.length ? (
                <div className="p-8 text-center">
                  <div
                    className="mx-auto mb-3 flex h-10 w-10 items-center justify-center rounded-[14px]"
                    style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
                  >
                    <CalendarClock className="h-5 w-5" />
                  </div>
                  <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>还没有配置定时任务</div>
                </div>
              ) : null}
            </div>
          </section>
        </div>
      </div>
    </div>
  )
}

function PromotionAccountPicker({
  action,
  accounts,
  loading,
  itemsByUid,
  coinPromotedItemIdsByUid,
  exposureBlockedItemIdsByUid,
  coinPackagesByKey,
  exposureAmountsByUid,
  selections,
  onChange,
  onRefresh,
}) {
  const isCoin = action === 'coin_promotion'
  return (
    <div className="grid gap-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
          每个账号只能选择一个商品，已在推广中的商品不会展示。
        </span>
        <button type="button" className="btn-secondary btn-sm" onClick={onRefresh} disabled={loading}>
          {loading ? '加载中' : '刷新商品'}
        </button>
      </div>
      {accounts.length ? accounts.map((account) => {
        const uid = account.uid
        const blockedIds = new Set(isCoin ? (coinPromotedItemIdsByUid[uid] || []) : (exposureBlockedItemIdsByUid[uid] || []))
        const availableItems = (itemsByUid[uid] || []).filter((item) => {
          const itemId = itemIdOf(item)
          return itemId && !blockedIds.has(itemId)
        })
        const selected = selections[uid] || {}
        const selectedItemId = selected.itemId || ''
        const selectedItem = availableItems.find((item) => itemIdOf(item) === selectedItemId)
        const packageKey = `${uid}:${selectedItemId}`
        const coinPackages = coinPackagesByKey[packageKey] || []
        const exposureAmounts = exposureAmountsByUid[uid] || []
        const selectedPackage = coinPackages.find((pkg) => String(pkg.promotionNum ?? '') === String(selected.promotionNum ?? '')) || coinPackages[0]
        const selectedExposure = String(selected.exposureNum || pickNumber(exposureAmounts[0], ['exposureNum', 'amount', 'num'], '100'))

        return (
          <div key={uid} className="rounded-[12px] border p-3" style={{ borderColor: 'var(--border-light)', background: 'rgba(255,255,255,0.58)' }}>
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{account.nick || uid}</div>
                <div className="mt-0.5 text-[11px]" style={{ color: 'var(--text-secondary)' }}>{uid}</div>
              </div>
              <span className="shrink-0 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                可选 {availableItems.length} 个
              </span>
            </div>
            <div className="grid gap-2">
              <select
                className="input w-full"
                value={selectedItemId}
                onChange={(event) => {
                  const itemId = event.target.value
                  const firstPackage = coinPackagesByKey[`${uid}:${itemId}`]?.[0]
                  const firstAmount = exposureAmounts[0]
                  onChange(uid, {
                    itemId,
                    itemTitle: itemTitleOf(availableItems.find((item) => itemIdOf(item) === itemId) || {}),
                    ...(isCoin ? {
                      promotionNum: firstPackage ? String(firstPackage.promotionNum ?? '') : '',
                      costCoinNum: Number(firstPackage?.costCoinNum ?? firstPackage?.cost ?? firstPackage?.coinNum ?? 0),
                      expectedCostCoinNum: Number(firstPackage?.costCoinNum ?? firstPackage?.cost ?? firstPackage?.coinNum ?? 0),
                    } : {
                      exposureNum: Number(pickNumber(firstAmount, ['exposureNum', 'amount', 'num'], '100')) || 100,
                      expectedExposureNum: Number(pickNumber(firstAmount, ['exposureNum', 'amount', 'num'], '100')) || 100,
                    }),
                  })
                }}
              >
                <option value="">{loading ? '商品加载中' : '请选择商品'}</option>
                {availableItems.map((item) => {
                  const itemId = itemIdOf(item)
                  const price = itemPriceOf(item)
                  return (
                    <option key={itemId} value={itemId}>
                      {itemTitleOf(item)}{price && price !== '—' ? ` · ¥${price}` : ''}
                    </option>
                  )
                })}
              </select>

              {selectedItem ? (
                <div className="flex items-center gap-2 rounded-[10px] border px-2 py-2" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-card-strong)' }}>
                  <div className="h-10 w-10 shrink-0 overflow-hidden rounded-[8px]" style={{ background: 'var(--surface-sunken)' }}>
                    {itemImageOf(selectedItem) ? <img src={itemImageOf(selectedItem)} className="h-full w-full object-cover" alt="" /> : null}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[12px] font-medium" style={{ color: 'var(--text-main)' }}>{itemTitleOf(selectedItem)}</div>
                    <div className="mt-0.5 truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>商品 ID {selectedItemId}</div>
                  </div>
                </div>
              ) : null}

              {isCoin && selectedItemId ? (
                <select
                  className="input w-full"
                  value={String(selected.promotionNum || selectedPackage?.promotionNum || '')}
                  onChange={(event) => {
                    const next = coinPackages.find((pkg) => String(pkg.promotionNum ?? '') === event.target.value)
                    onChange(uid, {
                      promotionNum: event.target.value,
                      costCoinNum: Number(next?.costCoinNum ?? next?.cost ?? next?.coinNum ?? 0),
                      expectedCostCoinNum: Number(next?.costCoinNum ?? next?.cost ?? next?.coinNum ?? 0),
                    })
                  }}
                >
                  {coinPackages.length ? coinPackages.map((pkg, index) => (
                    <option key={`${pkg.promotionNum ?? index}`} value={String(pkg.promotionNum ?? '')}>{packageLabel(pkg)}</option>
                  )) : <option value="">暂无可用推广套餐</option>}
                </select>
              ) : null}

              {!isCoin && selectedItemId ? (
                <select
                  className="input w-full"
                  value={selectedExposure}
                  onChange={(event) => {
                    const value = Number(event.target.value) || 100
                    onChange(uid, { exposureNum: value, expectedExposureNum: value })
                  }}
                >
                  {(exposureAmounts.length ? exposureAmounts : [{ exposureNum: 100, title: '默认档位' }]).map((item, index) => {
                    const value = String(pickNumber(item, ['exposureNum', 'amount', 'num'], '100'))
                    return <option key={`${value}-${index}`} value={value}>{value} 曝光</option>
                  })}
                </select>
              ) : null}
            </div>
          </div>
        )
      }) : (
        <div className="rounded-[12px] px-3 py-2 text-[12px]" style={{ background: 'rgba(255,255,255,0.58)', color: 'var(--text-secondary)' }}>
          请先选择可执行账号。
        </div>
      )}
    </div>
  )
}

function SummaryCell({ label, value, tone = 'var(--text-main)' }) {
  return (
    <div className="rounded-[14px] border px-3 py-2" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}>
      <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{label}</div>
      <div className="mt-1 text-[18px] font-semibold leading-none" style={{ color: tone }}>{value}</div>
    </div>
  )
}

function RunMetric({ label, value, icon = null, tone = null }) {
  const valueStyle = tone || { color: 'var(--text-main)' }
  return (
    <div className="min-w-0 rounded-[12px] px-3 py-2 text-[12px]" style={{ background: 'var(--surface-soft)' }}>
      <div className="mb-1 flex items-center gap-1.5" style={{ color: 'var(--text-secondary)' }}>
        {icon}
        <span>{label}</span>
      </div>
      <div className="truncate font-medium" style={valueStyle}>{value}</div>
    </div>
  )
}

function StatusPill({ enabled }) {
  return (
    <span
      className="shrink-0 rounded-full px-2 py-0.5 text-[11px]"
      style={{
        background: enabled ? 'var(--status-success-bg)' : 'var(--status-neutral-bg)',
        color: enabled ? 'var(--status-success-text)' : 'var(--text-secondary)',
      }}
    >
      {enabled ? '已启用' : '已停用'}
    </span>
  )
}
