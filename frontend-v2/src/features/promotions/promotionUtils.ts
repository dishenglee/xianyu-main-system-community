import { unwrapMtopData } from '../../api/shopGrowth'
import type { Account, ClaimCoinTaskPayload, ReceiveExposureRewardPayload } from '../../api/types'

export type JsonRecord = Record<string, unknown>
export type TaskLoadStatus = 'idle' | 'loading' | 'loaded' | 'failed'

export type PromotionState = {
  signin: JsonRecord | null
  coinActivityUserInfo: JsonRecord | null
  coinChallengeInfo: JsonRecord | null
  coinSubscribeInfo: JsonRecord | null
  coinTasks: JsonRecord[]
  coinTasksStatus: TaskLoadStatus
  coinUserDetailList: JsonRecord[]
  coinVirtualCards: JsonRecord[]
  promotions: JsonRecord[]
  exposureTasks: JsonRecord[]
  exposureExecutableTasks: JsonRecord[]
  exposureTasksStatus: TaskLoadStatus
  exposureHome: JsonRecord | null
  exposureAmountOptions: JsonRecord | null
  exposureAmountList: JsonRecord[]
  exposureOrders: JsonRecord[]
  exposeableItems: JsonRecord[]
  workbench: JsonRecord | null
  exposureHistory: JsonRecord[]
  exposureDetails: JsonRecord[]
}

export type RewardPayload = ClaimCoinTaskPayload & ReceiveExposureRewardPayload & {
  taskInstanceId?: string
  activityId?: string
  benefitList?: unknown[]
}

export function freshEmptyState(): PromotionState {
  return {
    signin: null,
    coinActivityUserInfo: null,
    coinChallengeInfo: null,
    coinSubscribeInfo: null,
    coinTasks: [],
    coinTasksStatus: 'idle',
    coinUserDetailList: [],
    coinVirtualCards: [],
    promotions: [],
    exposureTasks: [],
    exposureExecutableTasks: [],
    exposureTasksStatus: 'idle',
    exposureHome: null,
    exposureAmountOptions: null,
    exposureAmountList: [],
    exposureOrders: [],
    exposeableItems: [],
    workbench: null,
    exposureHistory: [],
    exposureDetails: [],
  }
}

export function asRecord(value: unknown): JsonRecord {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as JsonRecord : {}
}

export function asList(value: unknown): JsonRecord[] {
  if (!Array.isArray(value)) return []
  return value.map(asRecord).filter((item) => Object.keys(item).length > 0)
}

export function pickString(record: JsonRecord | null | undefined, keys: string[], fallback = '—') {
  if (!record) return fallback
  for (const key of keys) {
    const value = record[key]
    if (value !== undefined && value !== null && String(value).trim() !== '') return String(value)
  }
  return fallback
}

export function pickNumber(record: JsonRecord | null, keys: string[], fallback: string | number = '—') {
  if (!record) return fallback
  for (const key of keys) {
    const value = record[key]
    if (value !== undefined && value !== null && String(value).trim() !== '') return String(value)
  }
  return fallback
}


export function deepPickString(record: JsonRecord, keys: string[], fallback = '—') {
  // 先在顶层找
  const top = pickString(record, keys, '')
  if (top) return top
  // 再在常见嵌套路径里找
  const nestedPaths = ['idleTaskBriefVO', 'taskBrief', 'taskInfo', 'benefitVO', 'awardVO', 'extend', 'extendInfo', 'progressVO']
  for (const path of nestedPaths) {
    const nested = asRecord(record[path])
    if (Object.keys(nested).length) {
      const val = pickString(nested, keys, '')
      if (val) return val
    }
  }
  return fallback
}

export function deepPickNumber(record: JsonRecord, keys: string[], fallback: string | number = '—') {
  const top = pickNumber(record, keys, '')
  if (top !== '') return top
  const nestedPaths = ['idleTaskBriefVO', 'taskBrief', 'taskInfo', 'benefitVO', 'awardVO', 'extend', 'extendInfo', 'progressVO']
  for (const path of nestedPaths) {
    const nested = asRecord(record[path])
    if (Object.keys(nested).length) {
      const val = pickNumber(nested, keys, '')
      if (val !== '') return val
    }
  }
  return fallback
}


export function pickAnywhere(record: JsonRecord, keys: string[], fallback = '—'): string {
  const seen = new Set<unknown>()
  const walk = (node: unknown, depth: number): string => {
    if (depth > 12 || seen.has(node)) return ''
    if (!node || typeof node !== 'object') return ''
    seen.add(node)
    if (Array.isArray(node)) {
      for (const item of node) {
        const val = walk(item, depth + 1)
        if (val) return val
      }
      return ''
    }
    const obj = node as JsonRecord
    for (const key of keys) {
      const v = obj[key]
      if (v !== undefined && v !== null) {
        const s = String(v).trim()
        if (s && s !== '0') return s
      }
    }
    for (const child of Object.values(obj)) {
      const val = walk(child, depth + 1)
      if (val) return val
    }
    return ''
  }
  return walk(record, 0) || fallback
}

