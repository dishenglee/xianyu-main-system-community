import { request } from './client'
import type {
  ClaimCoinTaskPayload,
  CompleteSafeViewTasksPayload,
  CreateExposureOrderPayload,
  CreatePromotionPayload,
  DatacompassDateType,
  MtopResponse,
  ReceiveExposureRewardPayload,
  RunExposureTaskPayload,
  ShopGrowthQueryOptions,
  SuperExposurePayload,
} from './types'

function qs(params: Record<string, string | number | boolean | undefined | null>) {
  const search = new URLSearchParams()
  Object.entries(params).forEach(([key, value]) => {
    if (value === undefined || value === null || value === '') return
    search.set(key, String(value))
  })
  return search.toString()
}

export function unwrapMtopData<T = unknown>(payload: MtopResponse<T> | T): T {
  if (
    payload
    && typeof payload === 'object'
    && !Array.isArray(payload)
    && Object.prototype.hasOwnProperty.call(payload, 'data')
  ) {
    return (payload as MtopResponse<T>).data as T
  }
  return payload as T
}

export function mtopRetText(payload: unknown): string {
  const ret = (payload as { ret?: unknown })?.ret
  if (Array.isArray(ret)) return String(ret[0] ?? '')
  return String(ret ?? '')
}

export function mtopSucceeded(payload: unknown): boolean {
  return mtopRetText(payload).toUpperCase().includes('SUCCESS')
}

export const getCoinSigninHome = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/signin/home?${qs({ uid })}`, undefined, options)

export const getCoinChallengeInfo = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/signin/challenge-info?${qs({ uid })}`, undefined, options)

export const signInCoin = (uid: string) =>
  request<MtopResponse>('POST', '/api/growth/coin/signin', { uid, confirm: true })

export const getCoinActivityUserInfo = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/activity/user-info?${qs({ uid })}`, undefined, options)

export const getCoinUserDetailList = (
  uid: string,
  page = 1,
  pageSize = 20,
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/growth/coin/user-detail-list?${qs({ uid, page, pageSize })}`, undefined, options)

export const getCoinVirtualCardList = (
  uid: string,
  cardType = 7,
  page = 1,
  pageSize = 10,
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/growth/coin/virtual-card-list?${qs({ uid, cardType, page, pageSize })}`, undefined, options)

export const getCoinTasks = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/tasks?${qs({ uid })}`, undefined, options)

export const getCoinTaskDetail = (
  uid: string,
  deliveryId: string | number,
  sceneId = 496,
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/growth/coin/task/detail?${qs({ uid, deliveryId, sceneId })}`, undefined, options)

export const getCoinWljTaskDetail = (
  uid: string,
  deliveryId: string | number,
  sceneId = 496,
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/growth/coin/task/wlj-detail?${qs({ uid, deliveryId, sceneId })}`, undefined, options)

export const completeCoinViewTasks = (payload: string | CompleteSafeViewTasksPayload) => {
  const body = typeof payload === 'string' ? { uid: payload } : payload
  return request<MtopResponse>('POST', '/api/growth/coin/tasks/complete-view', {
    ...body,
    confirm: true,
  })
}

export const claimCoinTask = (payload: ClaimCoinTaskPayload) =>
  request<MtopResponse>('POST', '/api/growth/coin/tasks/claim', { ...payload, confirm: true })

export const getCoinSubscribeStatus = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/subscribe?${qs({ uid })}`, undefined, options)

export const getPromotionView = (uid: string, itemId?: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/packages?${qs({ uid, itemId })}`, undefined, options)

export const getPromotionPage = (uid: string, page = 1, pageSize = 20, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/orders?${qs({ uid, page, pageSize })}`, undefined, options)

export const getPromotionDetail = (uid: string, promotionId: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/coin/detail?${qs({ uid, promotionId })}`, undefined, options)

