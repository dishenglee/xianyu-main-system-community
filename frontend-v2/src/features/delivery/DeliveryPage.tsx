// @ts-nocheck
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Package, RefreshCw, Sparkles } from 'lucide-react'
import { useAccounts } from '../../hooks/useAccounts'
import {
  createDeliveryConfiguration,
  deleteDeliveryConfiguration,
  getCards,
  itemAllList,
  listDeliveryConfigurations,
  listDeliveryRuns,
  refreshDeliveryProductStock,
  retryDeliveryRun,
  updateAccountSetting,
  updateDeliveryConfiguration,
} from '../../api/index'
import { TopBar } from '../../components/TopBar'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { EmptyState } from '../../components/EmptyState'
import { SensitiveValue, maskSensitiveText, useSensitiveAvatarInitial, useSensitiveHidden } from '../../components/SensitiveValue'
import { Modal } from '../../components/Modal'
import { StatusBadge } from '../../components/StatusBadge'
import { useSafeNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import { useDevice } from '../../hooks/useDevice'
import { emitAccountsSynced } from '../runtimeEvents'
import { useAuthScope } from '../../hooks/useAuthScope'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import { limitConcurrency } from '../../hooks/limitConcurrency'
import {
  getStockRefreshCandidates,
  normalizeDeliveryProducts,
} from './deliveryProducts'
import CardsPage from '../cards/CardsPage'
import {
  buildDeliveryPoolOptions,
  findDeliveryPoolOption,
  sanitizeSelectedOneTimePoolTags,
  sanitizeSelectedFixedPoolTags,
  normalizeExclusiveDeliveryPoolTags,
  selectAvailableDeliveryPoolOptions,
} from './deliveryPoolOptions'
import { filterRunsForConfiguration } from './deliveryRunFilters'
import { buildDeliveryAccountScopeTargets, mergeDeliveryAccountResults } from './deliveryAccountScope'
import { buildDeliveryAccountGroups } from './deliveryAccountGroups'
import { buildDeliverySharedPoolWarnings } from './deliverySharedPoolWarnings'
import { completeDeliverySave } from './deliverySaveLifecycle'
import { DeliverySelectionCheckbox } from './DeliverySelectionCheckbox'
import DeliveryAccountMultiSelect from './DeliveryAccountMultiSelect'
import DeliveryProductAccountGroups from './DeliveryProductAccountGroups'
import {
  countDeliveryItemSelection,
  formatDeliveryCreateFailures,
  removeDeliveryAccountSelection,
  retainFailedDeliverySelections,
  runDeliveryMultiAccountCreate,
  toggleDeliveryItemSelection,
  type DeliveryItemSelectionByUid,
} from './deliveryMultiAccountCreate'
import {
  emptyDeliveryAccountProductState,
  loadDeliveryAccountProducts,
  type DeliveryAccountProductState,
} from './deliveryMultiAccountProducts'
import {
  getDeliverySelectionState,
  runDeliveryBatchDelete,
  toggleDeliverySelection,
} from './deliveryBatchDelete'

const emptyForm = {
  uid: '',
  itemId: '',
  poolScope: 'global',
  poolTags: [],
  fixedPoolTags: [],
  poolPreference: null,
  fixedEnabled: false,
  fixedMessage: '',
  enabled: true,
}

function configPoolTags(config) {
  const tags = Array.isArray(config?.poolTags) ? config.poolTags : (config?.oneTimeEnabled && config?.poolTag ? [config.poolTag] : [])
  return Array.from(new Set(tags.map((tag) => String(tag).trim()).filter(Boolean)))
}

function configFixedPoolTags(config) {
  const tags = Array.isArray(config?.fixedPoolTags)
    ? config.fixedPoolTags
    : (config?.fixedPoolTag ? [config.fixedPoolTag] : (!config?.oneTimeEnabled && config?.poolTag ? [config.poolTag] : []))
  return Array.from(new Set(tags.map((tag) => String(tag).trim()).filter(Boolean)))
}

function formatStock(quantity) {
  if (quantity === undefined || quantity === null) return '库存待同步'
  return `库存 ${quantity}`
}

function formatTime(value) {
  const number = Number(value || 0)
  if (!number) return '—'
  const millis = number > 10_000_000_000 ? number : number * 1000
  return new Date(millis).toLocaleString('zh-CN', { hour12: false })
}

function runStatus(status) {
  const map = {
    pending: ['neutral', '待执行'],
    waiting_stock: ['warning', '等补卡'],
    reserved: ['info', '已预占'],
    consigning: ['info', '发货中'],
    sending: ['info', '发送中'],
    retrying: ['warning', '稍后重试'],
    manual_required: ['danger', '需人工'],
    completed: ['success', '已完成'],
    cancelled: ['neutral', '已取消'],
  }
  return map[status] || ['neutral', status || '未知']
}

function AccountAvatar({ account }) {
  const [failed, setFailed] = useState(false)
  const label = account?.nick || account?.uid || '?'
  const initial = useSensitiveAvatarInitial(label)
  if (account?.avatar && !failed) {
    return <img src={account.avatar} alt="" onError={() => setFailed(true)} className="h-11 w-11 shrink-0 rounded-[12px] object-cover" />
  }
  return <div className="liquid-avatar flex h-11 w-11 shrink-0 items-center justify-center rounded-[12px] text-[16px] font-semibold">{initial}</div>
}

export default function DeliveryPage() {
  const { accounts, loadAccounts } = useAccounts()
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const useMobilePresentation = isMobile || isTablet
  const sensitiveHidden = useSensitiveHidden()
  const activeAccounts = useMemo(() => filterBusinessAccounts(accounts), [accounts])
  const activeAccountKey = useMemo(() => activeAccounts.map((a) => a.uid).join('|'), [activeAccounts])

  const [selectedUid, setSelectedUid] = useState('')
  const [configs, setConfigs] = useState([])
  const [runs, setRuns] = useState([])
  const [items, setItems] = useState([])
  const [loading, setLoading] = useState(false)
  const [runsLoading, setRunsLoading] = useState(false)
  const [itemsLoading, setItemsLoading] = useState(false)
  const [itemsError, setItemsError] = useState('')
  const [stockSyncFailures, setStockSyncFailures] = useState(0)
  const [poolCards, setPoolCards] = useState([])
  const [poolsLoading, setPoolsLoading] = useState(false)
  const [poolsError, setPoolsError] = useState('')
  const [showModal, setShowModal] = useState(false)
  const [editing, setEditing] = useState(null)
  const [form, setForm] = useState({ ...emptyForm })
  const [selectedCreateUids, setSelectedCreateUids] = useState<string[]>([])
  const [createProductStatesByUid, setCreateProductStatesByUid] = useState<Record<string, DeliveryAccountProductState>>({})
  const [selectedCreateItemIdsByUid, setSelectedCreateItemIdsByUid] = useState<DeliveryItemSelectionByUid>({})
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [togglingConfigId, setTogglingConfigId] = useState(0)
  const [expandedRunConfigs, setExpandedRunConfigs] = useState(() => new Set())
  const [expandedAccountUids, setExpandedAccountUids] = useState(() => new Set())
  const [selectedConfigIds, setSelectedConfigIds] = useState(() => new Set())
  const [batchDeleting, setBatchDeleting] = useState(false)

  const configSeqRef = useRef(0)
  const itemsSeqRef = useRef(0)
  const runsSeqRef = useRef(0)
  const poolsSeqRef = useRef(0)
  const createItemsSeqRef = useRef<Record<string, number>>({})
  const saveSeqRef = useRef(0)
  const autoExpandedMismatchUidsRef = useRef(new Set())
  const selectedUidRef = useRef(selectedUid)
  selectedUidRef.current = selectedUid
  const selectedCreateUidsRef = useRef(selectedCreateUids)
  selectedCreateUidsRef.current = selectedCreateUids

  const selectedAccount = activeAccounts.find((account) => account.uid === form.uid)
  const selectedAccountLabel = selectedAccount ? (selectedAccount.nick || selectedAccount.uid) : '请选择账号'
  const accountLabelByUid = useMemo(() => new Map(activeAccounts.map((account) => [account.uid, account.nick || account.uid])), [activeAccounts])
  const accountByUid = useMemo(() => new Map(activeAccounts.map((account) => [account.uid, account])), [activeAccounts])
  const itemMap = useMemo(() => new Map(items.map((item) => [item.id, item])), [items])
  const selectedProduct = itemMap.get(form.itemId)
  const onlineItems = items
  const selectedCreateAccounts = useMemo(
    () => selectedCreateUids.map((uid) => activeAccounts.find((account) => account.uid === uid)).filter(Boolean),
    [activeAccounts, selectedCreateUids],
  )
  const selectedCreateItemCount = useMemo(
    () => countDeliveryItemSelection(Object.fromEntries(
      selectedCreateAccounts.map((account) => [account.uid, selectedCreateItemIdsByUid[account.uid] ?? []]),
    )),
    [selectedCreateAccounts, selectedCreateItemIdsByUid],
  )
  const poolOptions = useMemo(
    () => buildDeliveryPoolOptions(poolCards),
    [poolCards],
  )
  const selectablePoolOptions = useMemo(() => selectAvailableDeliveryPoolOptions(poolOptions, true), [poolOptions])
  const selectableFixedPoolOptions = useMemo(() => poolOptions.filter((option) => !option.oneTimeEnabled && option.totalCount > 0), [poolOptions])
  const validPoolTags = useMemo(() => sanitizeSelectedOneTimePoolTags(form.poolTags, poolOptions), [form.poolTags, poolOptions])
  const validFixedPoolTags = useMemo(() => sanitizeSelectedFixedPoolTags(form.fixedPoolTags, poolOptions), [form.fixedPoolTags, poolOptions])
  const selectedPools = useMemo(() => validPoolTags.map((tag) => findDeliveryPoolOption(poolOptions, tag, true)).filter(Boolean), [poolOptions, validPoolTags])
  const selectedPoolsAvailable = selectedPools.reduce((sum, pool) => sum + pool.availableCount, 0)
  const accountGroups = useMemo(() => buildDeliveryAccountGroups(configs), [configs])
  const sharedPoolWarnings = useMemo(() => buildDeliverySharedPoolWarnings(configs, poolOptions), [configs, poolOptions])
  const allConfigIds = useMemo(() => configs.map((config) => config.id), [configs])
  const allSelectionState = useMemo(
    () => getDeliverySelectionState(selectedConfigIds, allConfigIds),
    [allConfigIds, selectedConfigIds],
  )

  useEffect(() => {
    const availableIds = new Set(allConfigIds)
    setSelectedConfigIds((current) => {
      const next = new Set([...current].filter((id) => availableIds.has(id)))
      return next.size === current.size ? current : next
    })
  }, [allConfigIds])

  useEffect(() => {
    const newMismatchUids = accountGroups
      .filter((group) => group.mismatchCount > 0 && !autoExpandedMismatchUidsRef.current.has(group.uid))
      .map((group) => group.uid)
    if (!newMismatchUids.length) return
    newMismatchUids.forEach((uid) => autoExpandedMismatchUidsRef.current.add(uid))
    setExpandedAccountUids((current) => new Set([...current, ...newMismatchUids]))
  }, [accountGroups])

  const clearPageState = useCallback(() => {
    configSeqRef.current += 1
    itemsSeqRef.current += 1
    runsSeqRef.current += 1
    poolsSeqRef.current += 1
    createItemsSeqRef.current = {}
    saveSeqRef.current += 1
    setSelectedUid('')
    setConfigs([])
    setRuns([])
    setItems([])
    setItemsError('')
    setStockSyncFailures(0)
    setPoolCards([])
    setPoolsLoading(false)
    setPoolsError('')
    setShowModal(false)
    setEditing(null)
    setForm({ ...emptyForm })
    setSelectedCreateUids([])
    selectedCreateUidsRef.current = []
    setCreateProductStatesByUid({})
    setSelectedCreateItemIdsByUid({})
    setError('')
    setSaving(false)
    setLoading(false)
    setRunsLoading(false)
    setItemsLoading(false)
    setTogglingConfigId(0)
    setExpandedRunConfigs(new Set())
    setExpandedAccountUids(new Set())
    autoExpandedMismatchUidsRef.current = new Set()
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  useEffect(() => { loadAccounts() }, [authToken])
  useEffect(() => { void loadPoolOptions() }, [authToken, activeAccountKey])
  useEffect(() => {
    if (poolsLoading || !poolCards.length) return
    setForm((current) => {
      const { poolTags, fixedPoolTags } = normalizeExclusiveDeliveryPoolTags(current.poolTags, current.fixedPoolTags, poolOptions, current.poolPreference ?? undefined)
      const unchanged = poolTags.length === current.poolTags.length && poolTags.every((tag, index) => tag === current.poolTags[index])
        && fixedPoolTags.length === current.fixedPoolTags.length && fixedPoolTags.every((tag, index) => tag === current.fixedPoolTags[index])
      return unchanged ? current : { ...current, poolTags, fixedPoolTags }
    })
  }, [poolsLoading, poolCards, poolOptions])
  useEffect(() => {
    if (selectedUid && !activeAccounts.some((account) => account.uid === selectedUid)) setSelectedUid('')
  }, [selectedUid, activeAccountKey])
  useEffect(() => {
    loadConfigurations()
    loadRuns()
    if (selectedUid) {
      loadItems()
    }
    else {
      setItems([])
      setItemsError('')
      setStockSyncFailures(0)
    }
  }, [selectedUid, authToken, activeAccountKey, showModal])

  async function loadConfigurations() {
    const scope = captureScope('')
    const seq = ++configSeqRef.current
    const targets = buildDeliveryAccountScopeTargets(activeAccounts, '')
    if (!targets.length) { setConfigs([]); return }
    setLoading(true)
    try {
      const results = await limitConcurrency(targets, 4, async (uid) => {
        try { return { uid, records: await listDeliveryConfigurations(uid), failed: false } }
        catch { return { uid, records: [], failed: true } }
      })
      if (!isScopeCurrent(scope, '') || configSeqRef.current !== seq) return
      setConfigs(mergeDeliveryAccountResults(results.map((result) => ({ ...result, records: Array.isArray(result.records) ? result.records.map((config) => ({ ...config, poolTags: configPoolTags(config), fixedPoolTags: configFixedPoolTags(config) })) : [] }))))
      const failedCount = results.filter((result) => result.failed).length
      if (failedCount) notify(`有 ${failedCount} 个账号的自动发货配置加载失败，已展示其余账号数据`, 'error')
    } catch (e) {
      if (isScopeCurrent(scope, '')) notify(e?.message || '加载自动发货配置失败', 'error')
    } finally {
      if (isScopeCurrent(scope, '') && configSeqRef.current === seq) setLoading(false)
    }
  }

  async function loadRuns() {
    const scope = captureScope('')
    const seq = ++runsSeqRef.current
    const targets = buildDeliveryAccountScopeTargets(activeAccounts, '')
    if (!targets.length) { setRuns([]); return }
    setRunsLoading(true)
    try {
      const results = await limitConcurrency(targets, 4, async (uid) => {
        try { return { uid, records: await listDeliveryRuns({ uid, page: 1, pageSize: 50 }), failed: false } }
        catch { return { uid, records: [], failed: true } }
      })
      if (!isScopeCurrent(scope, '') || runsSeqRef.current !== seq) return
      setRuns(mergeDeliveryAccountResults(results.map((result) => ({ ...result, records: Array.isArray(result.records) ? result.records : [] }))))
      const failedCount = results.filter((result) => result.failed).length
      if (failedCount) notify(`有 ${failedCount} 个账号的执行记录加载失败，已展示其余账号数据`, 'error')
    } catch (e) {
      if (isScopeCurrent(scope, '')) notify(e?.message || '加载执行记录失败', 'error')
    } finally {
      if (isScopeCurrent(scope, '') && runsSeqRef.current === seq) setRunsLoading(false)
    }
  }

  async function loadItems() {
    if (!selectedUid) return
    const scope = captureScope(selectedUid)
    const seq = ++itemsSeqRef.current
    setItemsLoading(true)
    setItemsError('')
    try {
      const response = await itemAllList(selectedUid, 1, 200)
      if (!isScopeCurrent(scope, selectedUidRef.current) || itemsSeqRef.current !== seq) return
      const normalized = normalizeDeliveryProducts(response)
      setItems(normalized)
      const pending = getStockRefreshCandidates(normalized)
      setStockSyncFailures(0)
      if (pending.length) {
        const results = await limitConcurrency(pending, 3, async (item) => {
          try {
            const result = await refreshDeliveryProductStock(selectedUid, item.id)
            return { id: item.id, quantity: result.quantity, failed: false }
          } catch {
            return { id: item.id, quantity: null, failed: true }
          }
        })
        if (!isScopeCurrent(scope, selectedUidRef.current) || itemsSeqRef.current !== seq) return
        const resultMap = new Map(results.map((result) => [result.id, result]))
        setItems((current) => current.map((item) => {
          const result = resultMap.get(item.id)
          return result && !result.failed ? { ...item, quantity: result.quantity } : item
        }))
        setStockSyncFailures(results.filter((result) => result.failed || result.quantity === null).length)
      }
    } catch (e) {
      if (!isScopeCurrent(scope, selectedUidRef.current) || itemsSeqRef.current !== seq) return
      setItems([])
      setItemsError(e?.message || '商品加载失败')
    } finally {
      if (isScopeCurrent(scope, selectedUidRef.current) && itemsSeqRef.current === seq) {
        setItemsLoading(false)
      }
    }
  }

  async function loadCreateAccountItems(uid) {
    if (!uid || !selectedCreateUidsRef.current.includes(uid)) return
    const scope = captureScope(uid)
    const seq = (createItemsSeqRef.current[uid] || 0) + 1
    createItemsSeqRef.current[uid] = seq
    setCreateProductStatesByUid((current) => ({
      ...current,
      [uid]: { ...emptyDeliveryAccountProductState(), items: current[uid]?.items ?? [], loading: true },
    }))
    const isCurrent = () => (
      isScopeCurrent(scope, uid)
      && createItemsSeqRef.current[uid] === seq
      && selectedCreateUidsRef.current.includes(uid)
    )
    const result = await loadDeliveryAccountProducts({
      uid,
      listProducts: itemAllList,
      refreshStock: refreshDeliveryProductStock,
      isCurrent,
      onProductsLoaded: (state) => {
        if (!isCurrent()) return
        setCreateProductStatesByUid((current) => ({ ...current, [uid]: state }))
      },
    })
    if (result && isCurrent()) {
      setCreateProductStatesByUid((current) => ({
        ...current,
        [uid]: result,
      }))
    }
  }

  function changeCreateAccounts(nextUids) {
    const previousSet = new Set(selectedCreateUidsRef.current)
    const nextSet = new Set(nextUids)
    const removedUids = [...previousSet].filter((uid) => !nextSet.has(uid))
    const addedUids = nextUids.filter((uid) => !previousSet.has(uid))
    selectedCreateUidsRef.current = nextUids
    setSelectedCreateUids(nextUids)
    if (removedUids.length) {
      for (const uid of removedUids) createItemsSeqRef.current[uid] = (createItemsSeqRef.current[uid] || 0) + 1
      setSelectedCreateItemIdsByUid((current) => removedUids.reduce(
        (selection, uid) => removeDeliveryAccountSelection(selection, uid),
        current,
      ))
      setCreateProductStatesByUid((current) => {
        const next = { ...current }
        removedUids.forEach((uid) => delete next[uid])
        return next
      })
    }
    addedUids.forEach((uid) => { void loadCreateAccountItems(uid) })
  }

  async function loadPoolOptions() {
    const scope = captureScope('')
    const seq = ++poolsSeqRef.current
    setPoolsLoading(true)
    setPoolsError('')
    try {
      const globalCards = await getCards('__global__')
      if (!isScopeCurrent(scope, '') || poolsSeqRef.current !== seq) return
      setPoolCards(Array.isArray(globalCards) ? globalCards : [])
    } catch (e) {
      if (isScopeCurrent(scope, '')) {
        setPoolCards([])
        setPoolsError(e?.message || '加载卡密池失败')
      }
    } finally {
      if (isScopeCurrent(scope, '') && poolsSeqRef.current === seq) setPoolsLoading(false)
    }
  }

  function openCreate() {
    saveSeqRef.current += 1
    setSelectedUid('')
    setItems([])
    setEditing(null)
    setForm({
      ...emptyForm,
      uid: '',
      itemId: '',
    })
    setSelectedCreateUids([])
    selectedCreateUidsRef.current = []
    setCreateProductStatesByUid({})
    setSelectedCreateItemIdsByUid({})
    setError('')
    setShowModal(true)
  }

  function openEdit(config) {
    saveSeqRef.current += 1
    setSelectedUid(config.uid)
    setEditing(config)
    setSelectedCreateUids([])
    selectedCreateUidsRef.current = []
    setCreateProductStatesByUid({})
    setSelectedCreateItemIdsByUid({})
    setForm({
      uid: config.uid,
      itemId: config.itemId,
      poolScope: 'global',
      poolTags: configPoolTags(config),
      fixedPoolTags: configFixedPoolTags(config),
      poolPreference: typeof config.oneTimeEnabled === 'boolean' ? config.oneTimeEnabled : null,
      fixedEnabled: Boolean(config.fixedMessage),
      fixedMessage: config.fixedMessage || '',
      enabled: Boolean(config.enabled),
    })
    setError('')
    setShowModal(true)
  }

  function closeModal() {
    saveSeqRef.current += 1
    setSelectedUid('')
    setShowModal(false)
    setEditing(null)
    setForm({ ...emptyForm })
    setSelectedCreateUids([])
    selectedCreateUidsRef.current = []
    setCreateProductStatesByUid({})
    setSelectedCreateItemIdsByUid({})
    setPoolsError('')
    setError('')
  }

  async function submit() {
    setError('')
    const createTargetUids = selectedCreateAccounts.map((account) => account.uid)
    if (editing && !form.uid) { setError('请先选择账号'); return }
    if (!editing && !createTargetUids.length) { setError('请先选择账号'); return }
    if (editing && !form.itemId.trim()) { setError('请选择要自动发货的商品'); return }
    if (!editing && !selectedCreateItemCount) { setError('请至少选择一个要自动发货的商品'); return }
    const { poolTags: submitPoolTags, fixedPoolTags: submitFixedPoolTags } = normalizeExclusiveDeliveryPoolTags(form.poolTags, form.fixedPoolTags, poolOptions, form.poolPreference ?? undefined)
    if (!submitPoolTags.length && !submitFixedPoolTags.length && !(form.fixedEnabled && form.fixedMessage.trim())) {
      setError('请至少选择一个一次性卡密池、固定卡密池，或开启并填写固定说明')
      return
    }
    setSaving(true)
    const saveSeq = ++saveSeqRef.current
    const saveScopeKey = editing ? selectedUidRef.current : [...createTargetUids].sort().join('|')
    const scope = captureScope(saveScopeKey)
    const activeCreateUidSet = new Set(activeAccounts.map((account) => account.uid))
    const currentSaveScopeKey = () => editing
      ? selectedUidRef.current
      : selectedCreateUidsRef.current.filter((uid) => activeCreateUidSet.has(uid)).sort().join('|')
    const saveIsCurrent = () => saveSeqRef.current === saveSeq && isScopeCurrent(scope, currentSaveScopeKey())
    try {
      if (editing) {
        await updateDeliveryConfiguration(editing.id, {
          poolScope: 'global',
          poolTags: submitPoolTags,
          fixedPoolTags: submitFixedPoolTags,
          fixedMessage: form.fixedEnabled ? form.fixedMessage.trim() : '',
          oneTimeEnabled: submitPoolTags.length > 0,
          enabled: form.enabled,
        })
        if (!saveIsCurrent()) return
      } else {
        const productsByUid = Object.fromEntries(
          createTargetUids.map((uid) => [uid, createProductStatesByUid[uid]?.items ?? []]),
        )
        const configuredItemIdsByUid = Object.fromEntries(
          createTargetUids.map((uid) => [
            uid,
            configs.filter((config) => config.uid === uid).map((config) => config.itemId),
          ]),
        )
        const result = await runDeliveryMultiAccountCreate({
          selectedUids: createTargetUids,
          selectedItemIdsByUid: selectedCreateItemIdsByUid,
          productsByUid,
          configuredItemIdsByUid,
          refreshStock: refreshDeliveryProductStock,
          createConfiguration: createDeliveryConfiguration,
          payload: {
            poolScope: 'global',
            poolTags: submitPoolTags,
            fixedPoolTags: submitFixedPoolTags,
            fixedMessage: form.fixedEnabled ? form.fixedMessage.trim() : '',
            oneTimeEnabled: submitPoolTags.length > 0,
            enabled: form.enabled,
          },
        })
        if (!saveIsCurrent()) return

        const stockByUid = new Map()
        result.stock.forEach((stock) => {
          if (!stock.success) return
          const current = stockByUid.get(stock.uid) ?? new Map()
          current.set(stock.itemId, stock.quantity)
          stockByUid.set(stock.uid, current)
        })
        setCreateProductStatesByUid((current) => Object.fromEntries(
          Object.entries(current).map(([uid, state]) => {
            const quantities = stockByUid.get(uid)
            if (!quantities) return [uid, state]
            return [uid, {
              ...state,
              items: state.items.map((item) => quantities.has(item.id) ? { ...item, quantity: quantities.get(item.id) } : item),
            }]
          }),
        ))

        const createdUids = Array.from(new Set(result.created.map((created) => created.uid)))
        if (createdUids.length) {
          emitAccountsSynced({ uids: createdUids, source: 'delivery-config-save' })
          void loadAccounts().catch(() => undefined)
        }
        if (!saveIsCurrent()) return

        const failures = result.failures
        setSelectedCreateItemIdsByUid(retainFailedDeliverySelections(result.failures))
        if (failures.length) {
          const details = formatDeliveryCreateFailures(failures, accountLabelByUid)
          const summary = `批量创建部分完成：成功 ${result.created.length} 条，失败 ${failures.length} 条。${details}`
          setError(summary)
          notify(summary, 'error')
          void Promise.all([loadConfigurations(), loadRuns()])
          return
        }

        completeDeliverySave({
          releaseSaving: () => setSaving(false),
          close: closeModal,
          notifySuccess: () => notify(`已成功新增 ${result.created.length} 条自动发货配置`, 'success'),
          refresh: async () => {
            await Promise.all([loadConfigurations(), loadRuns()])
          },
        })
        return
      }
      completeDeliverySave({
        releaseSaving: () => setSaving(false),
        close: closeModal,
        notifySuccess: () => notify('自动发货配置已更新', 'success'),
        refresh: async () => {
          if (form.enabled) {
            emitAccountsSynced({ uids: [form.uid], source: 'delivery-config-save' })
            await loadAccounts()
          }
          await Promise.all([loadConfigurations(), loadRuns()])
        },
      })
    } catch (e) {
      if (saveIsCurrent()) setError(e?.message || '保存自动发货配置失败')
    } finally {
      if (saveSeqRef.current === saveSeq) setSaving(false)
    }
  }

  async function toggleConfigEnabled(config) {
    const nextEnabled = !config.enabled
    setTogglingConfigId(config.id)
    const viewUid = selectedUidRef.current
    const scope = captureScope(viewUid)
    try {
      await updateDeliveryConfiguration(config.id, { enabled: nextEnabled })
      if (nextEnabled) {
        await updateAccountSetting(config.uid, { autoShip: true })
        emitAccountsSynced({ uids: [config.uid], source: 'delivery-config-toggle' })
        await loadAccounts()
      }
      if (!isScopeCurrent(scope, selectedUidRef.current)) return
      await Promise.all([loadConfigurations(), loadRuns()])
      notify(nextEnabled ? '自动发货配置已启用' : '自动发货配置已停用', 'success')
    } catch (e) {
      if (isScopeCurrent(scope, selectedUidRef.current)) notify(e?.message || '切换配置状态失败', 'error')
    } finally {
      if (isScopeCurrent(scope, selectedUidRef.current)) setTogglingConfigId(0)
    }
  }

  async function remove(config) {
    const name = config.product?.title || config.itemId
    const confirmed = await showConfirm(`确认删除"${name}"的自动发货配置？`, {
      title: '删除自动发货配置',
      confirmLabel: '删除',
      danger: true,
    })
    if (!confirmed) return
    try {
      await deleteDeliveryConfiguration(config.id)
      const remainingForUid = configs.filter((item) => item.id !== config.id && item.uid === config.uid)
      if (!remainingForUid.length) {
        emitAccountsSynced({ uids: [config.uid], source: 'delivery-config-delete' })
        await loadAccounts()
      }
      await Promise.all([loadConfigurations(), loadRuns()])
      notify('自动发货配置已删除', 'success')
    } catch (e) {
      notify(e?.message || '删除自动发货配置失败', 'error')
    }
  }

  async function removeSelected() {
    const ids = [...selectedConfigIds]
    if (!ids.length || batchDeleting) return
    const confirmed = await showConfirm(`确认批量删除已选中的 ${ids.length} 条自动发货配置？删除后不可恢复。`, {
      title: '批量删除自动发货配置',
      confirmLabel: `删除 ${ids.length} 条`,
      danger: true,
    })
    if (!confirmed) return

    setBatchDeleting(true)
    try {
      const result = await runDeliveryBatchDelete(ids, (id) => deleteDeliveryConfiguration(id), 3)
      const failedIds = new Set(result.failures.map((failure) => failure.id))
      const succeededIds = new Set(result.succeededIds)
      setConfigs((current) => current.filter((config) => !succeededIds.has(config.id)))
      setSelectedConfigIds(failedIds)

      if (!result.failures.length) {
        notify(`已删除 ${result.succeededIds.length} 条自动发货配置`, 'success')
      } else {
        const reasons = Array.from(new Set(result.failures.map((failure) => failure.reason))).join('；')
        notify(`批量删除部分完成：成功 ${result.succeededIds.length} 条，失败 ${result.failures.length} 条。失败原因：${reasons}`, 'error')
      }
      const affectedUids = Array.from(new Set(
        configs
          .filter((config) => succeededIds.has(config.id))
          .map((config) => config.uid)
          .filter(Boolean),
      ))
      if (affectedUids.length) {
        emitAccountsSynced({ uids: affectedUids, source: 'delivery-config-batch-delete' })
        await loadAccounts()
      }
      void Promise.all([loadConfigurations(), loadRuns()])
    } finally {
      setBatchDeleting(false)
    }
  }

  async function refreshStock(config) {
    try {
      const result = await refreshDeliveryProductStock(config.uid, config.itemId)
      await loadConfigurations()
      notify(`商品库存已同步：${formatStock(result.quantity)}`, 'success')
    } catch (e) {
      notify(e?.message || '刷新商品库存失败', 'error')
    }
  }

  async function retryRun(run) {
    try {
      await retryDeliveryRun(run.id)
      await loadRuns()
      notify('自动发货任务已重新排队', 'success')
    } catch (e) {
      notify(e?.message || '任务不能直接重试，请先人工核对', 'error')
    }
  }

  const pageClass = useMobilePresentation ? 'h-full flex flex-col mobile-page-shell' : 'contents'

  return (
    <div className={pageClass} style={useMobilePresentation ? { backgroundColor: 'var(--bg-page)' } : undefined}>
      {useMobilePresentation ? <TopBar title="自动发货" /> : null}
      {!useMobilePresentation && <HubTabs tabs={HUB_PRESETS.automation} />}

      <div className={useMobilePresentation ? 'flex-1 overflow-y-auto mobile-page-body' : 'flex-1 overflow-y-auto px-3 py-3 md:px-6 md:py-4'}>
        <div className={useMobilePresentation ? 'mobile-page-stack' : 'space-y-3 md:space-y-4'}>
          <CardsPage embedded />

          <section className={`${useMobilePresentation ? 'mobile-surface-card px-3.5 py-3 space-y-3' : 'card px-4 py-3 space-y-3'} overflow-visible`}>
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-baseline gap-2.5">
                <h2 className={useMobilePresentation ? 'mobile-section-title' : 'text-[16px] font-semibold'} style={{ color: 'var(--text-main)' }}>自动发货配置</h2>
                <p className="hidden truncate text-[12px] sm:block" style={{ color: 'var(--text-secondary)' }}>
                  从上方已有卡密池中选择发货内容，不在配置里重复录入卡密。
                </p>
              </div>
              <button onClick={openCreate} className="btn-primary btn-sm shrink-0">
                新增配置
              </button>
            </div>

            <div className="flex items-center justify-between gap-3 border-t pt-3" style={{ borderColor: 'var(--border-light)' }}>
              <div>
                <h2 className={useMobilePresentation ? 'mobile-section-title' : 'text-[16px] font-semibold'} style={{ color: 'var(--text-main)' }}>已保存自动发货配置</h2>
                <p className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  一次性卡密按购买数量消耗；固定卡密每单重复发送，不消耗。
                </p>
              </div>
              <span className="rounded-full px-3 py-1 text-[12px]" style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>{accountGroups.length} 个账号</span>
            </div>

            {configs.length ? (
              <div className="flex flex-col gap-2 rounded-[12px] border px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}>
                <div className="flex min-w-0 items-center gap-2">
                  <DeliverySelectionCheckbox
                    checked={allSelectionState.checked}
                    indeterminate={allSelectionState.indeterminate}
                    disabled={batchDeleting}
                    label="全选当前自动发货配置"
                    onChange={(checked) => setSelectedConfigIds((current) => toggleDeliverySelection(current, allConfigIds, checked))}
                  />
                  <span className="truncate text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>已选 {selectedConfigIds.size} 条</span>
                </div>
                <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                  <button type="button" className="btn-ghost btn-sm" disabled={batchDeleting || allSelectionState.checked} onClick={() => setSelectedConfigIds(new Set(allConfigIds))}>全选当前列表</button>
                  <button type="button" className="btn-ghost btn-sm" disabled={batchDeleting || !selectedConfigIds.size} onClick={() => setSelectedConfigIds(new Set())}>清空</button>
                  <button type="button" className="btn-sm" style={{ color: 'var(--danger)' }} disabled={batchDeleting || !selectedConfigIds.size} onClick={removeSelected}>{batchDeleting ? '批量删除中…' : '批量删除'}</button>
                </div>
              </div>
            ) : null}

            <div className="space-y-2.5">
              {loading ? (
                <div className="flex items-center justify-center py-20 gap-2" style={{ color: 'var(--text-secondary)' }}>
                  <RefreshCw className="w-4 h-4 animate-spin" /> 加载中…
                </div>
              ) : !configs.length ? (
                <EmptyState icon={Sparkles} title="还没有自动发货配置" description="新增配置后，成交订单会进入自动发货任务" />
              ) : (
                <div className="space-y-2.5">
                  {accountGroups.map((group) => {
                    const accountExpanded = expandedAccountUids.has(group.uid)
                    const accountName = accountLabelByUid.get(group.uid) || group.uid
                    const safeAccountName = sensitiveHidden ? maskSensitiveText('account', accountName) : accountName
                    const groupConfigIds = group.configs.map((config) => config.id)
                    const configuredItemIds = Array.from(new Set(group.configs.map((config) => config.itemId).filter(Boolean)))
                    const groupSelectionState = getDeliverySelectionState(selectedConfigIds, groupConfigIds)
                    const groupPoolWarnings = Array.from(new Set(group.configs
                      .filter((config) => config.enabled && config.oneTimeEnabled)
                      .flatMap((config) => configPoolTags(config))
                      .filter(Boolean)))
                      .map((tag) => sharedPoolWarnings.get(tag))
                      .filter(Boolean)
                    return (
                    <div key={group.uid} className={useMobilePresentation ? 'mobile-soft-card p-3.5' : 'card p-3.5 md:p-4'}>
                      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
                        <div className="flex min-w-0 flex-1 items-center gap-3">
                          <DeliverySelectionCheckbox
                            checked={groupSelectionState.checked}
                            indeterminate={groupSelectionState.indeterminate}
                            disabled={batchDeleting}
                            label={`选择${safeAccountName}全部配置`}
                            onChange={(checked) => setSelectedConfigIds((current) => toggleDeliverySelection(current, groupConfigIds, checked))}
                          />
                          <AccountAvatar account={accountByUid.get(group.uid)} />
                          <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-2">
                              <SensitiveValue kind="account" className="truncate text-[14px] font-semibold" style={{ color: 'var(--text-main)' }} value={accountName} />
                              <StatusBadge status={group.enabledCount ? 'info' : 'neutral'} label={group.enabledCount ? '运行中' : '已停用'} />
                              {group.repairCount ? <StatusBadge status="danger" label={`${group.repairCount} 项需修复`} /> : null}
                              {group.stockIssueCount ? <StatusBadge status="warning" label={`${group.stockIssueCount} 项库存异常`} /> : null}
                              {group.mismatchCount ? <StatusBadge status="danger" label={`${group.mismatchCount} 笔待发货未匹配`} /> : null}
                            </div>
                            <div className="mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>账号 <SensitiveValue kind="uid" value={group.uid} /></div>
                          </div>
                        </div>
                        <div className="grid grid-cols-3 gap-2 sm:min-w-[300px]">
                          {[
                            ['已配置', `${group.configuredCount} 个商品`],
                            ['已启用', `${group.enabledCount} 个`],
                            ['库存异常', `${group.stockIssueCount} 个`],
                          ].map(([label, value]) => (
                            <div key={label} className="rounded-[10px] px-2.5 py-2 text-center" style={{ background: 'var(--surface-soft)' }}>
                              <div className="text-[10px]" style={{ color: 'var(--text-muted)' }}>{label}</div>
                              <div className="mt-0.5 text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>{value}</div>
                            </div>
                          ))}
                        </div>
                        <button
                          onClick={() => setExpandedAccountUids((current) => {
                            const next = new Set(current)
                            if (accountExpanded) next.delete(group.uid)
                            else next.add(group.uid)
                            return next
                          })}
                          className="btn-primary btn-sm shrink-0"
                          aria-expanded={accountExpanded}
                        >
                          {accountExpanded ? '收起详情' : '查看详情'}
                        </button>
                      </div>
                      {group.mismatchCount ? (
                        <div className="liquid-notice liquid-notice-danger mt-3 rounded-xl px-3 py-2 text-[12px]">
                          <div className="font-semibold">待发货订单商品 ID 与自动发货配置不一致</div>
                          <div className="mt-1 space-y-1">
                            {group.mismatches.map((mismatch) => (
                              <div key={`${mismatch.orderId || ''}-${mismatch.itemId || ''}`}>
                                订单号：<SensitiveValue kind="order-id" value={mismatch.orderId || '未返回'} /> · 实际商品 ID：<SensitiveValue kind="order-id" value={mismatch.itemId || '未返回'} />{mismatch.title ? <> · <SensitiveValue kind="item-title" value={mismatch.title} /></> : ''}
                              </div>
                            ))}
                          </div>
                          <div className="mt-1">当前配置商品 ID：<SensitiveValue kind="order-id" value={configuredItemIds.join('、') || '未返回'} /></div>
                          <div className="mt-1">请新建或修改为对应商品 ID 的自动发货配置，避免订单无法自动发货。</div>
                        </div>
                      ) : null}
                      {groupPoolWarnings.map((warning) => (
                        <div key={warning.poolTag} className="liquid-notice liquid-notice-warning mt-3 rounded-xl px-3 py-2 text-[12px]">
                          卡密库存不足，请补充卡密库存 · 标签「{warning.poolTag}」可用卡密数 {warning.availableCount} / 已配置商品数 {warning.configuredProductCount}
                        </div>
                      ))}
                      {accountExpanded ? (
                        <div className="mt-3 space-y-2 border-t pt-3" style={{ borderColor: 'var(--border-light)' }}>
                          <div className="flex items-center justify-between gap-2 px-1">
                            <span className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>商品与卡密标签关系</span>
                            <span className="text-[11px]" style={{ color: 'var(--text-muted)' }}>共 {group.configuredCount} 项</span>
                          </div>
                          <div className="grid gap-2 md:grid-cols-2">
                          {group.configs.map((config) => {
                            const configRuns = filterRunsForConfiguration(runs, config.id)
                            const runsExpanded = expandedRunConfigs.has(config.id)
                            const configTags = configPoolTags(config)
                            const fixedConfigTags = configFixedPoolTags(config)
                            const configPools = configTags.map((tag) => findDeliveryPoolOption(poolOptions, tag, true)).filter(Boolean)
                            const configWarnings = config.enabled ? configTags.map((tag) => sharedPoolWarnings.get(tag)).filter(Boolean) : []
                            return (
                              <div key={config.id} className="rounded-[12px] border p-3" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}>
                                <div className="flex gap-3">
                                  <DeliverySelectionCheckbox
                                    checked={selectedConfigIds.has(config.id)}
                                    disabled={batchDeleting}
                                    label={`选择${sensitiveHidden ? maskSensitiveText('item-title', config.product?.title || config.itemId) : (config.product?.title || config.itemId)}自动发货配置`}
                                    onChange={(checked) => setSelectedConfigIds((current) => toggleDeliverySelection(current, [config.id], checked))}
                                  />
                                  <div className="h-12 w-12 shrink-0 overflow-hidden rounded-[10px]" style={{ background: 'var(--surface-sunken)' }}>
                                    {config.product?.image ? <img src={config.product.image} alt="" className="h-full w-full object-cover" /> : <Package className="m-3 h-6 w-6" style={{ color: 'var(--text-tertiary)' }} />}
                                  </div>
                                  <div className="min-w-0 flex-1">
                                    <div className="flex flex-wrap items-center gap-1.5">
                                      <SensitiveValue kind="item-title" className="min-w-0 flex-1 truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }} value={config.product?.title || config.itemId} />
                                      <StatusBadge status={config.enabled ? 'info' : 'neutral'} label={config.enabled ? '启用中' : '已停用'} />
                                      <div className="ml-auto flex flex-wrap justify-end gap-1">
                                        <button disabled={batchDeleting} onClick={() => setExpandedRunConfigs((current) => { const next = new Set(current); if (runsExpanded) next.delete(config.id); else next.add(config.id); return next })} className="btn-ghost btn-sm" aria-expanded={runsExpanded}>执行记录{configRuns.length ? `（${configRuns.length}）` : ''}</button>
                                        <button disabled={batchDeleting} onClick={() => refreshStock(config)} className="btn-ghost btn-sm">刷新库存</button>
                                        <button onClick={() => toggleConfigEnabled(config)} disabled={batchDeleting || togglingConfigId === config.id} className="btn-ghost btn-sm disabled:opacity-40">{togglingConfigId === config.id ? '处理中...' : config.enabled ? '停用' : '启用'}</button>
                                        <button disabled={batchDeleting} onClick={() => openEdit(config)} className="btn-ghost btn-sm">编辑</button>
                                        <button disabled={batchDeleting} onClick={() => remove(config)} className="btn-sm" style={{ color: 'var(--danger)' }}>删除</button>
                                      </div>
                                    </div>
                                    <div className="mt-1.5 flex flex-wrap gap-x-4 gap-y-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                                      <span>一次性池：{configTags.length ? configTags.join('、') : '未启用'}</span>
                                      <span>合计可用 {configPools.reduce((sum, pool) => sum + pool.availableCount, 0)} 条</span>
                                      <span>固定池：{fixedConfigTags.length ? fixedConfigTags.join('、') : '未启用'}</span>
                                      <span>固定说明：{config.fixedMessage ? '已开启' : '未开启'}</span>
                                      <span>{formatStock(config.product?.quantity)}</span>
                                    </div>
                                    {configWarnings.map((poolWarning) => (
                                      <div key={poolWarning.poolTag} className="liquid-notice liquid-notice-warning mt-2 rounded-lg px-2.5 py-1.5 text-[11px]">
                                        标签「{poolWarning.poolTag}」卡密库存不足，请补充卡密库存 · 可用卡密数 {poolWarning.availableCount} / 已配置商品数 {poolWarning.configuredProductCount}
                                      </div>
                                    ))}
                                  </div>
                                </div>
                                {runsExpanded ? (
                                  <div className="mt-2 space-y-2 border-t pt-2" style={{ borderColor: 'var(--border-light)' }}>
                                    {!configRuns.length ? <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>暂无执行记录</div> : configRuns.map((run) => {
                                const [tone, label] = runStatus(run.status)
                                return (
                                  <div key={run.id} className="rounded-[12px] border p-3" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}>
                                    <div className="flex flex-wrap items-center justify-between gap-2">
                                      <div className="min-w-0 text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>订单 <SensitiveValue kind="order-id" value={run.orderId} /></div>
                                      <StatusBadge status={tone} label={label} />
                                    </div>
                                    <div className="mt-2 grid gap-2 text-[12px] md:grid-cols-4" style={{ color: 'var(--text-secondary)' }}>
                                      <span>购买数量：{run.purchaseQuantity ?? '未知'}</span>
                                      <span>需卡密：{run.requiredCardCount}</span>
                                      <span>尝试：{run.attemptCount}</span>
                                      <span>更新时间：{formatTime(run.updatedAt)}</span>
                                    </div>
                                    <div className="mt-2 rounded-[10px] px-3 py-2 text-[12px]" style={run.lastErrorMessage ? { background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' } : { background: 'var(--status-success-bg)', color: 'var(--status-success-text)' }}>
                                      {run.lastErrorMessage || (run.status === 'completed' ? '执行完成' : '暂无错误信息')}
                                    </div>
                                    {['retrying', 'manual_required'].includes(run.status) ? (
                                      <div className="mt-2 flex justify-end"><button onClick={() => retryRun(run)} className="btn-ghost btn-sm">重新排队</button></div>
                                    ) : null}
                                  </div>
                                )
                              })}
                                  </div>
                                ) : null}
                              </div>
                            )
                          })}
                          </div>
                        </div>
                      ) : null}
                    </div>
                    )
                  })}
                </div>
              )}
            </div>
          </section>

        </div>
      </div>

      <Modal open={showModal} onClose={saving ? () => undefined : closeModal} title={editing ? '编辑自动发货配置' : '新增自动发货配置'}>
        <div className={useMobilePresentation ? 'space-y-3 py-1' : 'space-y-4 py-2'}>
          {!editing ? (
            <div className={useMobilePresentation ? 'mobile-soft-card p-3.5' : 'rounded-[16px] border p-4'} style={useMobilePresentation ? undefined : { borderColor: 'var(--border-light)' }}>
              <span className="label">选择账号</span>
              <DeliveryAccountMultiSelect
                accounts={activeAccounts}
                selectedUids={selectedCreateUids}
                onChange={changeCreateAccounts}
                disabled={saving}
              />
              <p className="mt-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>可同时选择多个账号；商品和库存按账号独立加载，取消账号会清理该账号的商品选择。</p>
            </div>
          ) : null}
          <div className={useMobilePresentation ? 'mobile-soft-card p-3.5 space-y-3' : 'rounded-[16px] border p-4 space-y-3'} style={useMobilePresentation ? undefined : { borderColor: 'var(--border-light)' }}>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>发货商品</div>
              <span className="rounded-full px-2.5 py-1 text-[11px]" style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>
                {editing ? selectedAccountLabel : `${selectedCreateAccounts.length} 个账号 · 已选 ${selectedCreateItemCount} 个商品`}
              </span>
            </div>
            {editing ? (
              <div className="flex items-center gap-3 rounded-[14px] border p-3" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}>
                <div className="h-12 w-12 shrink-0 overflow-hidden rounded-[10px]" style={{ background: 'var(--surface-sunken)' }}>
                  {editing.product?.image ? <img src={editing.product.image} alt="" className="h-full w-full object-cover" /> : <Package className="m-3 h-6 w-6" style={{ color: 'var(--text-tertiary)' }} />}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{editing.product?.title || editing.itemId}</div>
                  <div className="mt-1 flex flex-wrap gap-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                    <span>{editing.product?.price || '价格 —'}</span>
                    <span>{formatStock(editing.product?.quantity)}</span>
                    <span>一次性池 {configPoolTags(editing).length} 个</span>
                  </div>
                </div>
              </div>
            ) : (
              <DeliveryProductAccountGroups
                accounts={selectedCreateAccounts}
                statesByUid={createProductStatesByUid}
                selectedItemIdsByUid={selectedCreateItemIdsByUid}
                configurations={configs}
                disabled={saving}
                onRefresh={(uid) => { void loadCreateAccountItems(uid) }}
                onToggleItem={(uid, itemId) => setSelectedCreateItemIdsByUid((current) => (
                  toggleDeliveryItemSelection(current, uid, itemId)
                ))}
              />
            )}
            {editing && form.itemId ? (
              <div className="flex flex-wrap items-center justify-between gap-2 rounded-[12px] border px-3 py-2 text-[12px]" style={{ borderColor: 'var(--border-light)', color: 'var(--text-secondary)' }}>
                <span>当前商品库存：{formatStock((selectedProduct || editing?.product)?.quantity)}</span>
              </div>
            ) : null}
            {editing && !itemsLoading && stockSyncFailures > 0 ? (
              <p className="liquid-notice liquid-notice-warning rounded-2xl px-4 py-3 text-[13px]">
                有 {stockSyncFailures} 个商品暂时没能同步到真实库存，已从可选列表排除。点击“刷新商品和库存”可以重试。
              </p>
            ) : null}
          </div>

          <div className={useMobilePresentation ? 'mobile-soft-card p-3.5 space-y-3' : 'rounded-[16px] border p-4 space-y-3'} style={useMobilePresentation ? undefined : { borderColor: 'var(--border-light)' }}>
            <div className="flex flex-wrap items-center justify-between gap-2" style={{ opacity: validFixedPoolTags.length ? 0.55 : 1 }}>
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>一次性卡密池</div>
              <span className="rounded-full px-2.5 py-1 text-[11px]" style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>
                已选 {validPoolTags.length} 个 · 合计可用 {selectedPoolsAvailable} 条
              </span>
            </div>
            <div className="grid max-h-[240px] gap-2 overflow-y-auto sm:grid-cols-2">
              {selectablePoolOptions.map((option) => {
                const checked = validPoolTags.includes(option.tag)
                return <label key={option.tag} className="flex cursor-pointer items-center gap-3 rounded-xl border px-3 py-2.5" style={{ borderColor: checked ? 'var(--primary)' : 'var(--border-light)', background: checked ? 'var(--primary-light)' : 'var(--surface-soft)', opacity: validFixedPoolTags.length && !checked ? 0.6 : 1 }}><input type="checkbox" checked={checked} onChange={() => setForm((current) => ({ ...current, poolPreference: true, fixedPoolTags: [], poolTags: checked ? current.poolTags.filter((tag) => tag !== option.tag) : [...current.poolTags, option.tag] }))} style={{ accentColor: 'var(--primary)' }} /><span className="min-w-0 flex-1"><span className="block truncate text-[13px] font-semibold">{option.tag}</span><span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>可用 {option.availableCount} / 总数 {option.totalCount}</span></span></label>
              })}
            </div>
            {validFixedPoolTags.length ? <p className="text-[11px]" style={{ color: 'var(--text-muted)' }}>当前已选择固定卡密池；选择一次性池会清空固定池选择。</p> : null}

            {poolsError ? (
              <p className="liquid-notice liquid-notice-danger rounded-2xl px-4 py-3 text-[13px]">{poolsError}</p>
            ) : !poolsLoading && selectablePoolOptions.length === 0 ? (
              <p className="liquid-notice liquid-notice-warning rounded-2xl px-4 py-3 text-[13px]">
                没有可用的一次性卡密池，可改用下方固定说明。
              </p>
            ) : null}

            <div className="flex flex-wrap items-center justify-between gap-2 border-t pt-3" style={{ borderColor: 'var(--border-light)', opacity: validPoolTags.length ? 0.55 : 1 }}>
              <div><div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>固定卡密池</div><div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>每单重复发送，不消耗。</div></div>
              <span className="rounded-full px-2.5 py-1 text-[11px]" style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>已选 {validFixedPoolTags.length} 个</span>
            </div>
            <div className="grid max-h-[200px] gap-2 overflow-y-auto sm:grid-cols-2">
              {selectableFixedPoolOptions.map((option) => {
                const checked = validFixedPoolTags.includes(option.tag)
                return <label key={option.tag} className="flex cursor-pointer items-center gap-3 rounded-xl border px-3 py-2.5" style={{ borderColor: checked ? 'var(--success)' : 'var(--border-light)', background: checked ? 'var(--status-success-bg)' : 'var(--surface-soft)', opacity: validPoolTags.length && !checked ? 0.6 : 1 }}><input type="checkbox" checked={checked} onChange={() => setForm((current) => ({ ...current, poolPreference: false, poolTags: [], fixedPoolTags: checked ? current.fixedPoolTags.filter((tag) => tag !== option.tag) : [...current.fixedPoolTags, option.tag] }))} style={{ accentColor: 'var(--success)' }} /><span className="min-w-0 flex-1"><span className="block truncate text-[13px] font-semibold">{option.tag}</span><span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>固定内容 {option.totalCount} 条</span></span></label>
              })}
              {!poolsLoading && !selectableFixedPoolOptions.length ? <div className="text-[12px]" style={{ color: 'var(--text-muted)' }}>暂无固定卡密池</div> : null}
            </div>
            {validPoolTags.length ? <p className="text-[11px]" style={{ color: 'var(--text-muted)' }}>当前已选择一次性卡密池；选择固定池会清空一次性池选择。</p> : null}

            <label className="flex cursor-pointer items-center gap-3 rounded-xl border px-3 py-2.5" style={{ borderColor: 'var(--border-light)' }}><input type="checkbox" checked={form.fixedEnabled} onChange={(event) => setForm((current) => ({ ...current, fixedEnabled: event.target.checked, fixedMessage: event.target.checked ? current.fixedMessage : '' }))} style={{ accentColor: 'var(--primary)' }} /><span className="text-[13px] font-semibold">固定说明</span></label>
            {form.fixedEnabled ? <textarea className="input min-h-[96px]" value={form.fixedMessage} onChange={(event) => setForm((current) => ({ ...current, fixedMessage: event.target.value }))} placeholder={'您好，你的卡密如下，请及时使用\nXXXX-XXXXX-XXXX-'} /> : null}

            <label className="flex items-center gap-3 rounded-2xl border px-4 py-3 text-[13px] cursor-pointer" style={{ borderColor: 'var(--border-light)' }}>
              <input
                type="checkbox"
                checked={form.enabled}
                onChange={(event) => setForm((prev) => ({ ...prev, enabled: event.target.checked }))}
                className="h-4 w-4 rounded"
                style={{ accentColor: 'var(--primary)' }}
              />
              保存后启用这条配置
            </label>
          </div>

          {error ? <p className="liquid-notice liquid-notice-danger rounded-2xl px-4 py-3 text-[13px]">{error}</p> : null}
          <div className="flex items-center justify-end gap-2 md:gap-3 pt-2">
            <button onClick={closeModal} disabled={saving} className="btn-secondary btn-sm flex-1 md:flex-none disabled:opacity-40">取消</button>
            <button onClick={submit} disabled={saving} className="btn-primary btn-sm disabled:opacity-40 flex-1 md:flex-none">{saving ? '保存中…' : '保存配置'}</button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
