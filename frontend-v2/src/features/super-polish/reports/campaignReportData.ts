import {
  getSuperPolishCampaignReport,
  unwrapMtopData,
  type SuperPolishParams,
} from '../../../api/superPolish'
import { readPath } from '../model'
import { assertSuperPolishRequestSuccess } from '../mtop'
import { buildBatchReportPayload } from './reportCatalog'

const REPORT_MAX_WINDOW_DAYS = 7
const REPORT_FETCH_PAGE_SIZE = 50
const REPORT_WINDOW_CONCURRENCY = 4

export interface CampaignReportDetailItem {
  time: string
  clickPv: number
  chatPv: number
  favorites: number
  cost: number
  showPv: number
  inquiries: number
  payPv: number
  realCost: number
}

export interface CampaignReportRow {
  campaignId: string
  title: string
  statusCode: number | null
  adGroupCount: number
  adGroupIds: string[]
  clickPv: number
  chatPv: number
  favorites: number
  cost: number
  showPv: number
  inquiries: number
  payPv: number
  realCost: number
  detailItems: CampaignReportDetailItem[]
}

export interface CampaignReportSummary {
  clickPv: number
  chatPv: number
  favorites: number
  cost: number
  showPv: number
  inquiries: number
  payPv: number
  realCost: number
}

export interface CampaignReportWindow {
  startDate: string
  endDate: string
}

export interface CampaignReportRangeData {
  rows: CampaignReportRow[]
  summary: CampaignReportSummary
  windows: CampaignReportWindow[]
}

type CampaignReportRequest = (
  uid: string,
  payload: SuperPolishParams,
) => Promise<unknown>

function zeroSummary(): CampaignReportSummary {
  return {
    clickPv: 0,
    chatPv: 0,
    favorites: 0,
    cost: 0,
    showPv: 0,
    inquiries: 0,
    payPv: 0,
    realCost: 0,
  }
}

function toNumber(value: unknown) {
  const numeric = Number(value)
  return Number.isFinite(numeric) ? numeric : 0
}

function parseInputDate(value: string) {
  return new Date(`${value}T00:00:00`)
}