export const createPromotion = (payload: CreatePromotionPayload) =>
  request<MtopResponse>('POST', '/api/growth/coin/create', {
    ...payload,
    confirm: true,
    expectedCostCoinNum: payload.expectedCostCoinNum ?? payload.costCoinNum,
  })

export const getExposureTasks = (uid: string, scene = 'ONLY_TASK_LIST', options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/exposure/tasks?${qs({ uid, scene })}`, undefined, options)

export const getExposureHome = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/exposure/home?${qs({ uid })}`, undefined, options)

export const getExposureAmountOptions = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/exposure/amount-options?${qs({ uid })}`, undefined, options)

export const getExposureItemRendering = (
  uid: string,
  itemIds: string[],
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/growth/exposure/item-rendering?${qs({ uid, itemId: itemIds.join(',') })}`, undefined, options)

export const createExposureOrder = (payload: CreateExposureOrderPayload) =>
  request<MtopResponse>('POST', '/api/growth/exposure/create', {
    ...payload,
    confirm: true,
    expectedExposureNum: payload.expectedExposureNum ?? payload.exposureNum,
  })

export const completeSafeViewTasks = (payload: string | CompleteSafeViewTasksPayload) => {
  const body = typeof payload === 'string' ? { uid: payload } : payload
  return request<MtopResponse>('POST', '/api/growth/exposure/tasks/complete-view', {
    ...body,
    confirm: true,
  })
}

export const syncExposureTask = (payload: ReceiveExposureRewardPayload) =>
  request<MtopResponse>('POST', '/api/growth/exposure/tasks/sync', { ...payload, confirm: true })

export const runExposureTask = (payload: RunExposureTaskPayload) =>
  request<MtopResponse>('POST', '/api/growth/exposure/tasks/run', { ...payload, confirm: true })

export const receiveExposureReward = (payload: ReceiveExposureRewardPayload) =>
  request<MtopResponse>('POST', '/api/growth/exposure/tasks/receive', { ...payload, confirm: true })

export const getExposureHistory = (uid: string, page = 1, pageSize = 20, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/exposure/history?${qs({ uid, page, pageSize })}`, undefined, options)

export const getExposureDetails = (
  uid: string,
  params: {
    page?: number
    pageSize?: number
    itemId?: string
    date?: string
    instanceId?: string
    bizCode?: string
    taskInstanceId?: string
  } = {},
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>(
    'GET',
    `/api/growth/exposure/details?${qs({
      uid,
      page: params.page ?? 1,
      pageSize: params.pageSize ?? 20,
      itemId: params.itemId,
      date: params.date,
      instanceId: params.instanceId,
      bizCode: params.bizCode,
      taskInstanceId: params.taskInstanceId,
    })}`,
    undefined,
    options,
  )

export const getShopWorkbench = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/shop/workbench?${qs({ uid })}`, undefined, options)

export const getSuperPolishAccount = (uid: string, options: ShopGrowthQueryOptions = {}) =>
  request<MtopResponse>('GET', `/api/growth/super-polish/account?${qs({ uid })}`, undefined, options)

export const triggerSuperExposure = (payload: string | SuperExposurePayload) => {
  const body = typeof payload === 'string' ? { uid: payload } : payload
  return request<MtopResponse>('POST', '/api/growth/super-polish/trigger', {
    ...body,
    confirm: true,
  })
}

export const getDatacompassSummary = (
  uid: string,
  dateType: DatacompassDateType = 'recent1d',
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/shop/datacompass/summary?${qs({ uid, dateType })}`, undefined, options)

export const getDatacompassItems = (
  uid: string,
  dateType: DatacompassDateType = 'recent7d',
  page = 1,
  pageSize = 20,
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/shop/datacompass/items?${qs({ uid, dateType, page, pageSize })}`, undefined, options)

export const getDatacompassFlow = (
  uid: string,
  dateType: DatacompassDateType = 'recent7d',
  options: ShopGrowthQueryOptions = {},
) =>
  request<MtopResponse>('GET', `/api/shop/datacompass/flow?${qs({ uid, dateType })}`, undefined, options)