export function getNestedList(payload: unknown, keys: string[]) {
  const data = asRecord(unwrapMtopData(payload))
  // 1. try direct keys on data
  for (const key of keys) {
    const direct = asList(data[key])
    if (direct.length) return direct
  }
  // 2. try data.result.*
  const result = asRecord(data.result)
  for (const key of keys) {
    const nested = asList(result[key])
    if (nested.length) return nested
  }
  // 3. try known nested paths: data.module.*, data.data.*, etc
  const midPaths = ['module', 'data', 'content', 'body']
  for (const mid of midPaths) {
    const container = asRecord(data[mid])
    if (!Object.keys(container).length) continue
    for (const key of keys) {
      const found = asList(container[key])
      if (found.length) return found
    }
  }
  // 4. try result.module.* etc
  for (const mid of midPaths) {
    const container = asRecord(result[mid])
    if (!Object.keys(container).length) continue
    for (const key of keys) {
      const found = asList(container[key])
      if (found.length) return found
    }
  }
  // 5. recursive walk as last resort
  return collectNestedLists(data, keys)
}

export function getCoinTaskList(payload: unknown) {
  return getAllCoinTaskList(payload).filter(isDisplayableCoinTask)
}

export function getAllCoinTaskList(payload: unknown) {
  return getNestedList(payload, [
    'taskList',
    'tasks',
    'list',
  ]).map(normalizeCoinTask)
}

export function getCoinDetailList(payload: unknown) {
  return getNestedList(payload, ['userScoreDetailDOList', 'list', 'items'])
}

export function getCoinVirtualCardList(payload: unknown) {
  return getNestedList(payload, ['data', 'cardList', 'list', 'items'])
}

function normalizeCoinTask(task: JsonRecord): JsonRecord {
  const materialDTO = asRecord(task.materialDTO)
  const progressDTO = asRecord(task.progressDTO)
  const times = pickString(progressDTO, ['times'], '')
  const needTimes = pickString(progressDTO, ['needTimes', 'maxTimes'], '')

  return {
    ...task,
    title: pickString(materialDTO, ['title'], pickString(task, ['title', 'taskName', 'name'], '')),
    desc: pickString(materialDTO, ['desc'], pickString(task, ['desc', 'description'], '')),
    subTitle: pickString(materialDTO, ['subTitle'], pickString(task, ['subTitle', 'subtitle'], '')),
    taskType: pickString(materialDTO, ['type'], pickString(task, ['taskType', 'type'], '')),
    status: pickString(task, ['idleTaskStatus', 'taskStatus', 'status'], ''),
    rewardCoinNum: pickString(materialDTO, ['desc'], ''),
    progressText: times && needTimes ? `${times}/${needTimes}` : times,
  }
}

const EXPOSURE_TASK_GROUP_KEYS = [
  'learningTaskList',
  'publishTaskList',
  'orderTaskList',
  'serviceTaskList',
  'privateDomainTaskList',
  'widgetTaskList',
  'elseTaskList',
]

const CLAIMABLE_TASK_STATUSES = new Set(['AWARD', 'COMPLETE', 'COMPLETED'])
const COMPLETED_TASK_STATUSES = new Set(['FINISHED', 'FINISH', 'DONE', 'SUCCESS'])
const CLAIMED_TASK_STATUSES = new Set(['RECEIVED', 'CLAIMED', 'AWARDED', 'REWARD_RECEIVED', 'RECEIVE_SUCCESS'])
const BATCH_COMPLETABLE_TASK_STATUSES = new Set(['', 'ACCEPTED', 'INIT', 'PENDING', 'TODO', 'STARTED', 'PROCESSING'])
const TASK_CLAIMED_TEXT = ['已领取', '已领奖', '领取成功', '奖励已领取', '已发放', '已获得']

function getTaskStatus(task: JsonRecord): string {
  return pickString(task, ['idleTaskStatus', 'taskStatus', 'status', 'state'], '').trim().toUpperCase()
}

function getTaskRewardState(task: JsonRecord): string {
  return pickString(task, ['_rewardState', 'rewardState'], '').trim().toLowerCase()
}

function getTaskRewardStatus(task: JsonRecord): string {
  const benefitVO = asRecord(task.benefitVO)
  const rewardList = asList(benefitVO.rewardList)
  const firstReward = rewardList[0] ?? {}
  return pickString(task, ['awardStatus', 'rewardStatus', 'receiveStatus', 'benefitStatus'], '')
    || pickString(benefitVO, ['awardStatus', 'rewardStatus', 'receiveStatus', 'benefitStatus', 'status'], '')
    || pickString(firstReward, ['awardStatus', 'rewardStatus', 'receiveStatus', 'benefitStatus', 'status'], '')
}

