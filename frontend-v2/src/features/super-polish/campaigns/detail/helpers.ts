import {
  asArray,
  asRecord,
  pickString,
  readPath,
  type JsonRecord,
} from '../../model'
import {
  ALL_HOURS,
  formatDivisionSummary,
  formatFenInput,
  toFenAmount,
  type DivisionSelection,
  type PromotionMode,
  type SuperPolishPackageGroup,
  type SuperPolishPackageOption,
} from '../create/helpers'
import type {
  CampaignDetailPackageSelection,
  SuperPolishCampaignDetailItem,
  SuperPolishCampaignEditSettings,
  SuperPolishCampaignSettingsDraft,
} from './types'

function trim(value: unknown) {
  return String(value ?? '').trim()
}

function readBoolean(value: unknown): boolean | null {
  if (value === true || value === 'true' || value === 1 || value === '1') return true
  if (value === false || value === 'false' || value === 0 || value === '0') return false
  return null
}

function readNumber(value: unknown): number | null {
  const numeric = Number(value)
  return Number.isFinite(numeric) ? numeric : null
}

function readPromotionMode(value: unknown): PromotionMode {
  return trim(value) === 'order' ? 'order' : 'custom'
}

function normalizeNotSupportReasons(source: JsonRecord) {
  return asArray<JsonRecord>(source.notSupportAdInfo)
    .map((entry) => trim(entry.notSupportReason))
    .filter(Boolean)
}

function normalizeTags(source: JsonRecord) {
  return asArray(source.tags).map((tag) => trim(tag)).filter(Boolean)
}

function normalizeDivisionList(value: unknown) {
  return asArray<JsonRecord>(value)
    .map((entry) => {
      const divisionId = readNumber(entry.divisionId)
      if (!divisionId) return null
      return {
        area: trim(entry.area),
        divisionId,
        province: trim(entry.province),
      }
    })
    .filter((entry): entry is { area: string; divisionId: number; province: string } => Boolean(entry))
}

function normalizeBudgetScheduleDurations(value: unknown) {
  const values = asArray(value).map((entry) => trim(entry)).filter(Boolean)
  if (values.length > 0) return values
  return ALL_HOURS.map((hour) => String(hour))
}

function normalizeHourList(value: unknown) {
  const values = normalizeBudgetScheduleDurations(value)
    .map((entry) => Number(entry))
    .filter((entry) => Number.isInteger(entry) && entry >= 0 && entry < 24)
  if (values.length === 0) return ALL_HOURS
  return Array.from(new Set(values)).sort((left, right) => left - right)
}

function normalizeDivisionSelections(value: unknown) {
  return normalizeDivisionList(value).map((entry) => ({
    area: entry.area,
    divisionId: entry.divisionId,
    province: entry.province,
  }))
}

function dedupeDivisionSelections(divisions: DivisionSelection[]) {
  return Array.from(
    new Map(
      divisions.map((entry) => [`${entry.divisionId}:${entry.province}:${entry.area}`, entry]),
    ).values(),
  )
}

function preferAreaTarget(source: JsonRecord) {
  return readBoolean(source.areaTarget)
    ?? readBoolean(source.supportAreaTarget)
    ?? false
}

function normalizePackageSelection(
  source: Partial<CampaignDetailPackageSelection> | null | undefined,
): CampaignDetailPackageSelection | null {
  if (!source) return null
  const hasAnyValue = [
    source.orderPrice,
    source.bidPrice,
    source.bidType,
    source.orderStartDuration,
    source.orderEndDuration,
    source.packageType,
    source.title,
    source.promotionText,
  ].some((value) => value !== null && value !== undefined && trim(value) !== '')
  if (!hasAnyValue) return null
  return {
    orderPrice: readNumber(source.orderPrice),
    bidPrice: readNumber(source.bidPrice),
    bidType: trim(source.bidType) || 'smart_bid',
    orderStartDuration: readNumber(source.orderStartDuration),
    orderEndDuration: readNumber(source.orderEndDuration),
    promotionText: trim(source.promotionText),
    title: trim(source.title),
    packageType: trim(source.packageType),
    creativeOptimizationAvailable: readBoolean(source.creativeOptimizationAvailable) ?? false,
  }
}