function formatInputDate(value: Date) {
  const year = value.getFullYear()
  const month = String(value.getMonth() + 1).padStart(2, '0')
  const day = String(value.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function addDays(source: Date, days: number) {
  const next = new Date(source)
  next.setDate(next.getDate() + days)
  return next
}

function detailItemFromUnknown(value: unknown): CampaignReportDetailItem {
  return {
    time: String(readPath(value, ['time']) ?? ''),
    clickPv: toNumber(readPath(value, ['clickPv'])),
    chatPv: toNumber(readPath(value, ['chatPv'])),
    favorites: toNumber(readPath(value, ['favorites'])),
    cost: toNumber(readPath(value, ['cost'])),
    showPv: toNumber(readPath(value, ['showPv'])),
    inquiries: toNumber(readPath(value, ['inquiries'])),
    payPv: toNumber(readPath(value, ['payPv'])),
    realCost: toNumber(readPath(value, ['realCost'])),
  }
}

function summarizeDetailItems(items: CampaignReportDetailItem[], fallback?: CampaignReportSummary) {
  if (items.length === 0) {
    return fallback ?? zeroSummary()
  }

  return items.reduce<CampaignReportSummary>((summary, item) => ({
    clickPv: summary.clickPv + item.clickPv,
    chatPv: summary.chatPv + item.chatPv,
    favorites: summary.favorites + item.favorites,
    cost: summary.cost + item.cost,
    showPv: summary.showPv + item.showPv,
    inquiries: summary.inquiries + item.inquiries,
    payPv: summary.payPv + item.payPv,
    realCost: summary.realCost + item.realCost,
  }), zeroSummary())
}

function mergeDetailItems(items: CampaignReportDetailItem[]) {
  const byTime = new Map<string, CampaignReportDetailItem>()

  for (const item of items) {
    const key = item.time || `${item.clickPv}:${item.chatPv}:${item.cost}:${item.showPv}`
    const current = byTime.get(key)
    if (!current) {
      byTime.set(key, { ...item })
      continue
    }

    byTime.set(key, {
      time: current.time || item.time,
      clickPv: current.clickPv + item.clickPv,
      chatPv: current.chatPv + item.chatPv,
      favorites: current.favorites + item.favorites,
      cost: current.cost + item.cost,
      showPv: current.showPv + item.showPv,
      inquiries: current.inquiries + item.inquiries,
      payPv: current.payPv + item.payPv,
      realCost: current.realCost + item.realCost,
    })
  }

  return Array.from(byTime.values()).sort((left, right) => left.time.localeCompare(right.time))
}

function normalizeCampaignReportRow(value: unknown): CampaignReportRow | null {
  const campaignId = String(readPath(value, ['campaignId']) ?? '').trim()
  if (!campaignId) return null

  const detailItems = Array.isArray(readPath(value, ['idleAdGroupDataVO', 'dataDetailVOList']))
    ? (readPath(value, ['idleAdGroupDataVO', 'dataDetailVOList']) as unknown[]).map(detailItemFromUnknown)
    : []
  const mergedDetailItems = mergeDetailItems(detailItems)
  const summary = summarizeDetailItems(mergedDetailItems, {
    clickPv: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'clickPv'])),
    chatPv: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'chatPv'])),
    favorites: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'favorites'])),
    cost: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'cost'])),
    showPv: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'showPv'])),
    inquiries: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'inquiries'])),
    payPv: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'payPv'])),
    realCost: toNumber(readPath(value, ['idleAdGroupDataVO', 'data', 'realCost'])),
  })
  const adGroupIds = Array.isArray(readPath(value, ['adGroupIds']))
    ? (readPath(value, ['adGroupIds']) as unknown[]).map((item) => String(item)).filter(Boolean)
    : []
  const statusCodeValue = Number(readPath(value, ['status']))

  return {
    campaignId,
    title: String(readPath(value, ['title']) ?? readPath(value, ['campaignName']) ?? '未命名计划'),
    statusCode: Number.isFinite(statusCodeValue) ? statusCodeValue : null,
    adGroupCount: adGroupIds.length,
    adGroupIds,
    clickPv: summary.clickPv,
    chatPv: summary.chatPv,
    favorites: summary.favorites,
    cost: summary.cost,
    showPv: summary.showPv,
    inquiries: summary.inquiries,
    payPv: summary.payPv,
    realCost: summary.realCost,
    detailItems: mergedDetailItems,
  }
}

function latestDetailTime(row: CampaignReportRow) {
  return row.detailItems[row.detailItems.length - 1]?.time ?? ''
}

async function mapWithConcurrency<T, R>(
  items: T[],
  limit: number,
  worker: (item: T) => Promise<R>,
) {
  if (items.length === 0) return [] as R[]

  const results: R[] = new Array(items.length)
  let nextIndex = 0

  async function run() {
    while (nextIndex < items.length) {
      const currentIndex = nextIndex
      nextIndex += 1
      results[currentIndex] = await worker(items[currentIndex])
    }
  }

  const workerCount = Math.max(1, Math.min(limit, items.length))
  await Promise.all(Array.from({ length: workerCount }, () => run()))
  return results
}

async function loadCampaignReportWindow(
  uid: string,
  window: CampaignReportWindow,
  requestFn: CampaignReportRequest,
) {
  const rows: CampaignReportRow[] = []
  let pageNum = 1

  while (true) {
    const payload = {
      ...buildBatchReportPayload(window.startDate, window.endDate, String(REPORT_FETCH_PAGE_SIZE)),
      pageNum,
      pageSize: REPORT_FETCH_PAGE_SIZE,
    }
    const response = assertSuperPolishRequestSuccess(await requestFn(uid, payload))
    const data = unwrapMtopData(response)
    const pageRows = extractCampaignReportRows(data)
    rows.push(...pageRows)

    const total = toNumber(readPath(data, ['total']))
    const totalPage = toNumber(readPath(data, ['totalPage']))
    const loadedAllByTotal = total > 0 && rows.length >= total
    const loadedAllByPage = totalPage > 0 && pageNum >= totalPage

    if (loadedAllByTotal || loadedAllByPage || pageRows.length < REPORT_FETCH_PAGE_SIZE) {
      break
    }

    pageNum += 1
  }

  return rows
}