function getTaskActionText(task: JsonRecord): string {
  return pickAnywhere(task, [
    'awardBtn',
    'rewardBtn',
    'buttonText',
    'btnText',
    'actionText',
    'statusText',
    'statusDesc',
    'taskStatusDesc',
    'awardStatusDesc',
    'rewardStatusDesc',
    'receiveStatusDesc',
    'benefitStatusDesc',
  ], '')
}

function hasClaimedTaskText(task: JsonRecord): boolean {
  const text = getTaskActionText(task)
  return TASK_CLAIMED_TEXT.some((keyword) => text.includes(keyword))
}

function hasClaimedRewardStatus(task: JsonRecord): boolean {
  const status = getTaskRewardStatus(task).trim()
  return CLAIMED_TASK_STATUSES.has(status.toUpperCase())
    || TASK_CLAIMED_TEXT.some((keyword) => status.includes(keyword))
}

function getCoinTaskAction(task: JsonRecord): string {
  const materialDTO = asRecord(task.materialDTO)
  return pickString(materialDTO, ['action'], pickString(task, ['action', 'url', 'jumpUrl'], ''))
}

function coinTaskAdCode(action: string): string {
  const query = action.split('?')[1] ?? ''
  for (const pair of query.split('&')) {
    const [rawKey, rawValue] = pair.split('=')
    if (!rawKey || !rawValue) continue
    if (rawKey === 'adPositionCode' || rawKey === 'adCode') {
      return safeDecode(rawValue)
    }
  }
  return ''
}

function isSafeCoinAdBrowseUrl(action: string): boolean {
  const decoded = safeDecode(action).toLowerCase()
  if (!coinTaskAdCode(action)) return false
  if (containsAny(decoded, ['alipay', 'cashier', 'payment', 'pay.html', 'buy.html', 'orderconfirm', 'placeorder', 'trade'])) {
    return false
  }
  return containsAny(decoded, ['alimama-ads', '/xianyu/search.html', '/xianyu/like.html'])
}

function hasRiskyCoinTaskSemantics(task: JsonRecord): boolean {
  const materialDTO = asRecord(task.materialDTO)
  const title = pickString(materialDTO, ['title'], pickString(task, ['title', 'taskName', 'name'], '')).toLowerCase()
  const action = safeDecode(getCoinTaskAction(task)).toLowerCase()
  const adCode = coinTaskAdCode(getCoinTaskAction(task)).toLowerCase()
  if (containsAny(title, ['下单', '购买', '支付', '付款', '支付宝', '积分', '开通', '充值'])) return true
  if (containsAny(action, ['alipay', 'cashier', 'payment', 'pay.html', 'buy.html', 'orderconfirm', 'placeorder', 'integral'])) return true
  return containsAny(adCode, ['buy', 'pay', 'cashier', 'order_pay'])
}

function exposureTaskSearchText(task: JsonRecord): string {
  const extend = asRecord(task.extend)
  const materialVO = asRecord(task.materialVO)
  const textParts = [
    pickString(task, ['taskType'], pickString(extend, ['taskType'], '')),
    pickString(materialVO, ['title'], pickString(task, ['title', 'taskName', 'name'], '')),
    pickString(materialVO, ['desc'], pickString(task, ['desc', 'description', 'taskDesc'], '')),
  ]
  return textParts.join(' ').toLowerCase()
}

function hasSafeExposureViewSemantics(task: JsonRecord): boolean {
  return containsAny(exposureTaskSearchText(task), [
    'visit',
    'view',
    'browse',
    'diagnosis',
    'data',
    'xy_coin',
    '浏览',
    '查看',
    '访问',
    '诊断',
    '数据',
    '闲鱼币',
  ])
}

function hasManualRiskExposureSemantics(task: JsonRecord): boolean {
  return containsAny(exposureTaskSearchText(task), [
    'publish',
    'pubilsh',
    'follow',
    'subscribe',
    'open',
    'order',
    'buy',
    'pay',
    'groupon',
    '发布',
    '关注',
    '订阅',
    '开通',
    '订单',
    '下单',
    '购买',
    '支付',
    '拼团',
  ])
}

function containsAny(text: string, needles: string[]): boolean {
  return needles.some((needle) => text.includes(needle))
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}

export function isCompletableCoinTask(task: JsonRecord): boolean {
  const materialDTO = asRecord(task.materialDTO)
  const actionType = pickString(materialDTO, ['actionType'], '')
  const progressDTO = asRecord(task.progressDTO)
  const needTimes = Number(pickString(progressDTO, ['needTimes', 'maxTimes'], '0'))
  return actionType === 'VISIT'
    && needTimes === 1
    && isSafeCoinAdBrowseUrl(getCoinTaskAction(task))
    && !hasRiskyCoinTaskSemantics(task)
}

export function isCompletableExposureTask(task: JsonRecord): boolean {
  return hasSafeExposureViewSemantics(task) && !hasManualRiskExposureSemantics(task)
}

