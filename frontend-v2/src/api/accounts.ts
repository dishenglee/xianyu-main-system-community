import type { Account, RequestOptions } from './types'
import { request } from './client'

// ==================== 账号管理 ====================

export const getAccounts = (options: RequestOptions = {}) =>
  request<Account[]>('GET', '/api/accounts', undefined, options)
export const deleteAccount = (uid: string, options: RequestOptions = {}) =>
  request('DELETE', `/api/accounts/${uid}`, undefined, options)
export const refreshAccount = (uid: string, options: RequestOptions = {}) =>
  request('POST', `/api/accounts/${uid}/refresh`, undefined, options)
export const refreshAllAccounts = (options: RequestOptions = {}) =>
  request('POST', '/api/accounts/refresh-all', undefined, options)
export const accountsHealth = (options: RequestOptions = {}) =>
  request('GET', '/api/accounts/health', undefined, options)

// ==================== 扫码登录 ====================

export const qrGenerate = () => request('POST', '/api/qr/generate')
export const qrStatus = (sid: string) => request('GET', `/api/qr/status/${sid}`)
export const createAccountUnbindRequest = (
  payload: { account_uid: string; reason?: string },
  options: RequestOptions = {},
) => request('POST', '/api/accounts/unbind-requests', payload, options)

// ==================== 账号信息 ====================

export const accountSummary = (uid: string, force = false, options: RequestOptions = {}) =>
  request('GET', `/api/account/summary?uid=${uid}${force ? '&force=true' : ''}`, undefined, options)
export const accountProfile = (uid: string, force = false) =>
  request('GET', `/api/account/profile?uid=${uid}${force ? '&force=true' : ''}`)
export const accountEditableProfile = (uid: string, force = false) =>
  request('GET', `/api/account/profile/editable?uid=${uid}${force ? '&force=true' : ''}`)

export type AccountProfileUpdatePayload = {
  uid: string
  snsNick?: string
  defaultNick?: boolean | string
  signature?: string
  signatureTitle?: string
  avatar?: string
  gender?: string
  birthday?: string
  city?: string
  university?: string
}

export const updateAccountProfile = (payload: AccountProfileUpdatePayload, options: RequestOptions = {}) =>
  request('POST', '/api/account/profile', payload, options)

// 账号扩展数据接口（每个都返回 MTOP 原始 wrapper，data 字段里是具体内容）
export const accountCoinDetail = (uid: string, page = 1, pageSize = 50) =>
  request<any>('GET', `/api/account/coin_detail?uid=${uid}&page=${page}&page_size=${pageSize}`)
export const accountSecurityScoreRecords = (uid: string, page = 1) =>
  request<any>('GET', `/api/account/security_score/records?uid=${uid}&page=${page}`)
export const accountSellerData = (uid: string, dateType: 'recent1d' | 'recent7d' | 'recent30d' = 'recent30d') =>
  request<any>('GET', `/api/account/seller_data?uid=${uid}&date_type=${dateType}`)
export const securityScore = (uid: string) => request('GET', `/api/account/security_score?uid=${uid}`)
export const securityScoreRecords = (uid: string, page = 1) =>
  request('GET', `/api/account/security_score/records?uid=${uid}&page=${page}`)
export const accountViolations = (uid: string) => request('GET', `/api/account/violations?uid=${uid}`)
export const accountViolationsAll = (uid: string, pageSize = 50) =>
  request('GET', `/api/account/violations/all?uid=${uid}&page_size=${pageSize}`)
export const sellerData = (uid: string, dateType = 'recent30d') =>
  request('GET', `/api/account/seller_data?uid=${uid}&date_type=${dateType}`)
export const securityDetection = (uid: string) => request('GET', `/api/account/security_detection?uid=${uid}`)
export const accountCoinBalance = (uid: string) => request('GET', `/api/account/coin_balance?uid=${uid}`)
export const accountUserPage = (uid: string) => request('GET', `/api/account/user_page?uid=${uid}`)
export const accountUserHead = (uid: string) => request('GET', `/api/account/user_head?uid=${uid}`)
export const accountAlipay = (uid: string) => request('GET', `/api/account/alipay?uid=${uid}`)

export const updateAccountLocation = (uid: string, loc: unknown, options: RequestOptions = {}) =>
  request('POST', `/api/accounts/${uid}/location`, loc, options)

// ==================== 发布地（publish_locations 表）====================

export type PublishLocationRecord = {
  uid: string
  fishAdCode: string
  gps: string
  latitude: string
  longitude: string
  name: string
  address: string
  province: string
  city: string
  district: string
  updatedAt: number
}

export type PublishLocationResponse = {
  uid: string
  overridden: boolean
  effective: Record<string, string>
  record: PublishLocationRecord | null
}

export const getPublishLocation = (uid: string, options: RequestOptions = {}) =>
  request<PublishLocationResponse>(
    'GET',
    `/api/accounts/${uid}/publish_location`,
    undefined,
    options,
  )

export const putPublishLocation = (
  uid: string,
  payload: Partial<PublishLocationRecord>,
  options: RequestOptions = {},
) =>
  request<PublishLocationResponse>(
    'PUT',
    `/api/accounts/${uid}/publish_location`,
    payload,
    options,
  )

export const deletePublishLocation = (uid: string, options: RequestOptions = {}) =>
  request('DELETE', `/api/accounts/${uid}/publish_location`, undefined, options)

export type CityEntry = {
  province: string
  city: string
  district: string
  fishAdCode: string
  latitude: string
  longitude: string
  gps: string
}

export const listCities = (options: RequestOptions = {}) =>
  request<{ items: CityEntry[] }>('GET', '/api/location/cities', undefined, options)

// ==================== S04 aliases ====================

export const addAccount = () => qrGenerate()
export const getAccountDetail = (uid: string) =>
  request('GET', `/api/account/profile?uid=${uid}`)
export const updateAccountSetting = (uid: string, payload: Record<string, unknown>) =>
  request('PATCH', `/api/accounts/${uid}`, payload)
