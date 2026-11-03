import { type MutableRefObject, useEffect, useMemo, useRef, useState } from 'react'
import { useAppDialog } from '../../../components/AppDialog'
import { useSafeNotify } from '../../../components/Notify'
import { getAuthToken } from '../../../api/client'
import {
  getCoinActivityUserInfo,
  claimCoinTask,
  completeCoinViewTasks,
  completeSafeViewTasks,
  createExposureOrder,
  createPromotion,
  getCoinSigninHome,
  getCoinUserDetailList,
  getCoinTasks,
  getExposureDetails,
  getExposureAmountOptions,
  getExposureHome,
  getExposureHistory,
  getExposureItemRendering,
  getExposureTasks,
  getPromotionDetail,
  getPromotionPage,
  getPromotionView,
  runExposureTask,
  mtopRetText,
  mtopSucceeded,
  receiveExposureReward,
  signInCoin,
  unwrapMtopData,
} from '../../../api/shopGrowth'
import { getItems } from '../../../api/market'
import { useAccounts } from '../../../hooks/useAccounts'
import type { Account, Item } from '../../../api/types'
import { emitActionFailure, emitActionLog, emitActionStarted } from '../../../bootstrap/actionLogging'
import {
  asRecord,
  canClaimCoinTask,
  canReceiveExposureTask,
  coinTaskClaimPayloadFromTask,
  freshEmptyState,
  getAllCoinTaskList,
  getCoinBalance,
  getCoinDetailList,
  getCoinTaskList,
  getCoinTaskCompletionRowsFromDetails,
  getExposureTaskList,
  getVerifiedExposureTaskList,
  getExposureHistoryList,
  getExposureAmountList,
  getExposureOrderList,
  getExposeableItemList,
  getDefaultPackage,
  getNestedList,
  getPromotionRecordList,
  getPromotionPackages,
  hasFishShopFromWorkbench,
  isCompletableCoinTask,
  isCompletableExposureTask,
  isClaimedCoinTask,
  isClaimedExposureTask,
  needsExposureTaskBatchComplete,
  needsCoinTaskBatchComplete,
  pickNumber,
  pickString,
  receiveRewardPayloadFromTask,
  type JsonRecord,
  type PromotionState,
} from '../promotionUtils'

type ExposureDetailQuery = {
  instanceId?: string
  bizCode?: string
  itemId?: string
  taskInstanceId?: string
  date?: string
}

type GrowthLoadOptions = {
  bypassCache?: boolean
  silent?: boolean
}

type PromotionPackageQueryOptions = {
  silent?: boolean
  background?: boolean
}

type GrowthWriteOptions = {
  confirm?: boolean
  successMessage?: string | ((result: unknown) => string)
}

const PRIMARY_LOAD_TIMEOUT_MS = 4_500
const SECONDARY_LOAD_TIMEOUT_MS = 8_000
const TASK_LOAD_TIMEOUT_MS = 12_000
const DETAIL_LOAD_TIMEOUT_MS = 4_000
const PROMOTION_RECORD_RETRY_TIMEOUT_MS = 12_000
const PENDING_PROMOTION_STATUS = '已创建，等待远端同步'
const MAX_GROWTH_READS = 3

function mergeWorkbenchData(primary: JsonRecord | null, fallback: JsonRecord | null): JsonRecord | null {
  if (primary && fallback) {
    return {
      ...fallback,
      ...primary,
      taskCenter: {
        ...asRecord(fallback.taskCenter),
        ...asRecord(primary.taskCenter),
      },
    }
  }
  return primary ?? fallback
}

function isCoinSigninRecordSigned(record: JsonRecord | null | undefined) {
  if (!record) return false
  return ['signedIn', 'todaySignIn', 'signInAwarded', 'signIn'].some((key) => {
    const value = record[key]
    return value === true || value === 1 || String(value ?? '').toLowerCase() === 'true'
  })
}

function firstNumericSigninDays(record: JsonRecord | null | undefined) {
  if (!record) return null
  for (const key of ['signInDays', 'alreadySignInDays', 'signDays', 'continuousDays', 'days']) {
    const value = Number(record[key])
    if (Number.isFinite(value)) return { key, value }
  }
  return null
}

function markSigninRecordSigned(record: JsonRecord | null | undefined, incrementDays: boolean): JsonRecord {
  const next: JsonRecord = { ...(record ?? {}) }
  const days = firstNumericSigninDays(next)
  next.signedIn = true
  next.todaySignIn = true
  next.signInAwarded = true
  next.signIn = true
  if (days) {
    next[days.key] = incrementDays ? days.value + 1 : days.value
  } else {
    next.alreadySignInDays = 1
  }
  return next
}

function applySigninDays(record: JsonRecord, days: ReturnType<typeof firstNumericSigninDays>): JsonRecord {
  if (!days) return record
  const next = { ...record }
  const currentDays = firstNumericSigninDays(next)
  next[days.key] = currentDays ? Math.max(currentDays.value, days.value) : days.value
  return next
}

function extractPromotionItems(payload: unknown): Item[] {
  const result = asRecord(payload)
  const data = Array.isArray(result.data) ? result.data : asRecord(result.data)
  const candidates = [
    result.items,
    Array.isArray(data) ? data : data.itemList,
    Array.isArray(data) ? [] : data.items,
    Array.isArray(data) ? [] : data.list,
  ]
  const list = candidates.find(Array.isArray)
  return (list ?? []) as Item[]
}

function shouldForceSyncPromotionItems(payload: unknown, items: Item[], force: boolean) {
  if (force || items.length > 0) return false
  const result = asRecord(payload)
  const lastSyncedAt = Number(result._last_synced_at ?? 0)
  const lastSyncOk = result._last_sync_ok
  return lastSyncedAt <= 0 || lastSyncOk === false
}

function promotionRecordKey(record: JsonRecord) {
  const id = pickString(record, ['promotionId', 'id', 'activityId'], '')
  if (id) return `id:${id}`
  return [
    'item',
    pickString(record, ['itemId', 'item_id'], ''),
    pickString(record, ['promotionNum', 'aimedPromoteNum'], ''),
    pickString(record, ['costCoinNum', 'cost', 'payCoin', 'nowPrice'], ''),
  ].join(':')
}

function isSamePromotionRecord(a: JsonRecord, b: JsonRecord) {
  const aId = pickString(a, ['promotionId', 'id', 'activityId'], '')
  const bId = pickString(b, ['promotionId', 'id', 'activityId'], '')
  if (aId && bId && aId === bId) return true
  const aItemId = pickString(a, ['itemId', 'item_id'], '')
  const bItemId = pickString(b, ['itemId', 'item_id'], '')
  if (!aItemId || aItemId !== bItemId) return false
  const aPromotionNum = pickString(a, ['promotionNum', 'aimedPromoteNum'], '')
  const bPromotionNum = pickString(b, ['promotionNum', 'aimedPromoteNum'], '')
  const aCost = pickString(a, ['costCoinNum', 'cost', 'payCoin', 'nowPrice'], '')
  const bCost = pickString(b, ['costCoinNum', 'cost', 'payCoin', 'nowPrice'], '')
  return !!aPromotionNum && aPromotionNum === bPromotionNum && (!aCost || !bCost || aCost === bCost)
}