export function normalizeCampaignDetailItems(value: unknown): SuperPolishCampaignDetailItem[] {
  return asArray<JsonRecord>(value)
    .map((source) => {
      const itemId = trim(source.itemId || source.entityId)
      if (!itemId) return null
      return {
        itemId,
        catId: trim(source.catId),
        title: trim(source.title) || '未命名宝贝',
        price: Number(source.price) || 0,
        imageUrl: trim(source.imgUrl || source.picUrl),
        publishTime: readNumber(source.publishTime),
        saleCount: Number(source.saleCnt) || 0,
        supportAd: source.supportAd !== false,
        supportAreaTarget: readBoolean(source.supportAreaTarget) ?? false,
        areaTarget: preferAreaTarget(source),
        tags: normalizeTags(source),
        notSupportReasons: normalizeNotSupportReasons(source),
        raw: source,
      }
    })
    .filter((item): item is SuperPolishCampaignDetailItem => Boolean(item))
}

export function mergeCurrentCampaignItems(groupRows: JsonRecord[], campaign: JsonRecord) {
  const seedItems = normalizeCampaignDetailItems(readPath(campaign, ['entityList']))
  const seedMap = new Map(seedItems.map((item) => [item.itemId, item]))
  const groupItems = normalizeCampaignDetailItems(groupRows).map((item) => {
    const seed = seedMap.get(item.itemId)
    if (!seed) return item
    return {
      ...seed,
      ...item,
      catId: item.catId || seed.catId,
      title: item.title || seed.title,
      price: item.price || seed.price,
      imageUrl: item.imageUrl || seed.imageUrl,
      publishTime: item.publishTime ?? seed.publishTime,
      saleCount: item.saleCount || seed.saleCount,
      supportAd: item.supportAd ?? seed.supportAd,
      supportAreaTarget: item.supportAreaTarget || seed.supportAreaTarget,
      areaTarget: item.areaTarget || seed.areaTarget,
      tags: item.tags.length > 0 ? item.tags : seed.tags,
      notSupportReasons: seed.notSupportReasons.length > 0 ? seed.notSupportReasons : item.notSupportReasons,
      raw: {
        ...seed.raw,
        ...item.raw,
      },
    }
  })

  if (groupItems.length > 0) return groupItems
  return seedItems
}

export function buildCampaignDetailDraftEntity(
  item: SuperPolishCampaignDetailItem,
  areaTargetEnabled = item.areaTarget,
): JsonRecord {
  const raw = asRecord(item.raw)
  const rest = { ...raw }
  delete rest.entityId
  delete rest.picUrl
  delete rest.notSupportAdInfo

  const entity: JsonRecord = {
    ...rest,
    itemId: Number(item.itemId),
    catId: Number(item.catId),
    title: item.title,
    price: item.price,
    imgUrl: item.imageUrl,
    publishTime: item.publishTime ?? readNumber(raw.publishTime) ?? Date.now(),
    saleCnt: item.saleCount,
    supportAd: item.supportAd,
    supportAreaTarget: item.supportAreaTarget,
    areaTarget: item.supportAreaTarget ? areaTargetEnabled : false,
    tags: item.tags,
  }

  if (item.notSupportReasons.length > 0) {
    entity.notSupportAdInfo = item.notSupportReasons.map((reason) => ({
      notSupportReason: reason,
      showButton: false,
    }))
  }

  return entity
}

export function readCampaignDivisionSelections(campaign: unknown) {
  return normalizeDivisionSelections(
    readPath(campaign, ['targetInfo', 'divisionList'])
    ?? readPath(campaign, ['areaTargetInfo', 'divisionList']),
  )
}

export function readCampaignScheduleHours(campaign: unknown) {
  return normalizeHourList(
    readPath(campaign, ['budgetScheduleDurations'])
    ?? readPath(campaign, ['infoTab', 'budgetScheduleDurations']),
  )
}

export function readCampaignCreativeOptAvailable(campaign: unknown) {
  const explicit = readBoolean(readPath(campaign, ['showCreativeOptSwitch']))
  if (explicit !== null) return explicit
  return readBoolean(readPath(campaign, ['creativeOptSwitch'])) !== null
}

export function buildCampaignDetailSettingsDraft(campaign: unknown): SuperPolishCampaignSettingsDraft {
  const bidTarget = readCampaignBidTarget(campaign)
  const packageSelection = readCampaignPackageSelection(campaign)
  return {
    title: pickString(asRecord(campaign), [['title'], ['itemTitle']], ''),
    bidTarget,
    promotionMode: readCampaignPromotionMode(campaign),
    bidInput: formatFenInput(readCampaignBidCent(campaign)),
    budgetInput: formatFenInput(readCampaignBudgetCent(campaign)),
    divisions: readCampaignDivisionSelections(campaign),
    hours: readCampaignScheduleHours(campaign),
    creativeOptEnabled: readCampaignCreativeOptSwitch(campaign),
    selectedPackageOrderPriceByBidTarget: packageSelection && packageSelection.orderPrice
      ? { [bidTarget]: packageSelection.orderPrice }
      : {},
    selectedPackageByBidTarget: packageSelection ? { [bidTarget]: packageSelection } : {},
  }
}

