import {
  asRecord,
  compactDate,
  pickString,
  readPath,
  type JsonRecord,
} from '../model'

export const DEFAULT_REPORT_CAMPAIGN_STATUSES = [-6, -5, -4, -3, -2, 1, 2, 3, 4, 6, 99]

export interface ReportCampaignOption {
  campaignId: string
  adGroupId: string
  adGroupCount: number
  label: string
  status: string
}

export interface ReportGroupOption {
  adGroupId: string
  label: string
}

export function buildBatchReportPayload(startDate: string, endDate: string, pageSize: string) {
  return {
    pageNum: 1,
    pageSize: Number(pageSize),
    startDate: compactDate(startDate),
    endDate: compactDate(endDate),
    includeEndDate: true,
    allPeriod: false,
  }
}

export function buildCampaignCatalogPayload(startDate: string, endDate: string, pageSize: string) {
  return {
    ...buildBatchReportPayload(startDate, endDate, pageSize),
    statusGroup: [],
    campaignStatus: DEFAULT_REPORT_CAMPAIGN_STATUSES,
  }
}

export function buildGroupCatalogPayload(startDate: string, endDate: string, pageSize: string, campaignId: string) {
  return {
    pageNum: 1,
    pageSize: Number(pageSize),
    startDate: compactDate(startDate),
    endDate: compactDate(endDate),
    includeEndDate: true,
    campaignId: Number(campaignId),
  }
}

export function buildGroupReportPayload(startDate: string, endDate: string, groupId: string) {
  return {
    startDate: compactDate(startDate),
    endDate: compactDate(endDate),
    includeEndDate: true,
    adGroupId: groupId,
    allPeriod: false,
  }
}

export function campaignOptionsFromRows(rows: JsonRecord[]): ReportCampaignOption[] {
  return rows
    .map((row) => {
      const campaignId = pickString(row, [['campaignId']], '')
      const adGroupId = pickString(row, [['adGroupId']], '')
      if (!campaignId || !adGroupId) return null
      return {
        campaignId,
        adGroupId,
        adGroupCount: Number(readPath(row, ['adGroupCount']) ?? 0),
        label: pickString(row, [['title'], ['itemTitle']], `计划 ${campaignId}`),
        status: pickString(row, [['statusDescTitle'], ['statusDesc'], ['status']], ''),
      }
    })
    .filter((option): option is ReportCampaignOption => Boolean(option))
}

export function groupOptionsFromRows(rows: JsonRecord[]): ReportGroupOption[] {
  const seen = new Set<string>()
  const options: ReportGroupOption[] = []
  for (const row of rows) {
    const adGroupId = pickString(row, [['adGroupId'], ['groupId'], ['id']], '')
    if (!adGroupId || seen.has(adGroupId)) continue
    seen.add(adGroupId)
    options.push({
      adGroupId,
      label: pickString(row, [['groupName'], ['title'], ['itemTitle']], `分组 ${adGroupId}`),
    })
  }
  return options
}

export function deriveItemReportRowsFromCampaignRows(rows: JsonRecord[]): JsonRecord[] {
  return rows.map((row) => {
    const metrics = asRecord(readPath(row, ['adDataDetailVO']))
    return {
      itemId: pickString(row, [['itemId'], ['feedEntityId'], ['entityId']], ''),
      itemTitle: pickString(row, [['itemTitle'], ['title'], ['campaignName']], '未命名商品'),
      campaignName: pickString(row, [['title'], ['campaignName'], ['campaignTitle']], ''),
      campaignId: pickString(row, [['campaignId']], ''),
      statusDesc: pickString(row, [['statusDescTitle'], ['statusDesc'], ['status']], ''),
      clickPv: readPath(row, ['clickPv']) ?? readPath(metrics, ['clickPv']) ?? 0,
      chatPv: readPath(row, ['chatPv']) ?? readPath(metrics, ['chatPv']) ?? 0,
      favorites: readPath(row, ['favorites']) ?? readPath(metrics, ['favorites']) ?? 0,
      cost: readPath(row, ['cost']) ?? readPath(metrics, ['cost']) ?? 0,
    }
  })
}