function mergePromotionRecords(remoteRecords: JsonRecord[], pendingRecords: JsonRecord[]) {
  if (!pendingRecords.length) return remoteRecords
  const merged = [...remoteRecords]
  for (const pending of pendingRecords) {
    if (!merged.some((record) => isSamePromotionRecord(record, pending))) {
      merged.unshift(pending)
    }
  }
  const seen = new Set<string>()
  return merged.filter((record) => {
    const key = promotionRecordKey(record)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

export function usePromotionPageState() {
  const { activeAccounts, loading: accountsLoading, loadAccounts } = useAccounts()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const [selectedUid, setSelectedUid] = useState('')
  const [loading, setLoading] = useState(false)
  const [busyKeys, setBusyKeys] = useState<string[]>([])
  const [state, setState] = useState(freshEmptyState)
  const [itemId, setItemId] = useState('')
  const [promotionView, setPromotionView] = useState<JsonRecord | null>(null)
  const [selectedPromotionNum, setSelectedPromotionNum] = useState('')
  const [promotionDetail, setPromotionDetail] = useState<JsonRecord | null>(null)
  const [authToken, setAuthToken] = useState(() => getAuthToken())
  const [exposureDetailsLoading, setExposureDetailsLoading] = useState(false)
  const [exposureDetailSourceLabel, setExposureDetailSourceLabel] = useState('')
  const [promotionItems, setPromotionItems] = useState<Item[]>([])
  const [promotionItemsLoading, setPromotionItemsLoading] = useState(false)
  const [promotionItemViews, setPromotionItemViews] = useState<Record<string, JsonRecord | null>>({})
  const [promotionItemPackageLoading, setPromotionItemPackageLoading] = useState<Record<string, boolean>>({})
  const [promotionItemSelectedNums, setPromotionItemSelectedNums] = useState<Record<string, string>>({})

  const selectedAccount = useMemo(
    () => activeAccounts.find((account: Account) => account.uid === selectedUid),
    [activeAccounts, selectedUid],
  )
  const activeAccountKey = useMemo(
    () => activeAccounts.map((account: Account) => account.uid).join('|'),
    [activeAccounts],
  )
  const firstActiveUid = activeAccounts[0]?.uid ?? ''
  const selectedUidStillActive = useMemo(
    () => (selectedUid ? activeAccounts.some((account: Account) => account.uid === selectedUid) : false),
    [activeAccountKey, selectedUid],
  )
  const activeAccountsRef = useRef(activeAccounts)
  const activeAccountKeyRef = useRef(activeAccountKey)
  const selectedUidRef = useRef(selectedUid)
  const latestLoadIdRef = useRef(0)
  const growthCacheRef = useRef<Map<string, { state: PromotionState; timestamp: number }>>(new Map())
  const pendingPromotionRecordsRef = useRef<Map<string, JsonRecord[]>>(new Map())
  const retainedClaimedCoinTasksRef = useRef<Map<string, JsonRecord[]>>(new Map())
  const retainedClaimedExposureTasksRef = useRef<Map<string, JsonRecord[]>>(new Map())
  const autoPromotionPackageAttemptedRef = useRef<Set<string>>(new Set())
  const autoPromotionPackageQueueRef = useRef(Promise.resolve())
  const growthReadInFlightRef = useRef(0)
  const growthReadQueueRef = useRef<(() => void)[]>([])
  activeAccountsRef.current = activeAccounts
  activeAccountKeyRef.current = activeAccountKey
  selectedUidRef.current = selectedUid

  function beginBusy(label: string) {
    setBusyKeys((current) => current.includes(label) ? current : [...current, label])
  }

  function endBusy(label: string) {
    setBusyKeys((current) => current.filter((item) => item !== label))
  }

  async function acquireGrowthReadSlot() {
    if (growthReadInFlightRef.current >= MAX_GROWTH_READS) {
      await new Promise<void>((resolve) => {
        growthReadQueueRef.current.push(resolve)
      })
    }
    growthReadInFlightRef.current += 1
    return () => {
      growthReadInFlightRef.current = Math.max(0, growthReadInFlightRef.current - 1)
      const next = growthReadQueueRef.current.shift()
      if (next) next()
    }
  }

  async function runGrowthRead<T>(action: () => Promise<T>) {
    const release = await acquireGrowthReadSlot()
    try {
      return await action()
    } finally {
      release()
    }
  }

  function updateGrowthCache(uid: string, nextState: PromotionState) {
    growthCacheRef.current.set(uid, { state: nextState, timestamp: Date.now() })
  }

  function commitGrowthState(uid: string, updater: (previous: PromotionState) => PromotionState) {
    setState((previous) => {
      const next = updater(previous)
      updateGrowthCache(uid, next)
      return next
    })
  }

  function isPromotionStateEmpty(current: PromotionState) {
    return (
      current.signin === null
      && current.coinActivityUserInfo === null
      && current.coinChallengeInfo === null
      && current.coinSubscribeInfo === null
      && current.workbench === null
      && current.coinTasks.length === 0
      && current.coinUserDetailList.length === 0
      && current.coinVirtualCards.length === 0
      && current.promotions.length === 0
      && current.exposureTasks.length === 0
      && current.exposureExecutableTasks.length === 0
      && current.exposureHome === null
      && current.exposureAmountOptions === null
      && current.exposureAmountList.length === 0
      && current.exposureOrders.length === 0
      && current.exposeableItems.length === 0
      && current.exposureHistory.length === 0
      && current.exposureDetails.length === 0
    )
  }

  function clearScopedState() {
    setState((current) => (isPromotionStateEmpty(current) ? current : freshEmptyState()))
    setPromotionView(null)
    setPromotionDetail(null)
    setSelectedPromotionNum('')
    setExposureDetailsLoading(false)
    setExposureDetailSourceLabel('')
    setPromotionItems([])
    setPromotionItemsLoading(false)
    setPromotionItemViews({})
    setPromotionItemPackageLoading({})
    setPromotionItemSelectedNums({})
    autoPromotionPackageAttemptedRef.current.clear()
    retainedClaimedCoinTasksRef.current.clear()
    retainedClaimedExposureTasksRef.current.clear()
    setBusyKeys([])
    setLoading(false)
  }

  function isScopeCurrent(requestToken: string, requestAccountKey: string, uid?: string) {
    if (getAuthToken() !== requestToken) return false
    if (activeAccountKeyRef.current !== requestAccountKey) return false
    if (!uid) return true
    return selectedUidRef.current === uid && activeAccountsRef.current.some((account: Account) => account.uid === uid)
  }

  function isLoadCurrent(loadId: number, requestToken: string, requestAccountKey: string, uid?: string) {
    return latestLoadIdRef.current === loadId && isScopeCurrent(requestToken, requestAccountKey, uid)
  }

  useEffect(() => {
    const syncToken = () => setAuthToken(getAuthToken())
    window.addEventListener('xy:authChanged', syncToken)
    window.addEventListener('storage', syncToken)
    return () => {
      window.removeEventListener('xy:authChanged', syncToken)
      window.removeEventListener('storage', syncToken)
    }
  }, [])

  useEffect(() => {
    clearScopedState()
    setSelectedUid('')
    void loadAccounts({ force: true })
  }, [authToken, loadAccounts])

  useEffect(() => {
    if (!selectedUid && firstActiveUid) {
      clearScopedState()
      setSelectedUid(firstActiveUid)
      return
    }
    if (!firstActiveUid) {
      if (selectedUid) setSelectedUid('')
      clearScopedState()
      return
    }
    if (selectedUid && !selectedUidStillActive) {
      clearScopedState()
      setSelectedUid(firstActiveUid)
    }
  }, [activeAccountKey, firstActiveUid, selectedUid, selectedUidStillActive])

  useEffect(() => {
    if (!selectedUid || !activeAccounts.some((account: Account) => account.uid === selectedUid)) {
      clearScopedState()
      return
    }
    clearScopedState()
    void loadGrowthData(selectedUid)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedUid, activeAccountKey])

  useEffect(() => {
    if (!selectedUid || promotionItemsLoading || promotionItems.length === 0) return
    const pendingItems: Item[] = []
    for (const item of promotionItems.slice(0, 8)) {
      const targetItemId = resolvePromotionItemId(item)
      if (!targetItemId) continue
      if (Object.prototype.hasOwnProperty.call(promotionItemViews, targetItemId)) continue
      if (promotionItemPackageLoading[targetItemId]) continue
      const attemptKey = `${selectedUid}:${targetItemId}`
      if (autoPromotionPackageAttemptedRef.current.has(attemptKey)) continue
      autoPromotionPackageAttemptedRef.current.add(attemptKey)
      pendingItems.push(item)
    }
    if (pendingItems.length === 0) return
    autoPromotionPackageQueueRef.current = autoPromotionPackageQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        for (const item of pendingItems) {
          await queryPromotionPackageForItem(item, { silent: true, background: true })
        }
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedUid, promotionItems, promotionItemsLoading, promotionItemViews, promotionItemPackageLoading])

  const CACHE_TTL_MS = 30_000

  function primaryQueryOptions(timeoutMessage: string) {
    return {
      timeoutMs: PRIMARY_LOAD_TIMEOUT_MS,
      timeoutMessage,
    }
  }

  function secondaryQueryOptions(timeoutMessage: string) {
    return {
      timeoutMs: SECONDARY_LOAD_TIMEOUT_MS,
      timeoutMessage,
    }
  }

  function taskQueryOptions(timeoutMessage: string) {
    return {
      timeoutMs: TASK_LOAD_TIMEOUT_MS,
      timeoutMessage,
    }
  }

  function pendingPromotionRecords(uid: string) {
    return pendingPromotionRecordsRef.current.get(uid) ?? []
  }

  function mergePendingPromotionRecords(uid: string, records: JsonRecord[]) {
    return mergePromotionRecords(records, pendingPromotionRecords(uid))
  }

  function appendPendingPromotionRecord(uid: string, record: JsonRecord) {
    const current = pendingPromotionRecords(uid)
    const next = mergePromotionRecords([record], current)
    pendingPromotionRecordsRef.current.set(uid, next)
    setState((previous) => ({
      ...previous,
      promotions: mergePromotionRecords(previous.promotions, next),
    }))
    const cached = growthCacheRef.current.get(uid)
    if (cached) {
      growthCacheRef.current.set(uid, {
        ...cached,
        state: {
          ...cached.state,
          promotions: mergePromotionRecords(cached.state.promotions, next),
        },
      })
    }
  }

  function coinTaskKey(task: JsonRecord) {
    return pickString(task, ['deliveryId', 'taskId', 'id', 'taskInstanceId'], '')
  }

  function shouldLoadCoinTaskCompletionEvidence(rawTasks: JsonRecord[], displayTasks: JsonRecord[]) {
    if (rawTasks.length <= displayTasks.length) return false
    return rawTasks.some((task) => {
      const materialDTO = asRecord(task.materialDTO)
      const progressDTO = asRecord(task.progressDTO)
      return pickString(materialDTO, ['title'], '') !== ''
        && pickString(progressDTO, ['needTimes', 'maxTimes'], '') !== ''
    })
  }

  function exposureTaskKey(task: JsonRecord) {
    return pickString(task, ['taskInstanceId', 'taskId', 'id', 'benefitId'], '')
  }

  function mergeRetainedClaimedTasks(
    remoteTasks: JsonRecord[],
    retainedTasks: JsonRecord[],
    keyOf: (task: JsonRecord) => string,
  ): JsonRecord[] {
    if (!retainedTasks.length) return remoteTasks
    const retainedEntries: [string, JsonRecord][] = []
    for (const task of retainedTasks) {
      const key = keyOf(task)
      if (key) retainedEntries.push([key, task])
    }
    const retainedByKey = new Map<string, JsonRecord>(retainedEntries)
    const seen = new Set<string>()
    const merged = remoteTasks.map((task) => {
      const key = keyOf(task)
      if (key) seen.add(key)
      return key && retainedByKey.has(key) ? retainedByKey.get(key) ?? task : task
    })
    for (const task of retainedTasks) {
      const key = keyOf(task)
      if (!key || seen.has(key)) continue
      seen.add(key)
      merged.push(task)
    }
    return merged
  }

  function retainedClaimedToday(tasks: JsonRecord[]): JsonRecord[] {
    const today = localDateKey()
    return tasks.filter((task) => pickString(task, ['_claimedDate'], '') === today)
  }

  function rememberRetainedTasks(
    store: MutableRefObject<Map<string, JsonRecord[]>>,
    uid: string,
    tasks: JsonRecord[],
    keyOf: (task: JsonRecord) => string,
  ) {
    if (!tasks.length) return
    const merged = mergeRetainedClaimedTasks(store.current.get(uid) ?? [], tasks, keyOf)
    store.current.set(uid, merged)
  }

  function markCoinTaskClaimed(task: JsonRecord): JsonRecord {
    const progressDTO = asRecord(task.progressDTO)
    const needTimes = pickString(progressDTO, ['needTimes', 'maxTimes'], '')
    const claimedProgress = needTimes ? `${needTimes}/${needTimes}` : pickString(task, ['progressText'], '')
    return {
      ...task,
      _claimedDate: localDateKey(),
      idleTaskStatus: 'RECEIVED',
      taskStatus: 'RECEIVED',
      status: 'RECEIVED',
      state: 'RECEIVED',
      progressDTO: {
        ...progressDTO,
        times: needTimes || progressDTO.times || 1,
      },
      progressText: claimedProgress,
    }
  }

  function markExposureTaskClaimed(task: JsonRecord): JsonRecord {
    const progressVO = asRecord(task.progressVO)
    const needTimes = pickString(progressVO, ['needTimes', 'maxTimes'], '')
    const claimedProgress = needTimes ? `${needTimes}/${needTimes}` : pickString(task, ['progressText'], '')
    return {
      ...task,
      _claimedDate: localDateKey(),
      idleTaskStatus: 'RECEIVED',
      taskStatus: 'RECEIVED',
      status: 'RECEIVED',
      state: 'RECEIVED',
      progressVO: {
        ...progressVO,
        times: needTimes || progressVO.times || 1,
      },
      progressText: claimedProgress,
    }
  }

  function writeRetainedClaimedTasks(
    uid: string,
    coinTasks: JsonRecord[],
    exposureTasks: JsonRecord[],
  ) {
    const claimedCoinTasks = coinTasks.map(markCoinTaskClaimed)
    const claimedExposureTasks = exposureTasks.map(markExposureTaskClaimed)
    rememberRetainedTasks(retainedClaimedCoinTasksRef, uid, claimedCoinTasks, coinTaskKey)
    rememberRetainedTasks(retainedClaimedExposureTasksRef, uid, claimedExposureTasks, exposureTaskKey)
    setState((previous) => ({
      ...previous,
      coinTasks: mergeRetainedClaimedTasks(previous.coinTasks, claimedCoinTasks, coinTaskKey),
      exposureTasks: mergeRetainedClaimedTasks(previous.exposureTasks, claimedExposureTasks, exposureTaskKey),
    }))
  }

  function writeResultSucceeded(result: unknown) {
    if (!mtopSucceeded(result)) return false
    const data = asRecord(unwrapMtopData(result))
    return data.success !== false
  }

  function buildPendingPromotionRecord(
    itemOrId: Item | JsonRecord | string,
    targetItemId: string,
    promotionNum: string | undefined,
    costCoinNum: number,
    result: unknown,
  ): JsonRecord {
    const itemRecord = typeof itemOrId === 'string' ? {} : asRecord(itemOrId)
    const resultData = asRecord(asRecord(result).data)
    const promotionId = pickString(
      resultData,
      ['promotionId', 'id', 'activityId'],
      `pending-${targetItemId}-${promotionNum ?? 'current'}`,
    )
    return {
      promotionId,
      itemId: targetItemId,
      itemTitle: pickString(itemRecord, ['title', 'itemTitle'], targetItemId),
      costCoinNum,
      promotionNum: promotionNum ?? '',
      status: PENDING_PROMOTION_STATUS,
      createTime: Date.now(),
      pendingLocal: true,
    }
  }

  async function loadGrowthData(uid = selectedUid, options: boolean | GrowthLoadOptions = false) {
    const loadOptions = typeof options === 'boolean' ? { bypassCache: options } : options
    const bypassCache = loadOptions.bypassCache ?? false
    const silent = loadOptions.silent ?? false
    if (!uid) return
    if (!activeAccountsRef.current.some((account: Account) => account.uid === uid)) {
      clearScopedState()
      return
    }
    void loadPromotionItems(uid, bypassCache)

    // 缓存检查：30 秒内重复调用直接返回缓存数据
    if (!bypassCache) {
      const cached = growthCacheRef.current.get(uid)
      if (cached && Date.now() - cached.timestamp < CACHE_TTL_MS) {
        setState(cached.state)
        if (!silent) setLoading(false)
        return
      }
    }

    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    const loadId = ++latestLoadIdRef.current
    if (!silent) setLoading(true)
    if (!silent) {
      setState((previous) => ({
        ...previous,
        coinTasksStatus: 'loading',
        exposureTasksStatus: 'loading',
      }))
    }

    try {
      const summarySettled = await Promise.allSettled([
        runGrowthRead(() => getCoinSigninHome(uid, primaryQueryOptions('签到信息加载超时'))),
        runGrowthRead(() => getCoinActivityUserInfo(uid, primaryQueryOptions('闲鱼币信息加载超时'))),
      ])
      if (!isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) return

      const signinSettled = summarySettled[0]
      const coinActivitySettled = summarySettled[1]
      const coinActivityData: JsonRecord | null =
        coinActivitySettled.status === "fulfilled" ? asRecord(unwrapMtopData(coinActivitySettled.value)) : null
      commitGrowthState(uid, (previous) => ({
        ...previous,
        signin: signinSettled.status === "fulfilled" ? asRecord(unwrapMtopData(signinSettled.value)) : previous.signin,
        coinActivityUserInfo: coinActivityData,
        coinChallengeInfo: null,
        coinSubscribeInfo: null,
      }))
      if (!silent && isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) setLoading(false)

      void loadGrowthTasks(uid, requestToken, requestAccountKey, loadId, silent)
      void loadGrowthMetadata(uid, requestToken, requestAccountKey, loadId)
    } catch (error) {
      if (isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) {
        notify(error instanceof Error ? error.message : "加载推广数据失败", "error")
      }
    } finally {
      if (!silent && isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) setLoading(false)
    }
  }

  async function loadGrowthTasks(
    uid: string,
    requestToken: string,
    requestAccountKey: string,
    loadId: number,
    silent: boolean,
  ) {
    try {
      const taskSettled = await Promise.allSettled([
        runGrowthRead(() => getCoinTasks(uid, taskQueryOptions('闲鱼币任务加载超时'))),
        runGrowthRead(() => getExposureTasks(uid, 'ONLY_TASK_LIST', taskQueryOptions('曝光任务加载超时'))),
      ])
      if (!isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) return

      const coinTasksSettled = taskSettled[0]
      const exposureSettled = taskSettled[1]
      const exposureData: JsonRecord | null =
        exposureSettled.status === "fulfilled" ? asRecord(unwrapMtopData(exposureSettled.value)) : null
      const rawCoinTasks = coinTasksSettled.status === "fulfilled" ? getAllCoinTaskList(coinTasksSettled.value) : []
      const coinTasks = coinTasksSettled.status === "fulfilled" ? getCoinTaskList(coinTasksSettled.value) : []
      let coinDetailList: JsonRecord[] = []
      if (
        coinTasksSettled.status === "fulfilled"
        && shouldLoadCoinTaskCompletionEvidence(rawCoinTasks, coinTasks)
        && isLoadCurrent(loadId, requestToken, requestAccountKey, uid)
      ) {
        const detailsSettled = await Promise.allSettled([
          runGrowthRead(() => getCoinUserDetailList(uid, 1, 20, secondaryQueryOptions('闲鱼币明细加载超时'))),
        ])
        if (!isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) return
        if (detailsSettled[0].status === "fulfilled") {
          coinDetailList = getCoinDetailList(detailsSettled[0].value)
        }
      }
      const coinCompletionRows = getCoinTaskCompletionRowsFromDetails(coinDetailList, rawCoinTasks)
      const exposureTasks = exposureSettled.status === "fulfilled" ? getExposureTaskList(exposureSettled.value) : []
      const exposureExecutableTasks = exposureSettled.status === "fulfilled"
        ? getVerifiedExposureTaskList(exposureSettled.value)
        : []
      const selected = activeAccountsRef.current.find((account: Account) => account.uid === uid) as (Account & {
        fish_shop_enabled?: unknown
        fishShopEnabled?: unknown
      }) | undefined
      const accountHasFishShop = isTruthyFlag(selected?.fish_shop_enabled)
        || isTruthyFlag(selected?.fishShopEnabled)

      const retainedCoinTasks = retainedClaimedToday(retainedClaimedCoinTasksRef.current.get(uid) ?? [])
      const retainedExposureTasks = retainedClaimedToday(retainedClaimedExposureTasksRef.current.get(uid) ?? [])
      const displayCoinTasks = coinTasksSettled.status === "fulfilled"
        ? mergeRetainedClaimedTasks(
          mergeRetainedClaimedTasks(coinTasks, coinCompletionRows, coinTaskKey),
          retainedCoinTasks,
          coinTaskKey,
        )
        : coinTasks
      const displayExposureTasks = exposureSettled.status === "fulfilled"
        ? mergeRetainedClaimedTasks(exposureTasks, retainedExposureTasks, exposureTaskKey)
        : exposureTasks

      commitGrowthState(uid, (previous) => {
        const hasFishShop = hasFishShopFromWorkbench(previous.workbench)
          || hasFishShopFromWorkbench(exposureData)
          || !!previous.exposureHome
          || !!previous.exposureAmountOptions
          || previous.exposureAmountList.length > 0
          || exposureTasks.length > 0
          || accountHasFishShop
        return {
          ...previous,
          coinTasks: coinTasksSettled.status === "fulfilled" ? displayCoinTasks : previous.coinTasks,
          coinTasksStatus: coinTasksSettled.status === "fulfilled" ? 'loaded' : (silent ? previous.coinTasksStatus : 'failed'),
          coinUserDetailList: coinDetailList.length > 0 ? coinDetailList : previous.coinUserDetailList,
          exposureTasks: exposureSettled.status === "fulfilled" ? (hasFishShop ? displayExposureTasks : []) : previous.exposureTasks,
          exposureExecutableTasks: exposureSettled.status === "fulfilled" ? (hasFishShop ? exposureExecutableTasks : []) : previous.exposureExecutableTasks,
          exposureTasksStatus: exposureSettled.status === "fulfilled" ? 'loaded' : (silent ? previous.exposureTasksStatus : 'failed'),
          workbench: exposureData ? mergeWorkbenchData(previous.workbench, exposureData) : previous.workbench,
        }
      })
    } catch {
      if (!silent && isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) {
        commitGrowthState(uid, (previous) => ({
          ...previous,
          coinTasksStatus: 'failed',
          exposureTasksStatus: 'failed',
        }))
      }
    }
  }

  async function loadGrowthMetadata(
    uid: string,
    requestToken: string,
    requestAccountKey: string,
    loadId: number,
  ) {
    try {
      const metadataSettled = await Promise.allSettled([
        runGrowthRead(() => getPromotionPage(uid, 1, 20, primaryQueryOptions('推广记录加载超时'))),
        runGrowthRead(() => getExposureHome(uid, primaryQueryOptions('曝光首页加载超时'))),
        runGrowthRead(() => getExposureAmountOptions(uid, primaryQueryOptions('曝光档位加载超时'))),
      ])
      if (!isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) return

      const promotionsSettled = metadataSettled[0]
      const exposureHomeSettled = metadataSettled[1]
      const exposureAmountSettled = metadataSettled[2]
      const exposureHomeData: JsonRecord | null =
        exposureHomeSettled.status === "fulfilled" ? asRecord(unwrapMtopData(exposureHomeSettled.value)) : null
      const exposureAmountData: JsonRecord | null =
        exposureAmountSettled.status === "fulfilled" ? asRecord(unwrapMtopData(exposureAmountSettled.value)) : null

      commitGrowthState(uid, (previous) => ({
        ...previous,
        promotions: promotionsSettled.status === "fulfilled"
          ? mergePendingPromotionRecords(uid, getPromotionRecordList(promotionsSettled.value))
          : previous.promotions,
        exposureHome: exposureHomeData ?? previous.exposureHome,
        exposureAmountOptions: exposureAmountData ?? previous.exposureAmountOptions,
        exposureAmountList: exposureAmountSettled.status === "fulfilled" ? getExposureAmountList(exposureAmountSettled.value) : previous.exposureAmountList,
        exposureOrders: exposureHomeSettled.status === "fulfilled" ? getExposureOrderList(exposureHomeSettled.value) : previous.exposureOrders,
        exposeableItems: exposureHomeSettled.status === "fulfilled" ? getExposeableItemList(exposureHomeSettled.value) : previous.exposeableItems,
      }))
      if (promotionsSettled.status === "rejected") {
        void retryPromotionRecords(uid, requestToken, requestAccountKey, loadId)
      }

    } catch (error) {
      if (isLoadCurrent(loadId, requestToken, requestAccountKey, uid) && error instanceof Error) {
        notify(error.message, "error")
      }
    }
  }

  async function loadCoinDetails(
    uid = selectedUid,
    options: GrowthLoadOptions = {},
  ) {
    if (!uid) return
    if (!activeAccountsRef.current.some((account: Account) => account.uid === uid)) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    try {
      const result = await runGrowthRead(() => getCoinUserDetailList(uid, 1, 20, secondaryQueryOptions('闲鱼币明细加载超时')))
      if (!isScopeCurrent(requestToken, requestAccountKey, uid)) return
      commitGrowthState(uid, (previous) => ({
        ...previous,
        coinUserDetailList: getCoinDetailList(result),
      }))
    } catch (error) {
      if (!options.silent && isScopeCurrent(requestToken, requestAccountKey, uid)) {
        notify(error instanceof Error ? error.message : '闲鱼币明细加载失败', 'error')
      }
    }
  }

  async function loadExposureHistoryDetails(
    uid = selectedUid,
    options: GrowthLoadOptions = {},
  ) {
    if (!uid) return
    if (!activeAccountsRef.current.some((account: Account) => account.uid === uid)) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    try {
      const result = await runGrowthRead(() => getExposureHistory(uid, 1, 20, secondaryQueryOptions('曝光历史加载超时')))
      if (!isScopeCurrent(requestToken, requestAccountKey, uid)) return
      commitGrowthState(uid, (previous) => ({
        ...previous,
        exposureHistory: getExposureHistoryList(result),
      }))
    } catch (error) {
      if (!options.silent && isScopeCurrent(requestToken, requestAccountKey, uid)) {
        notify(error instanceof Error ? error.message : '曝光明细加载失败', 'error')
      }
    }
  }

  async function retryPromotionRecords(
    uid: string,
    requestToken: string,
    requestAccountKey: string,
    loadId: number,
  ) {
    try {
      const result = await runGrowthRead(() => getPromotionPage(
        uid,
        1,
        20,
        {
          timeoutMs: PROMOTION_RECORD_RETRY_TIMEOUT_MS,
          timeoutMessage: '推广记录加载超时',
        },
      ))
      if (!isLoadCurrent(loadId, requestToken, requestAccountKey, uid)) return
      const promotions = mergePendingPromotionRecords(uid, getPromotionRecordList(result))
      setState((previous) => ({
        ...previous,
        promotions,
      }))
      const cached = growthCacheRef.current.get(uid)
      if (cached) {
        growthCacheRef.current.set(uid, {
          ...cached,
          state: {
            ...cached.state,
            promotions,
          },
        })
      }
    } catch {
      // Primary load already recorded the timeout; keep the rest of the growth page usable.
    }
  }

  async function runWrite(
    label: string,
    message: string,
    action: () => Promise<unknown>,
    after?: (result: unknown) => void | Promise<void>,
    options: GrowthWriteOptions = {},
  ) {
    if (options.confirm !== false) {
      const confirmed = await showConfirm(message, { confirmLabel: '确认执行', cancelLabel: '取消' })
      if (!confirmed) {
        emitActionLog({
          module: 'promotions',
          action: 'growth_write_operation',
          outcome: 'cancelled',
          message: '用户取消推广/任务写操作',
          retryable: false,
          promoteToIncident: false,
          context: {
            label,
            uid: selectedUid,
          },
        })
        return
      }
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    beginBusy(label)
    emitActionStarted({
      module: 'promotions',
      action: 'growth_write_operation',
      message: '开始执行推广/任务写操作',
      context: {
        label,
        uid: selectedUid,
      },
    })
    try {
      const result = await action()
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      const ok = mtopSucceeded(result)
      const retText = mtopRetText(result)
      const successMessage = typeof options.successMessage === 'function'
        ? options.successMessage(result)
        : options.successMessage
      notify(ok ? (successMessage || '操作已完成') : (retText || '操作已提交，请查看返回状态'), ok ? 'success' : 'info')
      emitActionLog({
        module: 'promotions',
        action: 'growth_write_operation',
        outcome: ok ? 'success' : 'warn',
        message: ok ? '推广/任务写操作成功' : '推广/任务写操作返回非成功状态',
        detail: ok ? '' : retText,
        retryable: !ok,
        promoteToIncident: false,
        context: {
          label,
          uid: selectedUid,
          ok,
        },
      })
      await after?.(result)
    } catch (error) {
      if (isScopeCurrent(requestToken, requestAccountKey)) {
        emitActionFailure({
          module: 'promotions',
          action: 'growth_write_operation',
          message: '推广/任务写操作失败',
          error,
          context: {
            label,
            uid: selectedUid,
          },
        })
        notify(error instanceof Error ? error.message : '操作失败', 'error')
      }
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) endBusy(label)
    }
  }

  function applyLocalCoinSigninSuccess() {
    setState((previous) => {
      const currentSignin = previous.coinActivityUserInfo ?? previous.signin
      const wasSigned = isCoinSigninRecordSigned(currentSignin)
      const optimistic = markSigninRecordSigned(currentSignin, !wasSigned)
      const optimisticDays = firstNumericSigninDays(optimistic)
      const next = {
        ...previous,
        signin: applySigninDays(markSigninRecordSigned(previous.signin ?? optimistic, false), optimisticDays),
        coinActivityUserInfo: applySigninDays(markSigninRecordSigned(previous.coinActivityUserInfo ?? optimistic, false), optimisticDays),
      }
      return next
    })
  }

  function requireUid() {
    if (!selectedUid) {
      notify('请先选择账号', 'error')
      return ''
    }
    return selectedUid
  }

  function selectUid(uid: string) {
    setSelectedUid(uid)
    clearScopedState()
  }

  function setPromotionItemId(value: string) {
    setItemId(value.trim())
    setPromotionView(null)
    setSelectedPromotionNum('')
  }

  function resolvePromotionItemId(itemOrId: Item | JsonRecord | string) {
    if (typeof itemOrId === 'string') return itemOrId.trim()
    const record = asRecord(itemOrId)
    return String(record.item_id ?? record.itemId ?? record.id ?? '').trim()
  }

  function resolvePromotionItemTitle(itemOrId: Item | JsonRecord | string) {
    if (typeof itemOrId === 'string') return itemOrId.trim()
    const record = asRecord(itemOrId)
    return pickString(record, ['title', 'itemTitle', 'name'], resolvePromotionItemId(itemOrId))
  }

  function resolvePromotionItemImage(itemOrId: Item | JsonRecord | string) {
    if (typeof itemOrId === 'string') return ''
    const record = asRecord(itemOrId)
    return pickString(record, ['picUrl', 'pic_url', 'imageUrl', 'image_url', 'cover', 'pic'], '')
  }

  async function loadPromotionItems(uid = selectedUid, force = false) {
    if (!uid) return
    if (!activeAccountsRef.current.some((account: Account) => account.uid === uid)) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setPromotionItemsLoading(true)
    try {
      let result = await getItems(uid, 1, 20, 'ON_SALE', { force })
      if (!isScopeCurrent(requestToken, requestAccountKey, uid)) return
      let rawItems = extractPromotionItems(result)
      if (shouldForceSyncPromotionItems(result, rawItems, force)) {
        result = await getItems(uid, 1, 20, 'ON_SALE', { force: true })
        if (!isScopeCurrent(requestToken, requestAccountKey, uid)) return
        rawItems = extractPromotionItems(result)
      }
      for (const item of rawItems) {
        const loadedItemId = resolvePromotionItemId(item)
        if (loadedItemId) autoPromotionPackageAttemptedRef.current.delete(`${uid}:${loadedItemId}`)
      }
      setPromotionItems(rawItems.filter((item) => resolvePromotionItemId(item)))
    } catch (error) {
      if (isScopeCurrent(requestToken, requestAccountKey, uid)) {
        notify(error instanceof Error ? error.message : '商品列表加载失败', 'error')
      }
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey, uid)) setPromotionItemsLoading(false)
    }
  }

  function setPromotionItemPromotionNum(itemOrId: Item | JsonRecord | string, value: string) {
    const targetItemId = resolvePromotionItemId(itemOrId)
    if (!targetItemId) return
    setPromotionItemSelectedNums((current) => ({
      ...current,
      [targetItemId]: value,
    }))
  }

  async function queryPromotionPackageForItem(itemOrId: Item | JsonRecord | string, options: PromotionPackageQueryOptions = {}) {
    const uid = requireUid()
    const targetItemId = resolvePromotionItemId(itemOrId)
    if (!uid || !targetItemId) {
      if (!options.silent) notify('这个商品缺少可用的商品 ID', 'error')
      return
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setPromotionItemPackageLoading((current) => ({ ...current, [targetItemId]: true }))
    const busyLabel = `package-${targetItemId}`
    if (!options.background) beginBusy(busyLabel)
    try {
      const result = await runGrowthRead(() => getPromotionView(uid, targetItemId))
      if (!isScopeCurrent(requestToken, requestAccountKey, uid)) return
      const data = asRecord(unwrapMtopData(result))
      const packages = getPromotionPackages(data)
      const firstPromotionNum = String(packages[0]?.promotionNum ?? '')
      setItemId(targetItemId)
      setPromotionView(data)
      setSelectedPromotionNum(firstPromotionNum)
      setPromotionItemViews((current) => ({
        ...current,
        [targetItemId]: data,
      }))
      setPromotionItemSelectedNums((current) => ({
        ...current,
        [targetItemId]: current[targetItemId] || firstPromotionNum,
      }))
      if (!options.silent) {
        notify(packages.length > 0 ? '已查询推广套餐' : '该商品暂未返回可用推广套餐', packages.length > 0 ? 'success' : 'info')
      }
    } catch (error) {
      if (isScopeCurrent(requestToken, requestAccountKey, uid) && options.silent) {
        setPromotionItemViews((current) => (
          Object.prototype.hasOwnProperty.call(current, targetItemId)
            ? current
            : { ...current, [targetItemId]: {} }
        ))
      }
      if (isScopeCurrent(requestToken, requestAccountKey, uid) && !options.silent) {
        notify(error instanceof Error ? error.message : '查询推广套餐失败', 'error')
      }
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey, uid)) {
        if (!options.background) endBusy(busyLabel)
        setPromotionItemPackageLoading((current) => ({ ...current, [targetItemId]: false }))
      }
    }
  }

  async function queryPromotionPackage() {
    const uid = requireUid()
    const targetItemId = itemId.trim()
    if (!uid || !targetItemId) {
      notify('请输入商品 ID', 'error')
      return
    }
    await queryPromotionPackageForItem(targetItemId)
  }

  async function showPromotionDetail(row: JsonRecord) {
    const uid = requireUid()
    const promotionId = pickString(row, ['promotionId', 'id'], '')
    if (!uid) return
    if (!promotionId) {
      notify('这条推广记录没有可用的详情标识', 'error')
      return
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    const busyLabel = `detail-${promotionId}`
    beginBusy(busyLabel)
    try {
      const result = await getPromotionDetail(uid, promotionId)
      if (!isScopeCurrent(requestToken, requestAccountKey, uid)) return
      setPromotionDetail(asRecord(unwrapMtopData(result)))
    } catch (error) {
      if (isScopeCurrent(requestToken, requestAccountKey, uid)) {
        notify(error instanceof Error ? error.message : '加载推广详情失败', 'error')
      }
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey, uid)) endBusy(busyLabel)
    }
  }

  function buildExposureDetailQuery(historyRow: JsonRecord): ExposureDetailQuery | null {
    const containers = [
      historyRow,
      asRecord(historyRow.detail),
      asRecord(historyRow.extra),
      asRecord(historyRow.ext),
      asRecord(historyRow.bizExt),
      asRecord(historyRow.itemInfo),
      asRecord(historyRow.targetInfo),
    ]
    const query: ExposureDetailQuery = {}

    for (const container of containers) {
      if (!query.instanceId) {
        const value = pickString(container, ['instanceId', 'instance_id'], '')
        if (value) query.instanceId = value
      }
      if (!query.bizCode) {
        const value = pickString(container, ['bizCode', 'biz_code'], '')
        if (value) query.bizCode = value
      }
      if (!query.itemId) {
        const value = pickString(container, ['itemId', 'item_id'], '')
        if (value) query.itemId = value
      }
      if (!query.taskInstanceId) {
        const value = pickString(container, ['taskInstanceId', 'task_id', 'taskId'], '')
        if (value) query.taskInstanceId = value
      }
      if (!query.date) {
        const value = pickString(container, ['date', 'ds'], '')
        if (value) query.date = value
      }
    }

    if (query.itemId || query.taskInstanceId || (query.instanceId && query.bizCode)) {
      return query
    }
    return null
  }

  function canQueryExposureDetail(historyRow: JsonRecord) {
    return buildExposureDetailQuery(historyRow) !== null
  }

  async function openExposureDetailFromHistory(historyRow: JsonRecord) {
    const uid = requireUid()
    const query = buildExposureDetailQuery(historyRow)
    if (!uid || !query) return false

    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    const label = pickString(historyRow, ['itemTitle', 'title', 'itemId'], '曝光明细')
    const busyLabel = `exposure-detail-${label}`
    beginBusy(busyLabel)
    setExposureDetailsLoading(true)
    setExposureDetailSourceLabel(label)
    setState((previous) => ({
      ...previous,
      exposureDetails: [],
    }))
    try {
      const result = await getExposureDetails(
        uid,
        {
          ...query,
          page: 1,
          pageSize: 20,
        },
        {
          timeoutMs: DETAIL_LOAD_TIMEOUT_MS,
          timeoutMessage: '曝光明细加载超时',
        },
      )
      if (!isScopeCurrent(requestToken, requestAccountKey, uid)) return false
      setState((previous) => ({
        ...previous,
        exposureDetails: getNestedList(result, ['detailList', 'list', 'items']),
      }))
      return true
    } catch (error) {
      if (isScopeCurrent(requestToken, requestAccountKey, uid)) {
        notify(error instanceof Error ? error.message : '加载曝光明细失败', 'error')
      }
      return false
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey, uid)) {
        endBusy(busyLabel)
        setExposureDetailsLoading(false)
      }
    }
  }

  async function submitPromotion() {
    const uid = requireUid()
    const targetItemId = itemId.trim()
    if (!uid || !targetItemId) {
      notify('请先选择商品', 'error')
      return
    }
    await submitPromotionForItem(targetItemId)
  }

  async function submitPromotionForItem(itemOrId: Item | JsonRecord | string) {
    const uid = requireUid()
    const targetItemId = resolvePromotionItemId(itemOrId)
    const viewData = promotionItemViews[targetItemId] ?? (targetItemId === itemId.trim() ? promotionView : null)
    const selectedNum = promotionItemSelectedNums[targetItemId] ?? (targetItemId === itemId.trim() ? selectedPromotionNum : '')
    const pack = getPromotionPackages(viewData).find((item) => String(item.promotionNum ?? '') === selectedNum)
      ?? getDefaultPackage(viewData)
    const costCoinNum = Number(pack.costCoinNum ?? pack.cost ?? pack.coinNum)
    const promotionNum = pack.promotionNum ? String(pack.promotionNum) : selectedNum || undefined
    const title = resolvePromotionItemTitle(itemOrId)
    if (!uid || !targetItemId || !Number.isFinite(costCoinNum) || costCoinNum <= 0) {
      notify('请先查询有效推广套餐', 'error')
      return
    }
    await runWrite(
      `create-promotion-${targetItemId}`,
      `确认使用 ${costCoinNum} 闲鱼币为商品 ${targetItemId} 创建 ${promotionNum ?? '当前'} 档推广？`,
      () => createPromotion({ uid, itemId: targetItemId, promotionNum, costCoinNum, expectedCostCoinNum: costCoinNum }),
      async (result) => {
        if (mtopSucceeded(result)) {
          appendPendingPromotionRecord(
            uid,
            buildPendingPromotionRecord(itemOrId, targetItemId, promotionNum, costCoinNum, result),
          )
        }
        await loadGrowthData(uid, true)
      },
      { successMessage: `商品「${title || targetItemId}」闲鱼币推广已开启` },
    )
  }

  function exposureQuantityAvailable() {
    const raw = asRecord(state.exposureAmountOptions).hasQuantity
    return !(raw === false || raw === 0 || raw === '0' || raw === 'false')
  }

  function exposureAccountNumber() {
    const amountOptions = asRecord(state.exposureAmountOptions)
    const home = asRecord(state.exposureHome)
    const raw = pickNumber(
      amountOptions,
      ['userAccount', 'availableExposure', 'exposureCoinNum'],
      pickNumber(home, ['exposureCoinNum', 'availableExposureFlow'], ''),
    )
    const normalized = String(raw).replace(/[^\d.-]/g, '').trim()
    if (!normalized || normalized === '-' || normalized === '.' || normalized === '-.') return null
    const value = Number(normalized)
    return Number.isFinite(value) ? value : null
  }

  async function ensureExposureRenderingAllowsCreate(uid: string, targetItemId: string) {
    const rendering = await getExposureItemRendering(
      uid,
      [targetItemId],
      {
        timeoutMs: DETAIL_LOAD_TIMEOUT_MS,
        timeoutMessage: '曝光状态校验超时',
      },
    )
    const renderingData = asRecord(unwrapMtopData(rendering))
    const instances = getNestedList(renderingData, ['itemActivityInstanceVOList'])
    const itemState = instances[0] ?? {}
    if (isTruthyFlag(itemState.isPolishing)) {
      throw new Error('该商品正在曝光中，请稍后查看曝光账本')
    }
  }

  async function submitExposureForItem(itemOrId: Item | JsonRecord | string, exposureNum: number) {
    const uid = requireUid()
    const targetItemId = resolvePromotionItemId(itemOrId)
    const selectedExposureNum = Number(exposureNum)
    if (!uid || !targetItemId || !Number.isFinite(selectedExposureNum) || selectedExposureNum <= 0) {
      notify('请先选择商品和曝光量', 'error')
      return
    }
    if (!exposureQuantityAvailable()) {
      notify('今日曝光已抢完', 'error')
      return
    }
    const available = exposureAccountNumber()
    if (available !== null && available < selectedExposureNum) {
      notify('可用曝光不足', 'error')
      return
    }
    const title = resolvePromotionItemTitle(itemOrId)
    const picUrl = resolvePromotionItemImage(itemOrId)
    await runWrite(
      `create-exposure-${targetItemId}`,
      `确认使用 ${selectedExposureNum} 曝光为商品「${title || targetItemId}」加曝光？`,
      async () => {
        await ensureExposureRenderingAllowsCreate(uid, targetItemId)
        return createExposureOrder({
          uid,
          itemIds: [targetItemId],
          exposureNum: selectedExposureNum,
          expectedExposureNum: selectedExposureNum,
          picUrl,
          title,
        })
      },
      () => loadGrowthData(uid, true),
      { confirm: false, successMessage: `商品「${title || targetItemId}」曝光推广已开启` },
    )
  }

  async function runCoinSignin() {
    const uid = requireUid()
    if (!uid) return
    await runWrite(
      'signin',
      '确认签到领取闲鱼币？',
      () => signInCoin(uid),
      async (result) => {
        if (mtopSucceeded(result)) applyLocalCoinSigninSuccess()
        await loadGrowthData(uid, { bypassCache: true, silent: true })
      },
      { confirm: false, successMessage: '签到成功，闲鱼币状态已更新' },
    )
  }

  async function completeCoinTaskViews(sourceTasks = state.coinTasks) {
    const uid = requireUid()
    if (!uid) return
    await runWrite(
      'coin-safe-view',
      '只会完成已验证可直接完成的闲鱼币浏览任务，并自动领取已就绪奖励。确认继续？',
      () => completeCoinViewTasks(uid),
      async (result) => {
        if (writeResultSucceeded(result)) {
          writeRetainedClaimedTasks(
            uid,
            sourceTasks.filter((task) => !isClaimedCoinTask(task) && (canClaimCoinTask(task) || needsCoinTaskBatchComplete(task))),
            [],
          )
        }
        await loadGrowthData(uid, { bypassCache: true, silent: true })
      },
      { confirm: false, successMessage: '闲鱼币任务已处理，奖励状态已刷新' },
    )
  }

  async function claimCoinTaskReward(task: JsonRecord, index = 0) {
    const uid = requireUid()
    if (!uid) return
    if (canClaimCoinTask(task)) {
      const name = pickString(task, ['taskName', 'name', 'title', 'taskType'], '闲鱼币任务')
      const taskId = pickString(task, ['taskId', 'id', 'deliveryId'], '')
      await runWrite(
        `coin-claim-${taskId || index}`,
        `确认领取「${name}」的闲鱼币奖励？`,
        () => claimCoinTask(coinTaskClaimPayloadFromTask(uid, task)),
        async (result) => {
          if (writeResultSucceeded(result)) writeRetainedClaimedTasks(uid, [task], [])
          await loadGrowthData(uid, { bypassCache: true, silent: true })
        },
        { successMessage: `「${name}」闲鱼币奖励已领取` },
      )
      return
    }
    if (needsCoinTaskBatchComplete(task)) {
      await completeCoinTaskViews()
      return
    }
    if (!isCompletableCoinTask(task)) {
      notify('这个闲鱼币任务还没有验证到可直接完成，暂不处理', 'error')
      return
    }
    notify('这个闲鱼币任务暂时没有可直接执行的动作', 'error')
  }

  async function completeExposureViewTasks(sourceTasks = state.exposureTasks) {
    const uid = requireUid()
    if (!uid) return
    await runWrite(
      'safe-view',
      '只会完成已验证可直接完成的鱼小铺查看任务，不会自动开通、发布、关注或扣币。确认继续？',
      () => completeSafeViewTasks(uid),
      async (result) => {
        if (writeResultSucceeded(result)) {
          writeRetainedClaimedTasks(
            uid,
            [],
            sourceTasks.filter((task) => !isClaimedExposureTask(task) && (canReceiveExposureTask(task) || needsExposureTaskBatchComplete(task))),
          )
        }
        await loadGrowthData(uid, { bypassCache: true, silent: true })
      },
      { confirm: false, successMessage: '鱼小铺曝光任务已处理，奖励状态已刷新' },
    )
  }

  async function receiveExposureTaskReward(task: JsonRecord, index = 0) {
    const uid = requireUid()
    if (!uid) return
    if (canReceiveExposureTask(task)) {
      const name = pickString(task, ['taskName', 'name', 'title', 'taskType'], '曝光任务')
      const taskId = pickString(task, ['taskId', 'id'], '')
      await runWrite(
        `exposure-${taskId || index}`,
        `确认领取「${name}」的曝光奖励？`,
        () => receiveExposureReward(receiveRewardPayloadFromTask(uid, task)),
        async (result) => {
          if (writeResultSucceeded(result)) writeRetainedClaimedTasks(uid, [], [task])
          await loadGrowthData(uid, { bypassCache: true, silent: true })
        },
        { successMessage: `「${name}」曝光奖励已领取` },
      )
      return
    }
    if (!isCompletableExposureTask(task)) {
      notify('这个曝光任务还没有验证到可直接完成，暂不处理', 'error')
      return
    }
    if (needsExposureTaskBatchComplete(task)) {
      await completeExposureViewTasks()
      return
    }
    notify('这个曝光任务当前不是可直接同步的状态', 'error')
  }

  async function runVerifiedExposureTask(task: JsonRecord, index = 0) {
    const uid = requireUid()
    if (!uid) return
    const name = pickString(task, ['taskName', 'name', 'title', 'taskType'], '曝光任务')
    const taskId = pickString(task, ['taskId', 'id', 'taskInstanceId'], '')
    await runWrite(
      `exposure-run-${taskId || index}`,
      `确认执行「${name}」的高级 H5 任务链路？`,
      () => runExposureTask(receiveRewardPayloadFromTask(uid, task)),
      () => loadGrowthData(uid, true),
      { successMessage: `「${name}」曝光任务已执行` },
    )
  }

  const promotionPackages = getPromotionPackages(promotionView)
  const packageData = promotionPackages.find((item) => String(item.promotionNum ?? '') === selectedPromotionNum)
    ?? getDefaultPackage(promotionView)
  const taskCenter = asRecord(state.workbench?.taskCenter)
  const exposureAmountOptions = asRecord(state.exposureAmountOptions)
  const exposureHome = asRecord(state.exposureHome)
  const exposureHasQuantity = exposureQuantityAvailable()
  const exposureUserAccount = exposureAccountNumber()
  const exposureFlow = pickNumber(
    exposureAmountOptions,
    ['userAccount', 'availableExposure', 'exposureCoinNum'],
    pickNumber(
      exposureHome,
      ['exposureCoinNum', 'availableExposureFlow'],
      pickNumber(taskCenter, ['usableExposureAmount'], pickNumber(state.workbench, ['availableExposureFlow', 'availableFlow', 'flow', 'currentFlow', 'accumulateFlow'], '—')),
    ),
  )
  const selectedFishShopFlag = isTruthyFlag((selectedAccount as Account & {
    fish_shop_enabled?: unknown
    fishShopEnabled?: unknown
  } | undefined)?.fish_shop_enabled)
    || isTruthyFlag((selectedAccount as Account & {
      fish_shop_enabled?: unknown
      fishShopEnabled?: unknown
    } | undefined)?.fishShopEnabled)
  const hasFishShop = hasFishShopFromWorkbench(state.workbench)
    || !!state.exposureHome
    || !!state.exposureAmountOptions
    || state.exposureAmountList.length > 0
    || state.exposureTasks.length > 0
    || selectedFishShopFlag
  const coinBalance = getCoinBalance(state, selectedAccount)

  return {
    activeAccounts,
    accountsLoading,
    selectedAccount,
    selectedUid,
    loading,
    busy: busyKeys[0] ?? '',
    busyKeys,
    growth: {
      ...state,
      hasFishShop,
      coinBalance,
    },
    itemId,
    promotionView,
    selectedPromotionNum,
    promotionDetail,
    exposureDetailsLoading,
    exposureDetailSourceLabel,
    promotionPackages,
    promotionItems,
    promotionItemsLoading,
    promotionItemViews,
    promotionItemPackageLoading,
    promotionItemSelectedNums,
    packageData,
    exposureFlow,
    exposureAmountOptions,
    exposureAmountList: state.exposureAmountList,
    exposureHasQuantity,
    exposureUserAccount,
    canQueryExposureDetail,
    openExposureDetailFromHistory,
    selectUid,
    setPromotionItemId,
    setSelectedPromotionNum,
    setPromotionItemPromotionNum,
    refresh: () => loadGrowthData(undefined, true),
    refreshPromotionItems: () => loadPromotionItems(undefined, true),
    refreshCoinDetails: () => loadCoinDetails(undefined, { silent: false }),
    refreshExposureHistory: () => loadExposureHistoryDetails(undefined, { silent: false }),
    queryPromotionPackage,
    queryPromotionPackageForItem,
    showPromotionDetail,
    submitPromotion,
    submitPromotionForItem,
    submitExposureForItem,
    runCoinSignin,
    completeCoinTaskViews,
    claimCoinTaskReward,
    completeExposureViewTasks,
    receiveExposureTaskReward,
    runVerifiedExposureTask,
  }
}

export type PromotionPageState = ReturnType<typeof usePromotionPageState>

function isTruthyFlag(value: unknown): boolean {
  return value === true || value === 1 || value === '1' || value === 'true'
}

function localDateKey(date = new Date()): string {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}