export function buildCampaignReportWindows(
  startDate: string,
  endDate: string,
  maxWindowDays = REPORT_MAX_WINDOW_DAYS,
): CampaignReportWindow[] {
  if (!startDate || !endDate) return []

  const start = parseInputDate(startDate)
  const end = parseInputDate(endDate)
  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime()) || start > end) {
    return []
  }

  const windows: CampaignReportWindow[] = []
  let cursor = start

  while (cursor <= end) {
    const windowEnd = addDays(cursor, maxWindowDays - 1)
    windows.push({
      startDate: formatInputDate(cursor),
      endDate: formatInputDate(windowEnd < end ? windowEnd : end),
    })
    cursor = addDays(windowEnd, 1)
  }

  return windows
}

export function extractCampaignReportRows(value: unknown): CampaignReportRow[] {
  const result = readPath(value, ['result'])
  if (!Array.isArray(result)) return []

  return result
    .map((item) => normalizeCampaignReportRow(item))
    .filter((item): item is CampaignReportRow => Boolean(item))
}

export function mergeCampaignReportRows(rows: CampaignReportRow[]) {
  const mergedByCampaignId = new Map<string, CampaignReportRow>()

  for (const row of rows) {
    const current = mergedByCampaignId.get(row.campaignId)
    if (!current) {
      mergedByCampaignId.set(row.campaignId, {
        ...row,
        adGroupIds: [...row.adGroupIds],
        detailItems: [...row.detailItems],
      })
      continue
    }

    current.title = current.title || row.title
    current.statusCode = row.statusCode ?? current.statusCode
    current.adGroupIds = Array.from(new Set([...current.adGroupIds, ...row.adGroupIds]))
    current.adGroupCount = Math.max(current.adGroupCount, current.adGroupIds.length, row.adGroupCount)
    current.detailItems = mergeDetailItems([...current.detailItems, ...row.detailItems])

    const summary = summarizeDetailItems(current.detailItems, {
      clickPv: current.clickPv + row.clickPv,
      chatPv: current.chatPv + row.chatPv,
      favorites: current.favorites + row.favorites,
      cost: current.cost + row.cost,
      showPv: current.showPv + row.showPv,
      inquiries: current.inquiries + row.inquiries,
      payPv: current.payPv + row.payPv,
      realCost: current.realCost + row.realCost,
    })

    current.clickPv = summary.clickPv
    current.chatPv = summary.chatPv
    current.favorites = summary.favorites
    current.cost = summary.cost
    current.showPv = summary.showPv
    current.inquiries = summary.inquiries
    current.payPv = summary.payPv
    current.realCost = summary.realCost
  }

  return Array.from(mergedByCampaignId.values()).sort((left, right) => {
    const timeCompare = latestDetailTime(right).localeCompare(latestDetailTime(left))
    if (timeCompare !== 0) return timeCompare
    return right.cost - left.cost
  })
}

export function summarizeCampaignReportRows(rows: CampaignReportRow[]) {
  return rows.reduce<CampaignReportSummary>((summary, row) => ({
    clickPv: summary.clickPv + row.clickPv,
    chatPv: summary.chatPv + row.chatPv,
    favorites: summary.favorites + row.favorites,
    cost: summary.cost + row.cost,
    showPv: summary.showPv + row.showPv,
    inquiries: summary.inquiries + row.inquiries,
    payPv: summary.payPv + row.payPv,
    realCost: summary.realCost + row.realCost,
  }), zeroSummary())
}

export async function loadCampaignReportRange(
  uid: string,
  range: { startDate: string; endDate: string },
  requestFn: CampaignReportRequest = getSuperPolishCampaignReport,
): Promise<CampaignReportRangeData> {
  const windows = buildCampaignReportWindows(range.startDate, range.endDate)
  if (windows.length === 0) {
    return {
      rows: [],
      summary: zeroSummary(),
      windows: [],
    }
  }

  const windowRows = await mapWithConcurrency(
    windows,
    REPORT_WINDOW_CONCURRENCY,
    (window) => loadCampaignReportWindow(uid, window, requestFn),
  )
  const rows = mergeCampaignReportRows(windowRows.flat())

  return {
    rows,
    summary: summarizeCampaignReportRows(rows),
    windows,
  }
}