export function isVerifiedExecutableExposureTask(task: JsonRecord): boolean {
  const extend = asRecord(task.extend)
  const taskType = pickString(task, ['taskType'], pickString(extend, ['taskType'], ''))
  return taskType.trim() !== ''
}

export function canClaimCoinTask(task: JsonRecord): boolean {
  if (isCompletedCoinTask(task) || isClaimedCoinTask(task)) return false
  const progressDTO = asRecord(task.progressDTO)
  const status = getTaskStatus(task)
  const times = Number(pickString(progressDTO, ['times'], '0'))
  const needTimes = Number(pickString(progressDTO, ['needTimes', 'maxTimes'], '0'))
  return CLAIMABLE_TASK_STATUSES.has(status) || (needTimes > 0 && times >= needTimes)
}

export function isCompletedCoinTask(task: JsonRecord): boolean {
  return COMPLETED_TASK_STATUSES.has(getTaskStatus(task))
}

export function isClaimedCoinTask(task: JsonRecord): boolean {
  return CLAIMED_TASK_STATUSES.has(getTaskStatus(task))
    || hasClaimedRewardStatus(task)
    || hasClaimedTaskText(task)
}

export function isDisplayableCoinTask(task: JsonRecord): boolean {
  return canClaimCoinTask(task)
    || needsCoinTaskBatchComplete(task)
    || isCompletedCoinTask(task)
    || isClaimedCoinTask(task)
}

export function needsCoinTaskBatchComplete(task: JsonRecord): boolean {
  if (!isCompletableCoinTask(task) || canClaimCoinTask(task)) return false
  const progressDTO = asRecord(task.progressDTO)
  const status = getTaskStatus(task)
  const times = Number(pickString(progressDTO, ['times'], '0'))
  const needTimes = Number(pickString(progressDTO, ['needTimes', 'maxTimes'], '0'))
  return BATCH_COMPLETABLE_TASK_STATUSES.has(status) && needTimes === 1 && times < needTimes
}

export function canReceiveExposureTask(task: JsonRecord): boolean {
  const rewardState = getTaskRewardState(task)
  if (rewardState === 'claimed') return false
  if (isClaimedExposureTask(task)) return false
  if (!isCompletableExposureTask(task)) return false
  if (rewardState === 'claimable') return true
  return CLAIMABLE_TASK_STATUSES.has(getTaskStatus(task))
}

export function isClaimedExposureTask(task: JsonRecord): boolean {
  return getTaskRewardState(task) === 'claimed'
    || CLAIMED_TASK_STATUSES.has(getTaskStatus(task))
    || hasClaimedRewardStatus(task)
    || hasClaimedTaskText(task)
}

export function isCompletedExposureTask(task: JsonRecord): boolean {
  return COMPLETED_TASK_STATUSES.has(getTaskStatus(task))
}

export function isDisplayableExposureTask(task: JsonRecord): boolean {
  return isCompletableExposureTask(task)
    || isCompletedExposureTask(task)
    || isClaimedExposureTask(task)
}

export function needsExposureTaskBatchComplete(task: JsonRecord): boolean {
  if (!isCompletableExposureTask(task) || canReceiveExposureTask(task)) return false
  const status = getTaskStatus(task)
  return status === 'ACCEPTED'
}

export function isSafeBrowsingTask(task: JsonRecord): boolean {
  return isCompletableExposureTask(task)
}

export function getExposureTaskList(payload: unknown) {
  return getVerifiedExposureTaskList(payload).filter(isDisplayableExposureTask)
}

export function getVerifiedExposureTaskList(payload: unknown) {
  const data = asRecord(unwrapMtopData(payload))
  const incentiveTask = asRecord(data.incentiveTask)
  const groupedTasks: JsonRecord[] = []
  const rewardContext = getExposureRewardContext(data)

  for (const groupKey of EXPOSURE_TASK_GROUP_KEYS) {
    const list = asList(incentiveTask[groupKey])
    list.forEach((task) => groupedTasks.push(normalizeExposureTask(task, groupKey, rewardContext)))
  }
  if (groupedTasks.length) return groupedTasks.filter(isVerifiedExecutableExposureTask)

  return getNestedList(payload, [
    'taskList',
    'tasks',
    'list',
    'viewTaskList',
    'taskDetails',
    ...EXPOSURE_TASK_GROUP_KEYS,
  ]).map((task) => normalizeExposureTask(task, '', rewardContext)).filter(isVerifiedExecutableExposureTask)
}

export function getPromotionRecordList(payload: unknown) {
  return getNestedList(payload, [
    'promotionList',
    'records',
    'result',
    'list',
    'items',
  ]).map(normalizePromotionRecord)
}

export function getExposureAmountList(payload: unknown) {
  return getNestedList(payload, [
    'exposureAmountDOList',
    'amountList',
    'amountOptions',
    'options',
    'list',
    'items',
  ]).filter((item) => pickString(item, ['exposureNum', 'amount', 'num'], '') !== '')
}

