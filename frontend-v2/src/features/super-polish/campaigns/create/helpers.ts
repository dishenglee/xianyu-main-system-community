import {
  asArray,
  asRecord,
  pickString,
  readPath,
  type JsonRecord,
} from '../../model'

export const MAX_CREATE_ITEMS = 10
export const ALL_HOURS = Array.from({ length: 24 }, (_, index) => index)

export type PromotionMode = 'order' | 'custom'
export type CustomBidMode = 'recommend' | 'manual'

export interface SuperPolishSelectableItem {
  itemId: string
  catId: string
  title: string
  price: number
  imageUrl: string
  publishTime: number | null
  browseCount: number
  exposureCount: number
  saleCount: number
  supportAd: boolean
  supportAreaTarget: boolean
  checked: boolean
  tags: string[]
  notSupportReasons: string[]
  raw: JsonRecord
}

export interface SuperPolishItemTab {
  key: string
  label: string
  count: number
  tag: string
  description: string
}

export interface SuperPolishPackageOption {
  orderPrice: number | null
  bidPrice: number | null
  bidType: string
  orderStartDuration: number | null
  orderEndDuration: number | null
  orderImprove: number[]
  promotionText: string
  title: string
  packageType: string
  creativeOptimizationAvailable: boolean
  isRecommended: boolean
  valid: boolean
  raw: JsonRecord
}

export interface SuperPolishPackageGroup {
  bidTarget: string
  label: string
  recommendPackageType: string
  options: SuperPolishPackageOption[]
}

export interface SuperPolishBidEstimate {
  bidTarget: string
  label: string
  bidPrice: number | null
  customBidPrice: number | null
  bidType: string
  targetRecommendBudget: number | null
  recommendBudgetList: number[]
  creativeOptimizationAvailable: boolean
  raw: JsonRecord
}

export interface DivisionNode {
  id: string
  divisionId: number | null
  name: string
  level: string
  province: string
  city: string
  area: string
  hasChild: boolean
}

export interface DivisionSelection {
  divisionId: number
  province: string
  area: string
}

function readBoolean(value: unknown): boolean {
  return value === true || value === 'true' || value === 1 || value === '1'
}

function readNumber(value: unknown): number | null {
  const numeric = Number(value)
  return Number.isFinite(numeric) ? numeric : null
}

function trim(value: unknown) {
  return String(value ?? '').trim()
}

function normalizeImageUrl(value: unknown) {
  const url = trim(value)
  if (!url) return ''
  if (url.startsWith('//')) return `https:${url}`
  return url
}

function firstImageUrl(item: JsonRecord) {
  const direct = pickString(item, [
    ['imgUrl'],
    ['picUrl'],
    ['pic_url'],
    ['imageUrl'],
    ['image_url'],
    ['itemPic'],
    ['itemPicUrl'],
    ['mainPic'],
    ['majorPic'],
    ['cover'],
    ['coverUrl'],
    ['item', 'picUrl'],
    ['item', 'mainPic'],
    ['itemVO', 'picUrl'],
    ['itemVO', 'mainPic'],
  ], '')
  if (direct) return normalizeImageUrl(direct)

  const imageInfos = asArray<JsonRecord>(item.imageInfos).length
    ? asArray<JsonRecord>(item.imageInfos)
    : asArray<JsonRecord>(item.imageInfoDOList)
  const fromInfo = imageInfos
    .map((entry) => normalizeImageUrl(entry.url || entry.picUrl || entry.imageUrl))
    .find(Boolean)
  if (fromInfo) return fromInfo

  const imageUrls = asArray<unknown>(item.imageUrls)
    .map((entry) => normalizeImageUrl(entry))
    .find(Boolean)
  return imageUrls || ''
}

function stripDivisionSuffix(value: unknown) {
  return trim(value)
    .replace(/壮族自治区|回族自治区|维吾尔自治区|特别行政区|自治区$/u, '')
    .replace(/省$/u, '')
    .replace(/市$/u, '')
    .replace(/区$/u, '')
}

function formatYuan(value: number) {
  return value.toLocaleString('zh-CN', {
    minimumFractionDigits: Number.isInteger(value) ? 0 : 1,
    maximumFractionDigits: 2,
  })
}

export function formatCentMoney(value: number | string | null | undefined) {
  const numeric = Number(value)
  if (!Number.isFinite(numeric)) return '¥0'
  return `¥${formatYuan(numeric / 100)}`
}