export function resolveCampaignDetailEditSettings(
  campaign: unknown,
  draft: SuperPolishCampaignSettingsDraft,
  areaTargetAvailable: boolean,
  activePackageGroup?: SuperPolishPackageGroup | null,
): SuperPolishCampaignEditSettings {
  const bidTarget = draft.bidTarget.trim() || readCampaignBidTarget(campaign)
  const promotionMode = draft.promotionMode || readCampaignPromotionMode(campaign)
  const selectedPackageOption = resolveCampaignPackageSelection(
    bidTarget,
    draft,
    activePackageGroup,
  ) ?? readCampaignPackageSelection(campaign)

  return {
    title: draft.title.trim() || pickString(asRecord(campaign), [['title'], ['itemTitle']], ''),
    bidTarget,
    promotionMode,
    bidCent: toFenAmount(draft.bidInput) ?? readCampaignBidCent(campaign),
    budgetCent: toFenAmount(draft.budgetInput) ?? readCampaignBudgetCent(campaign),
    divisions: areaTargetAvailable ? dedupeDivisionSelections(draft.divisions) : [],
    hours: draft.hours.length > 0
      ? Array.from(new Set(draft.hours)).sort((left, right) => left - right)
      : ALL_HOURS,
    creativeOptEnabled: draft.creativeOptEnabled,
    bidType: selectedPackageOption?.bidType ?? readCampaignBidType(campaign),
    selectedPackageOption,
  }
}

export function formatCampaignDivisionSelectionSummary(divisions: DivisionSelection[]) {
  return formatDivisionSummary(divisions)
}

export function formatCampaignScheduleHoursSummary(hours: number[]) {
  return hours.length === ALL_HOURS.length ? '全部时段' : `${hours.length}个时段`
}

export function formatCampaignPackageSelectionSummary(selection: CampaignDetailPackageSelection | null) {
  if (!selection) return '待返回'
  const parts = []
  if (selection.orderStartDuration && selection.orderEndDuration) {
    parts.push(`${selection.orderStartDuration}-${selection.orderEndDuration} 天`)
  }
  if (selection.promotionText) {
    parts.push(selection.promotionText)
  }
  return parts.join(' · ') || '按平台推荐套餐投放'
}

export function snapshotCampaignPackageSelection(option: Partial<CampaignDetailPackageSelection> | SuperPolishPackageOption) {
  return normalizePackageSelection(option)
}

export function resolveCampaignPackageSelection(
  bidTarget: string,
  draft: Pick<
    SuperPolishCampaignSettingsDraft,
    'selectedPackageByBidTarget' | 'selectedPackageOrderPriceByBidTarget'
  >,
  activePackageGroup?: SuperPolishPackageGroup | null,
) {
  const savedPackage = normalizePackageSelection(draft.selectedPackageByBidTarget[bidTarget])
  const savedOrderPrice = readNumber(draft.selectedPackageOrderPriceByBidTarget[bidTarget] ?? savedPackage?.orderPrice)
  const options = activePackageGroup?.options ?? []

  if (options.length === 0) {
    return savedPackage
  }

  const matched = savedOrderPrice !== null
    ? options.find((option) => option.orderPrice === savedOrderPrice)
    : null
  const recommended = options.find((option) => option.isRecommended) ?? options[0] ?? null
  return snapshotCampaignPackageSelection(matched ?? recommended)
}

export function buildCampaignDetailTargetingEntityList(
  items: SuperPolishCampaignDetailItem[],
  areaTargetEnabled: boolean,
) {
  return items.map((item) => ({
    entityId: Number(item.itemId),
    catId: Number(item.catId),
    price: item.price,
    areaTarget: item.supportAreaTarget ? areaTargetEnabled : false,
    startTime: item.publishTime ?? Date.now(),
  }))
}