export function getExposureOrderList(payload: unknown) {
  return getNestedList(payload, [
    'exposureOrderList',
    'orderList',
    'orders',
    'list',
    'items',
  ])
}

export function getExposeableItemList(payload: unknown) {
  return getNestedList(payload, [
    'exposeableItemDOList',
    'exposableItemDOList',
    'itemList',
    'items',
    'list',
  ])
}

export function getExposureHistoryList(payload: unknown) {
  return getNestedList(payload, [
    'exposureHistoryList',
    'historyList',
    'records',
    'result',
    'list',
    'items',
  ]).map(normalizeExposureHistoryRecord)
}

type ExposureRewardContext = {
  hasUnclaimedBenefitList: boolean
  unclaimedKeys: Set<string>
  serverTime: string
}

function getExposureRewardContext(data: JsonRecord): ExposureRewardContext {
  const unclaimedBenefits = asRecord(data.unclaimedBenefits)
  const benefitListRaw = unclaimedBenefits.benefitList
  const benefitList = Array.isArray(benefitListRaw) ? asList(benefitListRaw) : []
  const unclaimedKeys = new Set<string>()
  benefitList.forEach((benefit) => {
    exposureIdentityKeys(benefit).forEach((key) => unclaimedKeys.add(key))
  })
  return {
    hasUnclaimedBenefitList: Array.isArray(benefitListRaw),
    unclaimedKeys,
    serverTime: pickString(data, ['serverTime', 'updateTime'], ''),
  }
}

function exposureIdentityKeys(record: JsonRecord): string[] {
  const benefitVO = asRecord(record.benefitVO)
  const containers = [record, benefitVO]
  const keys: string[] = []
  for (const container of containers) {
    for (const field of ['taskInstanceId', 'taskId', 'id', 'benefitId', 'benefitInstanceId', 'awardId']) {
      const value = pickString(container, [field], '')
      if (value) keys.push(`${field}:${value}`)
    }
    const activityId = pickString(container, ['activityId'], '')
    const taskInstanceId = pickString(container, ['taskInstanceId', 'taskId', 'id'], '')
    if (activityId && taskInstanceId) keys.push(`activity-task:${activityId}:${taskInstanceId}`)
  }
  return Array.from(new Set(keys))
}

function exposureRewardStateFromContext(task: JsonRecord, context: ExposureRewardContext) {
  if (!context.hasUnclaimedBenefitList) return ''
  if (!CLAIMABLE_TASK_STATUSES.has(getTaskStatus(task))) return ''
  const taskKeys = exposureIdentityKeys(task)
  const hasUnclaimedReward = taskKeys.some((key) => context.unclaimedKeys.has(key))
  return hasUnclaimedReward ? 'claimable' : 'claimed'
}

function normalizeExposureTask(task: JsonRecord, group = '', rewardContext?: ExposureRewardContext): JsonRecord {
  const materialVO = asRecord(task.materialVO)
  const extend = asRecord(task.extend)
  const benefitVO = asRecord(task.benefitVO)
  const progressVO = asRecord(task.progressVO)
  const rewardList = asList(benefitVO.rewardList)
  const firstReward = rewardList[0] ?? {}
  const rewardAmount = pickString(
    firstReward,
    ['denomination', 'amount', 'rewardAmount'],
    pickString(extend, ['flowNum', 'awardAmount'], ''),
  )
  const times = pickString(progressVO, ['times'], '')
  const needTimes = pickString(progressVO, ['needTimes', 'maxTimes'], '')

  const normalized: JsonRecord = {
    ...task,
    group,
    title: pickString(materialVO, ['title'], pickString(task, ['title', 'taskName', 'name'], '')),
    desc: pickString(materialVO, ['desc'], pickString(task, ['desc', 'description'], '')),
    taskType: pickString(task, ['taskType'], pickString(extend, ['taskType'], '')),
    awardAmount: rewardAmount,
    progressText: times && needTimes ? `${times}/${needTimes}` : times,
  }
  if (rewardContext) {
    const rewardState = exposureRewardStateFromContext(normalized, rewardContext)
    if (rewardState) normalized._rewardState = rewardState
    if (rewardContext.serverTime) normalized._serverTime = rewardContext.serverTime
  }
  return normalized
}