export function formatFenInput(value: number | null | undefined) {
  const numeric = Number(value)
  if (!Number.isFinite(numeric)) return ''
  const amount = numeric / 100
  return Number.isInteger(amount) ? String(amount) : String(amount)
}

export function toFenAmount(value: string) {
  const normalized = value.trim()
  if (!normalized) return null
  const numeric = Number(normalized)
  if (!Number.isFinite(numeric) || numeric <= 0) return null
  return Math.round(numeric * 100)
}

export function defaultPlanTitle(now = new Date()) {
  const parts = [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, '0'),
    String(now.getDate()).padStart(2, '0'),
    String(now.getHours()).padStart(2, '0'),
    String(now.getMinutes()).padStart(2, '0'),
    String(now.getSeconds()).padStart(2, '0'),
  ]
  return `擦亮宝贝${parts.join('')}`
}

export function bidTargetLabel(value: unknown) {
  switch (trim(value)) {
    case 'exposure':
      return '曝光量'
    case 'click':
      return '浏览量'
    case 'follow_collect':
      return '关注收藏量'
    case 'chat':
      return '咨询量'
    case 'order':
      return '订单量'
    default:
      return trim(value) || '未命名目标'
  }
}

export function normalizeSelectableItems(value: unknown) {
  return asArray<JsonRecord>(value).map((item) => {
    const notSupportReasons = asArray<JsonRecord>(item.notSupportAdInfo)
      .map((entry) => trim(entry.notSupportReason))
      .filter(Boolean)
    return {
      itemId: trim(item.itemId),
      catId: trim(item.catId),
      title: trim(item.title) || '未命名宝贝',
      price: Number(item.price) || 0,
      imageUrl: firstImageUrl(item),
      publishTime: readNumber(item.publishTime),
      browseCount: Number(item.broseCnt) || 0,
      exposureCount: Number(item.exposureCnt) || 0,
      saleCount: Number(item.saleCnt) || 0,
      supportAd: item.supportAd !== false,
      supportAreaTarget: readBoolean(item.supportAreaTarget),
      checked: readBoolean(item.checked),
      tags: asArray(item.tags).map((tag) => trim(tag)).filter(Boolean),
      notSupportReasons,
      raw: item,
    }
  }).filter((item) => item.itemId)
}

export function normalizeItemTabs(value: unknown) {
  const tabs = asArray<JsonRecord>(asRecord(value).tabList).map((tab) => ({
    key: trim(tab.tab),
    label: trim(tab.tabName) || trim(tab.tab),
    count: Number(tab.count) || 0,
    tag: trim(tab.tabTag),
    description: trim(tab.tabDesc),
  })).filter((tab) => tab.key)

  if (tabs.length > 0) return tabs
  return [
    { key: 'all', label: '全部', count: 0, tag: '', description: '' },
    { key: 'platReco', label: '平台推荐', count: 0, tag: '', description: '' },
  ]
}

export function normalizePackageGroups(value: unknown) {
  return asArray<JsonRecord>(asRecord(value).packageTargetVOList).map((group) => ({
    bidTarget: trim(group.bidTarget),
    label: bidTargetLabel(group.bidTarget),
    recommendPackageType: trim(group.recommendPackageType),
    options: asArray<JsonRecord>(group.orderVOList).map((order) => ({
      orderPrice: readNumber(order.orderPrice),
      bidPrice: readNumber(order.bidPrice),
      bidType: trim(order.bidType) || 'smart_bid',
      orderStartDuration: readNumber(order.orderStartDuration),
      orderEndDuration: readNumber(order.orderEndDuration),
      orderImprove: asArray(order.orderImprove).map((entry) => Number(entry)).filter((entry) => Number.isFinite(entry)),
      promotionText: trim(order.promotionText),
      title: trim(order.title),
      packageType: trim(order.packageType),
      creativeOptimizationAvailable: readBoolean(readPath(order, ['extraFunctions', 'showCreativeOptSwitch'])),
      isRecommended: readBoolean(order.algoRecommend) || readBoolean(order.recommend),
      valid: order.valid !== 0,
      raw: order,
    })),
  })).filter((group) => group.bidTarget)
}

export function normalizeBidEstimates(value: unknown) {
  return asArray<JsonRecord>(asRecord(value).customizeBidRecommendVOList).map((estimate) => ({
    bidTarget: trim(estimate.bidTarget),
    label: bidTargetLabel(estimate.bidTarget),
    bidPrice: readNumber(estimate.bidPrice),
    customBidPrice: readNumber(estimate.customBidPrice),
    bidType: trim(estimate.bidType) || 'smart_bid',
    targetRecommendBudget: readNumber(estimate.targetRecommendBudget),
    recommendBudgetList: asArray(estimate.recommendBudgetList).map((entry) => Number(entry)).filter((entry) => Number.isFinite(entry)),
    creativeOptimizationAvailable: readBoolean(readPath(estimate, ['extraFunctions', 'showCreativeOptSwitch'])),
    raw: estimate,
  })).filter((estimate) => estimate.bidTarget)
}