export function buildSuperPolishCampaignEditPayload(
  baseCampaign: unknown,
  items: SuperPolishCampaignDetailItem[],
  settings?: SuperPolishCampaignEditSettings,
) {
  const campaign = asRecord(baseCampaign)
  const divisionSelections = settings?.divisions ?? readCampaignDivisionSelections(campaign)
  const divisionList = normalizeDivisionList(divisionSelections)
  const budgetScheduleDurations = (settings?.hours ?? readCampaignScheduleHours(campaign))
    .map((hour) => String(hour))
  const areaTargetEnabled = divisionList.length > 0
  const entityList = items.map((item) => buildCampaignDetailDraftEntity(item, areaTargetEnabled))
  const entityIds = entityList
    .map((entity) => Number(entity.itemId))
    .filter((entityId) => Number.isFinite(entityId))
  const preferredPrimaryId = entityIds.includes(Number(campaign.itemId))
    ? Number(campaign.itemId)
    : entityIds.includes(Number(campaign.feedEntityId))
      ? Number(campaign.feedEntityId)
      : entityIds[0]
  const primaryEntity = entityList.find((entity) => Number(entity.itemId) === preferredPrimaryId) ?? entityList[0] ?? null

  const nextCampaign: JsonRecord = {
    ...campaign,
    entityList,
    entityIds,
    adGroupCount: entityList.length,
    budgetScheduleDurations,
    creativeOptSwitch: settings?.creativeOptEnabled ?? readCampaignCreativeOptSwitch(campaign),
    bidTarget: settings?.bidTarget ?? readCampaignBidTarget(campaign),
    bidType: settings?.bidType ?? readCampaignBidType(campaign),
    promotionMode: settings?.promotionMode ?? readCampaignPromotionMode(campaign),
  }

  if (settings) {
    nextCampaign.title = settings.title
    nextCampaign.targetInfo = { divisionList }
    nextCampaign.areaTargetInfo = {
      divisionList,
      empty: divisionList.length === 0,
    }
  } else if (divisionList.length > 0) {
    nextCampaign.targetInfo = { divisionList }
    nextCampaign.areaTargetInfo = {
      divisionList,
      empty: false,
    }
  }

  nextCampaign.maxDayBudget = settings?.budgetCent
    ?? readNumber(nextCampaign.maxDayBudget)
    ?? readNumber(readPath(campaign, ['infoTab', 'maxDayBudget']))
    ?? 0
  nextCampaign.budget = settings?.budgetCent
    ?? readNumber(campaign.budget)
    ?? readNumber(readPath(campaign, ['infoTab', 'maxDayBudget']))
    ?? 0
  nextCampaign.bid = nextCampaign.promotionMode === 'order'
    ? settings?.selectedPackageOption?.bidPrice
      ?? readCampaignBidCent(campaign)
    : settings?.bidCent
    ?? readNumber(campaign.bid)
    ?? readNumber(readPath(campaign, ['infoTab', 'bidPrice']))
    ?? readNumber(campaign.cpm)
    ?? 0
  nextCampaign.cpm = nextCampaign.promotionMode === 'order'
    ? settings?.selectedPackageOption?.bidPrice
      ?? readCampaignBidCent(campaign)
    : settings?.bidCent
    ?? readNumber(campaign.cpm)
    ?? readNumber(readPath(campaign, ['infoTab', 'cpm']))
    ?? readNumber(readPath(campaign, ['infoTab', 'bidPrice']))
    ?? 0

  if (nextCampaign.promotionMode === 'order' && settings?.selectedPackageOption) {
    const selectedPackage = settings.selectedPackageOption
    nextCampaign.orderPrice = selectedPackage.orderPrice
    nextCampaign.orderStartDuration = selectedPackage.orderStartDuration
    nextCampaign.orderEndDuration = selectedPackage.orderEndDuration
    nextCampaign.packageType = selectedPackage.packageType
    nextCampaign.orderInfo = {
      ...asRecord(readPath(campaign, ['orderInfo'])),
      orderPrice: selectedPackage.orderPrice,
      orderStartDuration: selectedPackage.orderStartDuration,
      orderEndDuration: selectedPackage.orderEndDuration,
      packageType: selectedPackage.packageType,
    }
  }

  nextCampaign.infoTab = {
    ...asRecord(readPath(campaign, ['infoTab'])),
    bidPrice: nextCampaign.bid,
    cpm: nextCampaign.cpm,
    maxDayBudget: nextCampaign.maxDayBudget,
    budgetScheduleDurations,
  }

  if (primaryEntity) {
    nextCampaign.itemId = Number(primaryEntity.itemId)
    nextCampaign.feedEntityId = Number(primaryEntity.itemId)
    nextCampaign.catId = Number(primaryEntity.catId)
    nextCampaign.price = Number(primaryEntity.price) || 0
    nextCampaign.imgUrl = trim(primaryEntity.imgUrl)
    nextCampaign.supportAreaTarget = readBoolean(primaryEntity.supportAreaTarget) ?? false
    nextCampaign.areaTarget = areaTargetEnabled
    nextCampaign.tags = asArray(primaryEntity.tags).map((tag) => trim(tag)).filter(Boolean)
  }

  return {
    itemCampaigns: [nextCampaign],
  }
}