function normalizePromotionRecord(record: JsonRecord): JsonRecord {
  const itemDTO = asRecord(record.itemDTO)
  const itemVO = asRecord(record.itemVO)
  const promotionDTO = asRecord(record.promotionDTO)
  const promotionInfo = asRecord(record.promotionInfo)
  const promotionDetailVO = asRecord(record.promotionDetailVO)
  const detail = asRecord(record.detail)
  const containers = [record, promotionDTO, promotionInfo, promotionDetailVO, detail]
  const normalized: JsonRecord = { ...record }
  const statusText = firstNestedPresent(containers, ['statusDescTitle', 'statusDesc'])
  const rawStatus = firstNestedPresent(containers, ['status'])

  assignPresent(normalized, 'promotionId', firstNestedPresent(containers, ['promotionId', 'id']))
  assignPresent(normalized, 'itemId', firstNestedPresent([record, itemDTO, itemVO, promotionDetailVO, detail], ['itemId', 'item_id']))
  assignPresent(normalized, 'itemTitle', firstNestedPresent([record, itemDTO, itemVO, promotionDetailVO, detail], ['itemTitle', 'title', 'itemName']))
  assignPresent(normalized, 'costCoinNum', firstNestedPresent(containers, ['costCoinNum', 'cost', 'payCoin', 'coinNum']))
  assignPresent(normalized, 'promotionNum', firstNestedPresent(containers, ['promotionNum', 'aimedPromoteNum', 'exposureNum', 'maxPageView']))
  assignPresent(normalized, 'realPromoteNum', firstNestedPresent(containers, ['realPromoteNum', 'realPromotionNum']))
  assignPresent(normalized, 'promotionUpperNum', firstNestedPresent(containers, ['promotionUpperNum', 'upperPromoteNum']))
  assignPresent(normalized, 'promotionStatus', firstNestedPresent(containers, ['promotionStatus']))
  if (statusText !== undefined && statusText !== null && String(statusText).trim() !== '') {
    assignPresent(normalized, 'status', statusText)
  } else if (rawStatus !== undefined && rawStatus !== null && Number.isNaN(Number(String(rawStatus).trim()))) {
    assignPresent(normalized, 'status', rawStatus)
  }
  assignPresent(normalized, 'gmtCreate', firstNestedPresent(containers, ['gmtCreate', 'createTime', 'gmtCreated', 'orderTime', 'startTime']))
  assignPresent(normalized, 'orderTime', firstNestedPresent(containers, ['orderTime']))
  assignPresent(normalized, 'startTime', firstNestedPresent(containers, ['startTime']))
  assignPresent(normalized, 'endTime', firstNestedPresent(containers, ['endTime']))
  assignPresent(normalized, 'settleTime', firstNestedPresent(containers, ['settleTime']))
  return normalized
}

function normalizeExposureHistoryRecord(record: JsonRecord): JsonRecord {
  const itemInfo = asRecord(record.itemInfo)
  const exposureInfo = asRecord(record.exposureInfo)
  const detail = asRecord(record.detail)
  const extra = asRecord(record.extra)
  const ext = asRecord(record.ext)
  const normalized: JsonRecord = { ...record }

  assignPresent(normalized, 'itemId', firstNestedPresent([record, itemInfo, detail], ['itemId', 'item_id']))
  assignPresent(normalized, 'itemTitle', firstNestedPresent([record, itemInfo, detail], ['itemTitle', 'title', 'itemName']))
  assignPresent(normalized, 'exposureNum', firstNestedPresent([record, exposureInfo, detail], ['exposureNum', 'exposureCnt', 'cnt', 'consumedExposure']))
  assignPresent(normalized, 'date', firstNestedPresent([record, exposureInfo, detail], ['date', 'ds', 'gmtCreate', 'createTime']))
  assignPresent(normalized, 'instanceId', firstNestedPresent([record, detail, extra, ext], ['instanceId', 'instance_id']))
  assignPresent(normalized, 'bizCode', firstNestedPresent([record, detail, extra, ext], ['bizCode', 'biz_code']))
  assignPresent(normalized, 'taskInstanceId', firstNestedPresent([record, detail, extra, ext], ['taskInstanceId', 'taskId', 'task_id']))
  return normalized
}

export function getClaimedExposureRewardHistoryRows(tasks: JsonRecord[]): JsonRecord[] {
  return tasks
    .filter((task) => isClaimedExposureTask(task))
    .map((task) => {
      const rewardAmount = pickString(task, ['awardAmount', 'awardExposureNum', 'exposureNum', 'amount'], '')
      return {
        itemTitle: pickAnywhere(task, ['title', 'taskName', 'name'], '曝光奖励'),
        exposureNum: rewardAmount,
        date: pickString(task, ['_serverTime', 'serverTime', 'updateTime'], '今日已领取'),
        status: '已领取',
        taskInstanceId: pickAnywhere(task, ['taskInstanceId', 'taskId', 'id'], ''),
      }
    })
    .filter((record) => pickString(record, ['itemTitle'], '') !== '')
}

export function getCoinTaskCompletionRowsFromDetails(
  detailRows: JsonRecord[],
  sourceTasks: JsonRecord[],
  todayKey = localDateKey(),
): JsonRecord[] {
  const completionDetails = latestCoinCompletionDetails(detailRows, todayKey)
  if (!completionDetails.length) return []
  const rows: JsonRecord[] = []
  const usedKeys = new Set<string>()
  completionDetails.forEach((detail, index) => {
    const sourceTask = sourceTasks.find((task) => coinDetailMatchesSourceTask(detail, task))
    if (!sourceTask) return
    const key = pickString(sourceTask, ['deliveryId', 'taskId', 'id'], `detail-${index}`)
    if (usedKeys.has(key)) return
    usedKeys.add(key)
    rows.push(markCoinTaskClaimedFromDetail(sourceTask, detail, key))
  })
  return rows
}