export function buildTargetingEntityList(items: SuperPolishSelectableItem[], areaTarget: boolean) {
  return items.map((item) => ({
    entityId: Number(item.itemId),
    catId: Number(item.catId),
    price: item.price,
    areaTarget: item.supportAreaTarget ? areaTarget : false,
    startTime: item.publishTime ?? Date.now(),
  }))
}

export function buildCreateEntityList(items: SuperPolishSelectableItem[], areaTargetEnabled = false) {
  return items.map((item) => ({
    entityId: Number(item.itemId),
    catId: Number(item.catId),
    price: item.price,
    ...(areaTargetEnabled
      ? { areaTarget: item.supportAreaTarget }
      : {}),
    startTime: item.publishTime ?? Date.now(),
  }))
}

export function buildAccountInfoPayload({
  promotionMode,
  orderPrice,
  bidType,
}: {
  promotionMode: PromotionMode
  orderPrice: number | null
  bidType: string
}) {
  if (!orderPrice) return null
  return {
    orderPrice,
    promotionMode,
    bidType,
    scene: 'creationRealTime',
  }
}

export function buildCreatePayload({
  oldMember,
  items,
  promotionMode,
  bidTarget,
  bidType,
  title,
  packageOption,
  bidPrice,
  maxDayBudget,
  budgetScheduleDurations,
  divisions,
  creativeOptSwitch,
}: {
  oldMember: boolean
  items: SuperPolishSelectableItem[]
  promotionMode: PromotionMode
  bidTarget: string
  bidType: string
  title: string
  packageOption: SuperPolishPackageOption | null
  bidPrice: number | null
  maxDayBudget: number | null
  budgetScheduleDurations: number[]
  divisions: DivisionSelection[]
  creativeOptSwitch: boolean
}) {
  const areaTargetEnabled = divisions.length > 0
  const effectivePromotionMode: PromotionMode = areaTargetEnabled ? 'custom' : promotionMode
  const payload: Record<string, unknown> = {
    entityType: 1,
    fromScene: 'pc_campaign-manage_yuxiaopu',
    oldMember,
    entityList: buildCreateEntityList(items, areaTargetEnabled),
    baseInfo: {
      productId: 1,
      promotionMode: effectivePromotionMode,
      bidType,
      bidTarget,
      title,
    },
    clueInfo: {
      chatButton: true,
      bookButton: false,
      bookText: '',
    },
    extraFunctions: {
      creativeOptSwitch,
    },
  }

  if (effectivePromotionMode === 'order' && packageOption?.orderPrice) {
    payload.orderInfo = {
      orderPrice: packageOption.orderPrice,
      orderStartDuration: packageOption.orderStartDuration,
      orderEndDuration: packageOption.orderEndDuration,
      orderImprove: packageOption.orderImprove,
      packageType: packageOption.packageType,
    }
  }

  if (effectivePromotionMode === 'custom' || budgetScheduleDurations.length !== ALL_HOURS.length) {
    payload.customInfo = {
      ...(effectivePromotionMode === 'custom'
        ? {
            bidPrice,
            maxDayBudget,
          }
        : {}),
      budgetScheduleDurations,
    }
  }

  if (divisions.length > 0) {
    payload.targetInfo = {
      divisionList: divisions,
    }
  }

  return payload
}

export function formatItemStats(item: SuperPolishSelectableItem) {
  return `浏览 ${item.browseCount} · 曝光 ${item.exposureCount} · 销量 ${item.saleCount}`
}

export function packageOptionDescription(option: SuperPolishPackageOption) {
  const parts = []
  if (option.orderStartDuration && option.orderEndDuration) {
    parts.push(`${option.orderStartDuration}-${option.orderEndDuration} 天`)
  }
  if (option.orderImprove.length === 2) {
    parts.push(`预计提升 ${option.orderImprove[0]}-${option.orderImprove[1]}`)
  }
  if (option.promotionText) {
    parts.push(option.promotionText)
  }
  return parts.join(' · ') || '按平台推荐套餐投放'
}