export function formatCampaignDivisionSummary(campaign: unknown) {
  const divisionList = normalizeDivisionList(
    readPath(campaign, ['targetInfo', 'divisionList'])
    ?? readPath(campaign, ['areaTargetInfo', 'divisionList']),
  )
  if (divisionList.length === 0) return '全部地域'
  if (divisionList.length === 1) return divisionList[0].area || divisionList[0].province || '已选地域'
  return `${divisionList[0].area || '已选地域'}等${divisionList.length}城`
}

export function formatCampaignScheduleSummary(campaign: unknown) {
  const hours = normalizeBudgetScheduleDurations(
    readPath(campaign, ['budgetScheduleDurations'])
    ?? readPath(campaign, ['infoTab', 'budgetScheduleDurations']),
  )
  if (hours.length === ALL_HOURS.length) return '全部时段'
  return `${hours.length}个时段`
}

export function readCampaignBidTarget(campaign: unknown) {
  return pickString(asRecord(campaign), [['bidTarget']], 'chat')
}

export function readCampaignPromotionMode(campaign: unknown): PromotionMode {
  return readPromotionMode(readPath(campaign, ['promotionMode']))
}

export function readCampaignBidType(campaign: unknown) {
  return pickString(asRecord(campaign), [['bidType']], 'smart_bid')
}

export function readCampaignOrderPrice(campaign: unknown) {
  return readNumber(readPath(campaign, ['orderPrice']))
    ?? readNumber(readPath(campaign, ['orderInfo', 'orderPrice']))
}

export function readCampaignOrderStartDuration(campaign: unknown) {
  return readNumber(readPath(campaign, ['orderStartDuration']))
    ?? readNumber(readPath(campaign, ['orderInfo', 'orderStartDuration']))
}

export function readCampaignOrderEndDuration(campaign: unknown) {
  return readNumber(readPath(campaign, ['orderEndDuration']))
    ?? readNumber(readPath(campaign, ['orderInfo', 'orderEndDuration']))
}

export function readCampaignPackageType(campaign: unknown) {
  return pickString(asRecord(campaign), [['packageType'], ['orderInfo', 'packageType']], '')
}

export function readCampaignPackageSelection(campaign: unknown) {
  const hasOrderShape = readCampaignPromotionMode(campaign) === 'order'
    || readCampaignOrderPrice(campaign) !== null
    || readCampaignOrderStartDuration(campaign) !== null
    || readCampaignOrderEndDuration(campaign) !== null
    || Boolean(readCampaignPackageType(campaign))
  if (!hasOrderShape) return null

  return normalizePackageSelection({
    orderPrice: readCampaignOrderPrice(campaign),
    bidPrice: readCampaignBidCent(campaign),
    bidType: readCampaignBidType(campaign),
    orderStartDuration: readCampaignOrderStartDuration(campaign),
    orderEndDuration: readCampaignOrderEndDuration(campaign),
    promotionText: pickString(asRecord(campaign), [['promotionText'], ['orderInfo', 'promotionText']], ''),
    title: pickString(asRecord(campaign), [['orderTitle'], ['title']], ''),
    packageType: readCampaignPackageType(campaign),
    creativeOptimizationAvailable: readCampaignCreativeOptAvailable(campaign),
  })
}

export function readCampaignBudgetCent(campaign: unknown) {
  return readNumber(readPath(campaign, ['maxDayBudget']))
    ?? readNumber(readPath(campaign, ['infoTab', 'maxDayBudget']))
    ?? 0
}

export function readCampaignBidCent(campaign: unknown) {
  return readNumber(readPath(campaign, ['bid']))
    ?? readNumber(readPath(campaign, ['cpm']))
    ?? readNumber(readPath(campaign, ['infoTab', 'bidPrice']))
    ?? readNumber(readPath(campaign, ['infoTab', 'cpm']))
    ?? 0
}

export function readCampaignCreativeOptSwitch(campaign: unknown) {
  return readBoolean(readPath(campaign, ['creativeOptSwitch']))
    ?? readBoolean(readPath(campaign, ['showCreativeOptSwitch']))
    ?? false
}
