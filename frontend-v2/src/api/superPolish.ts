import { request } from './client'
import type { MtopResponse, ShopGrowthQueryOptions } from './types'

export type SuperPolishParams = Record<string, unknown>

function buildQuery(uid: string, payload?: SuperPolishParams) {
  const search = new URLSearchParams()
  search.set('uid', uid)
  if (payload && Object.keys(payload).length > 0) {
    search.set('requestJson', JSON.stringify(payload))
  }
  return search.toString()
}

function get<T = unknown>(
  path: string,
  uid: string,
  payload?: SuperPolishParams,
  options: ShopGrowthQueryOptions = {},
) {
  return request<MtopResponse<T>>('GET', `${path}?${buildQuery(uid, payload)}`, undefined, options)
}

function post<T = unknown>(path: string, uid: string, payload: SuperPolishParams = {}) {
  return request<MtopResponse<T>>('POST', path, { uid, payload })
}

function postConfirm<T = unknown>(path: string, uid: string, payload: SuperPolishParams = {}) {
  return request<MtopResponse<T>>('POST', path, { uid, confirm: true, payload })
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

export const getSuperPolishBootstrap = (uid: string) =>
  get('/api/super-polish/bootstrap', uid)

export const getSuperPolishHome = (uid: string) =>
  get('/api/super-polish/home', uid)

export const getSuperPolishAccountInfo = (
  uid: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get('/api/super-polish/account/info', uid, payload, options)

export const getSuperPolishLoginInfo = (uid: string) =>
  get('/api/super-polish/account/login-info', uid)

export const getSuperPolishAccountCard = (
  uid: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get('/api/super-polish/account/card', uid, payload, options)

export const getSuperPolishUnreadMessages = (uid: string) =>
  get('/api/super-polish/account/messages/unread', uid)

export const getSuperPolishMemberData = (
  uid: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get('/api/super-polish/account/member-data', uid, payload, options)

export const getSuperPolishActivityReward = (uid: string) =>
  get('/api/super-polish/account/activity-reward', uid)

export const getSuperPolishCoupons = (
  uid: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get('/api/super-polish/account/coupons', uid, payload, options)

export const getSuperPolishCouponStatistics = (
  uid: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get('/api/super-polish/account/coupon-statistics', uid, payload, options)

export const getSuperPolishQualificationCategories = (uid: string) =>
  get('/api/super-polish/qualification/categories', uid)

export const getSuperPolishQualificationRecords = (
  uid: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get('/api/super-polish/qualification/records', uid, payload, options)

export const getSuperPolishCampaigns = (
  uid: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get('/api/super-polish/campaigns', uid, payload, options)

export const createSuperPolishCampaign = (uid: string, payload: SuperPolishParams) =>
  postConfirm('/api/super-polish/campaigns', uid, payload)

export const getSuperPolishCampaignPrecheck = (uid: string) =>
  request<MtopResponse>('POST', '/api/super-polish/campaigns/precheck', { uid, payload: {} })

export const getSuperPolishRecommendedItems = (uid: string) =>
  get('/api/super-polish/campaigns/items/recommend', uid)

export const searchSuperPolishItems = (uid: string, payload: SuperPolishParams) =>
  post('/api/super-polish/campaigns/items/search', uid, payload)

export const querySuperPolishPackages = (uid: string, payload: SuperPolishParams) =>
  post('/api/super-polish/campaigns/packages/query', uid, payload)

export const estimateSuperPolishBids = (uid: string, payload: SuperPolishParams) =>
  post('/api/super-polish/campaigns/bid-estimates', uid, payload)

export const getSuperPolishCampaignGroups = (
  uid: string,
  campaignId: string,
  payload: SuperPolishParams = {},
  options: ShopGrowthQueryOptions = {},
) => get(`/api/super-polish/campaigns/${campaignId}/groups`, uid, payload, options)

export const editSuperPolishCampaign = (
  uid: string,
  campaignId: string,
  payload: SuperPolishParams,
) =>
  request<MtopResponse>('POST', `/api/super-polish/campaigns/${campaignId}/edit`, {
    uid,
    confirm: true,
    payload,
  })

export const getSuperPolishAccountReport = (uid: string, payload: SuperPolishParams) =>
  post('/api/super-polish/reports/account', uid, payload)

export const getSuperPolishGroupReport = (uid: string, payload: SuperPolishParams) =>
  post('/api/super-polish/reports/group', uid, payload)

export const getSuperPolishItemReport = (uid: string, payload: SuperPolishParams) =>
  post('/api/super-polish/reports/item', uid, payload)

export const getSuperPolishCampaignReport = (uid: string, payload: SuperPolishParams) =>
  post('/api/super-polish/reports/campaign', uid, payload)