export function estimateDescription(estimate: SuperPolishBidEstimate) {
  const parts = []
  if (estimate.bidPrice) {
    parts.push(`平台推荐 ${formatCentMoney(estimate.bidPrice)}`)
  }
  if (estimate.customBidPrice) {
    parts.push(`自定义上限 ${formatCentMoney(estimate.customBidPrice)}`)
  }
  if (estimate.recommendBudgetList.length > 0) {
    parts.push(`推荐预算 ${estimate.recommendBudgetList.map((budget) => formatCentMoney(budget)).join(' / ')}`)
  }
  return parts.join(' · ')
}

export function formatAdvancedSummary(divisions: DivisionSelection[], hours: number[]) {
  const regionText = divisions.length === 0
    ? '全部地域'
    : divisions.length === 1
      ? `${divisions[0].province} ${divisions[0].area}`
      : `${divisions[0].province}等${divisions.length}城`
  const timeText = hours.length === ALL_HOURS.length
    ? '全部时段'
    : `${hours.length}个时段`
  return `在${regionText}投放，在${timeText}投放`
}

export function formatDivisionSummary(divisions: DivisionSelection[]) {
  if (divisions.length === 0) return '全部地域'
  if (divisions.length === 1) return `${divisions[0].province} ${divisions[0].area}`
  return `${divisions[0].province}等${divisions.length}城`
}

export function formatTimeSummary(hours: number[]) {
  return hours.length === ALL_HOURS.length ? '全部时段' : '部分时段'
}

export function normalizeDivisionNode(value: unknown): DivisionNode | null {
  const record = asRecord(value)
  const id = trim(record.id || record.divisionId)
  if (!id) return null
  return {
    id,
    divisionId: readNumber(record.divisionId || record.id),
    name: trim(record.na || record.name || record.area || record.city || record.prov),
    level: trim(record.level),
    province: trim(record.prov || record.province || record.pname),
    city: trim(record.city || record.cityName || record.cname),
    area: trim(record.area || record.district || record.adname),
    hasChild: Boolean(record.hasChild ?? record.haschild),
  }
}

export function normalizeDivisionTree(value: unknown, field: string) {
  return asArray(asRecord(value)[field])
    .map((entry) => normalizeDivisionNode(entry))
    .filter(Boolean) as DivisionNode[]
}

export function toDivisionSelection(node: DivisionNode): DivisionSelection | null {
  if (!node.divisionId) return null
  const province = stripDivisionSuffix(node.province || node.name)
  const area = stripDivisionSuffix(node.city || node.area || node.name)
  if (!province || !area) return null
  return {
    divisionId: node.divisionId,
    province,
    area,
  }
}

export function selectionKey(selection: DivisionSelection) {
  return `${selection.divisionId}:${selection.province}:${selection.area}`
}

export function buildBidTargetOrder(groups: SuperPolishPackageGroup[], estimates: SuperPolishBidEstimate[], preferredBidTarget: string) {
  const keys = new Set([
    ...groups.map((group) => group.bidTarget),
    ...estimates.map((estimate) => estimate.bidTarget),
  ])
  const preferred = ['exposure', 'click', 'follow_collect', 'chat', 'order']
    .filter((key) => keys.has(key))
  const withRecTarget = preferredBidTarget && keys.has(preferredBidTarget)
    ? [preferredBidTarget, ...preferred.filter((key) => key !== preferredBidTarget)]
    : preferred
  return withRecTarget.length > 0 ? withRecTarget : Array.from(keys)
}

export function firstCheckedItemIds(items: SuperPolishSelectableItem[]) {
  const checked = items.filter((item) => item.checked && item.supportAd).map((item) => item.itemId)
  if (checked.length > 0) return checked.slice(0, MAX_CREATE_ITEMS)
  const firstAvailable = items.find((item) => item.supportAd)
  return firstAvailable ? [firstAvailable.itemId] : []
}

export function mergeItems(...groups: SuperPolishSelectableItem[][]) {
  const map = new Map<string, SuperPolishSelectableItem>()
  for (const group of groups) {
    for (const item of group) {
      map.set(item.itemId, item)
    }
  }
  return map
}

export function upsertItems(current: SuperPolishSelectableItem[], items: SuperPolishSelectableItem[]) {
  const map = mergeItems(current, items)
  return Array.from(map.values())
}

export function limitedUniqueIds(baseIds: string[], nextIds: string[]) {
  const merged = Array.from(new Set([...baseIds, ...nextIds]))
  return merged.slice(0, MAX_CREATE_ITEMS)
}

export function safeText(value: unknown, fallback = '未返回') {
  return pickString({ value }, [['value']], fallback)
}