function latestCoinCompletionDetails(detailRows: JsonRecord[], todayKey: string): JsonRecord[] {
  const completionDetails = detailRows.filter((detail) => {
    const describe = pickString(detail, ['describe', 'title', 'bizType'], '')
    const score = Number(pickString(detail, ['score', 'amount', 'coinNum'], '0'))
    const date = coinDetailDate(detail)
    return date === todayKey
      && describe.startsWith('完成')
      && describe.includes('奖励')
      && Number.isFinite(score)
      && score > 0
  })
  return completionDetails
}

function coinDetailDate(detail: JsonRecord): string {
  const value = pickString(detail, ['time', 'gmtCreate', 'createTime'], '')
  const match = value.match(/\d{4}-\d{2}-\d{2}/)
  return match?.[0] ?? ''
}

function localDateKey(date = new Date()): string {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function coinDetailMatchesSourceTask(detail: JsonRecord, task: JsonRecord): boolean {
  const describe = pickString(detail, ['describe', 'title', 'bizType'], '').toLowerCase()
  const materialDTO = asRecord(task.materialDTO)
  const title = pickString(task, ['title'], pickString(materialDTO, ['title'], '')).toLowerCase()
  const action = safeDecode(pickString(materialDTO, ['action'], pickString(task, ['action'], ''))).toLowerCase()
  if (describe.includes('国补')) return title.includes('国补') || action.includes('guobu')
  if (describe.includes('全新好物')) return title.includes('全新好物') || action.includes('xybrworder')
  if (describe.includes('搜索')) return title.includes('搜') || action.includes('/search')
  return false
}

function markCoinTaskClaimedFromDetail(sourceTask: JsonRecord, detail: JsonRecord, fallbackKey: string): JsonRecord {
  const progressDTO = asRecord(sourceTask.progressDTO)
  const needTimes = pickString(progressDTO, ['needTimes', 'maxTimes'], '')
  const score = pickString(detail, ['score', 'amount', 'coinNum'], '')
  return {
    ...sourceTask,
    deliveryId: pickString(sourceTask, ['deliveryId', 'taskId', 'id'], fallbackKey),
    idleTaskStatus: 'RECEIVED',
    taskStatus: 'RECEIVED',
    status: 'RECEIVED',
    state: 'RECEIVED',
    awardStatus: '已领取',
    awardCoinNum: score,
    rewardCoinNum: score,
    progressDTO: {
      ...progressDTO,
      times: needTimes || progressDTO.times || 1,
    },
    progressText: needTimes ? `${needTimes}/${needTimes}` : pickString(sourceTask, ['progressText'], ''),
    _completionEvidence: pickString(detail, ['describe'], ''),
    _completionTime: pickString(detail, ['time', 'gmtCreate', 'createTime'], ''),
  }
}

function firstNestedPresent(containers: JsonRecord[], keys: string[]): unknown {
  for (const container of containers) {
    const value = firstPresent(container, keys)
    if (value !== undefined) return value
  }
  return undefined
}

function collectNestedLists(value: unknown, keys: string[]) {
  const output: JsonRecord[] = []
  const walk = (node: unknown) => {
    if (Array.isArray(node)) {
      node.forEach(walk)
      return
    }
    if (!node || typeof node !== 'object') return
    for (const [key, child] of Object.entries(node as JsonRecord)) {
      if (keys.includes(key)) {
        output.push(...asList(child))
      }
      walk(child)
    }
  }
  walk(value)
  return output
}

export function getPromotionPackages(viewData: JsonRecord | null) {
  const data = asRecord(viewData)
  const packages: JsonRecord[] = []
  const priceMap = asRecord(data.promotionPriceMap)
  Object.entries(priceMap).forEach(([promotionNum, raw]) => {
    const record = asRecord(raw)
    if (!Object.keys(record).length) return
    packages.push({
      ...record,
      promotionNum,
      costCoinNum: record.costCoinNum ?? record.nowPrice ?? record.originalPrice,
      exposureNum: record.exposureNum ?? record.maxPageView ?? promotionNum,
    })
  })
  packages.push(...asList(data.packageList))
  packages.push(...asList(data.promotionPackageList))
  packages.push(...asList(data.cardList))
  return packages.filter((item) => Number(item.costCoinNum ?? item.cost ?? item.coinNum ?? item.nowPrice) > 0)
}

export function getCoinBalance(state: PromotionState, account?: Account) {
  return pickNumber(
    state.coinActivityUserInfo ?? state.signin,
    ['myIdleCoin', 'coinNum', 'coinBalance', 'availableCoinNum', 'amount'],
    account?.coin_balance ?? '—',
  )
}

export function hasFishShopFromWorkbench(workbench: JsonRecord | null): boolean {
  if (!workbench) return false
  if (workbench.fishShopEnabled === true) return true
  if (workbench.incentiveTask && typeof workbench.incentiveTask === 'object') return true
  return ['currentFlow', 'accumulateFlow', 'availableExposureFlow', 'availableFlow', 'flow']
    .some((key) => workbench[key] !== undefined && workbench[key] !== null)
}

export function formatPromotionDetail(promotion: JsonRecord): { label: string; value: string }[] {
  const fieldMap: Record<string, string> = {
    itemTitle: '商品标题',
    costCoinNum: '消耗金币',
    status: '状态',
    promotionNum: '推广数量',
    exposureNum: '曝光量',
    createTime: '创建时间',
    endTime: '结束时间',
    promotionId: '推广ID',
    itemId: '商品ID',
    price: '价格',
    description: '描述',
    category: '分类',
    remainCoinNum: '剩余金币',
    totalCoinNum: '总金币',
    startTime: '开始时间',
    itemUrl: '商品链接',
    reason: '原因',
    message: '提示信息',
  }
  return Object.entries(promotion).map(([key, value]) => ({
    label: fieldMap[key] || key,
    value: value !== undefined && value !== null ? String(value) : '—',
  }))
}

export function getDefaultPackage(viewData: JsonRecord | null) {
  const packages = getPromotionPackages(viewData)
  if (packages.length) return packages[0]
  const data = asRecord(viewData)
  const candidates = [
    asRecord(data.defaultPackage),
    asRecord(data.package),
    asRecord(data.selectedPackage),
    ...asList(data.packageList),
    ...asList(data.promotionPackageList),
    data,
  ]
  return candidates.find((item) => Number(item.costCoinNum ?? item.cost ?? item.coinNum) > 0) ?? {}
}

function getTaskActivityId(task: JsonRecord) {
  const benefitVO = asRecord(task.benefitVO)
  return pickString(task, ['activityId'], pickString(benefitVO, ['activityId'], ''))
}

function firstPresent(record: JsonRecord, keys: string[]): unknown {
  for (const key of keys) {
    const value = record[key]
    if (value !== undefined && value !== null && String(value).trim() !== '') return value
  }
  return undefined
}

function assignPresent(target: Record<string, unknown>, key: string, value: unknown) {
  if (value === undefined || value === null || String(value).trim() === '') return
  target[key] = value
}

export function coinTaskClaimPayloadFromTask(uid: string, task: JsonRecord): ClaimCoinTaskPayload {
  const materialDTO = asRecord(task.materialDTO)
  const materialExtra = asRecord(materialDTO.extra)
  const taskBenefitDTO = asRecord(task.taskBenefitDTO)
  const payload: ClaimCoinTaskPayload = { uid }

  assignPresent(payload, 'deliveryId', firstPresent(task, ['deliveryId']))
  assignPresent(payload, 'sceneId', firstPresent(task, ['sceneId']) ?? 496)
  assignPresent(payload, 'groupId', firstPresent(task, ['groupId']))
  assignPresent(payload, 'implId', firstPresent(task, ['implId']))
  assignPresent(payload, 'fromToken', firstPresent(task, ['fromToken']))
  assignPresent(payload, 'taskId', firstPresent(task, ['taskId', 'id']))
  assignPresent(payload, 'taskType', firstPresent(task, ['taskType']))

  assignPresent(payload, 'oliverActivityId', firstPresent(taskBenefitDTO, ['oliverActivityId']) ?? firstPresent(materialExtra, ['oliverActivityId']))
  assignPresent(payload, 'oliverBenefitCode', firstPresent(taskBenefitDTO, ['oliverBenefitCode']) ?? firstPresent(materialExtra, ['oliverBenefitCode']))
  assignPresent(payload, 'oliverType', firstPresent(taskBenefitDTO, ['oliverType']) ?? firstPresent(materialExtra, ['oliverType']))

  Object.entries(materialExtra).forEach(([key, value]) => {
    if (payload[key] === undefined || payload[key] === null || String(payload[key]).trim() === '') {
      assignPresent(payload, key, value)
    }
  })
  return payload
}

export function receiveRewardPayloadFromTask(uid: string, task: JsonRecord): RewardPayload {
  const benefitVO = asRecord(task.benefitVO)
  const payload: RewardPayload = {
    uid,
    taskId: pickString(task, ['taskId', 'id'], ''),
    taskInstanceId: pickString(task, ['taskInstanceId'], ''),
    taskType: pickString(task, ['taskType'], ''),
    benefitId: pickString(task, ['benefitId', 'benefitInstanceId'], ''),
  }
  const activityId = getTaskActivityId(task)
  if (activityId && Object.keys(benefitVO).length) {
    payload.activityId = activityId
    payload.benefitList = [benefitVO]
  }
  return payload
}
