import { request } from './client'
import type {
  DashboardStats,
  DataCenterAccountRow,
  DataCenterAutomationMetric,
  DataCenterDataCompassSummary,
  DataCenterQueueItem,
  DataCenterShareBundle,
  DataCenterShareRange,
  DataCenterSellerTrendBundle,
  DataCenterSellerTrendRange,
  DataCenterStats,
  DataCenterTotals,
  DataCenterTrendBundle,
  DataCenterTrendRange,
} from './types'

// ==================== Dashboard stats ====================

const fallbackDashboardStats = (): DashboardStats => ({
  date: '',
  generatedAt: null,
  todayMessages: 0,
  unreadMessages: 0,
  unreadSessions: 0,
  overdueReplySessions: 0,
  todayOrders: 0,
  todayRevenue: 0,
  pendingPayOrders: 0,
  waitSendOrders: 0,
  waitReceiveOrders: 0,
  closedOrders: 0,
  accountTotal: 0,
  activeAccounts: 0,
  inactiveAccounts: 0,
  restrictedAccounts: 0,
  sessionErrorAccounts: 0,
  accountRows: [],
  recentEvents: [],
  riskItems: [],
  trend7d: [],
})

const numberOrZero = (value: unknown) => {
  const n = Number(value)
  return Number.isFinite(n) ? n : 0
}

const optionalNumber = (value: unknown) => {
  if (value === null || value === undefined) return value
  const n = Number(value)
  return Number.isFinite(n) ? n : null
}

const safeDisplayName = (uid: unknown, value: unknown, fallback = '未识别账号') => {
  const id = String(uid || '').trim()
  const name = String(value || '').trim()
  if (!name || name === id || name.toLowerCase() === 'unknown' || /^\d+$/.test(name)) return fallback
  return name
}

const normalizeDashboardStats = (raw: Partial<DashboardStats> | null | undefined): DashboardStats => {
  const base = fallbackDashboardStats()
  const data = raw || {}
  return {
    ...base,
    ...data,
    todayMessages: numberOrZero(data.todayMessages),
    unreadMessages: numberOrZero(data.unreadMessages),
    unreadSessions: numberOrZero(data.unreadSessions),
    overdueReplySessions: numberOrZero(data.overdueReplySessions),
    todayOrders: numberOrZero(data.todayOrders),
    todayRevenue: numberOrZero(data.todayRevenue),
    pendingPayOrders: numberOrZero(data.pendingPayOrders),
    waitSendOrders: numberOrZero(data.waitSendOrders),
    waitReceiveOrders: numberOrZero(data.waitReceiveOrders),
    closedOrders: numberOrZero(data.closedOrders),
    accountTotal: numberOrZero(data.accountTotal),
    activeAccounts: numberOrZero(data.activeAccounts),
    inactiveAccounts: numberOrZero(data.inactiveAccounts),
    restrictedAccounts: numberOrZero(data.restrictedAccounts),
    sessionErrorAccounts: numberOrZero(data.sessionErrorAccounts),
    accountRows: Array.isArray(data.accountRows) ? data.accountRows : [],
    recentEvents: Array.isArray(data.recentEvents) ? data.recentEvents : [],
    riskItems: Array.isArray(data.riskItems) ? data.riskItems : [],
    trend7d: Array.isArray(data.trend7d) ? data.trend7d : [],
  }
}

export const getDashboardStats = async (): Promise<DashboardStats> => {
  try {
    const data = await request<DashboardStats>('GET', '/api/stats/today')
    return normalizeDashboardStats(data)
  } catch (error) {
    console.warn('[dashboard] failed to load /api/stats/today, using fallback stats', error)
    return fallbackDashboardStats()
  }
}

const fallbackTotals = (): DataCenterTotals => ({
  todayRevenue: 0,
  todayOrders: 0,
  waitSendOrders: 0,
  waitSendEarliestAt: 0,
  pendingPayOrders: 0,
  pendingPayAmount: 0,
  unreadSessions: 0,
  unreadMessages: 0,
  overdueReplySessions: 0,
  onlineItems: 0,
  totalItems: 0,
  aftersaleOrders: 0,
  closedOrders: 0,
  accountTotal: 0,
  activeAccounts: 0,
  liveOnlineAccounts: 0,
  sessionErrorAccounts: 0,
})

const fallbackShareRange = (range: DataCenterShareRange['range'], label: string): DataCenterShareRange => ({
  range,
  label,
  totalAmount: 0,
  totalOrders: 0,
  segments: [],
})

const fallbackTrendRange = (range: DataCenterTrendRange['range'], label: string): DataCenterTrendRange => ({
  range,
  label,
  totalAmount: 0,
  totalOrders: 0,
  points: [],
})

const fallbackShare = (): DataCenterShareBundle => ({
  day: fallbackShareRange('day', '今日成交'),
  week: fallbackShareRange('week', '7 天成交'),
  total: fallbackShareRange('total', '总成交'),
})

const fallbackTrend = (): DataCenterTrendBundle => ({
  week: fallbackTrendRange('week', '7 天成交'),
  month: fallbackTrendRange('month', '30 天成交'),
  total: fallbackTrendRange('total', '总成交'),
})

const fallbackSellerTrendRange = (range: DataCenterSellerTrendRange['range'], label: string): DataCenterSellerTrendRange => ({
  available: false,
  range,
  label,
  totalAmount: 0,
  totalOrders: 0,
  points: [],
  syncedAt: 0,
})

const fallbackSellerTrend = (): DataCenterSellerTrendBundle => ({
  week: fallbackSellerTrendRange('week', '近 7 天'),
  halfMonth: fallbackSellerTrendRange('halfMonth', '近 15 天'),
  month: fallbackSellerTrendRange('month', '近 30 天'),
})

const fallbackDataCompass = (): DataCenterDataCompassSummary => ({
  status: 'pending',
  accountCount: 0,
  successCount: 0,
  failedCount: 0,
  lastSuccessAt: 0,
  errorSummary: '',
})

const fallbackDataCenterStats = (): DataCenterStats => ({
  date: '',
  generatedAt: 0,
  totals: fallbackTotals(),
  accounts: [],
  queue: [],
  share: fallbackShare(),
  trend: fallbackTrend(),
  sellerTrend: fallbackSellerTrend(),
  sync: {
    status: 'unknown',
    subtitle: '等待本地数据同步',
    sqliteOk: false,
    liveOnline: 0,
    liveTotal: 0,
    staleAccounts: 0,
    signServiceOk: false,
    signServiceStatusCode: 0,
    licenseServiceOk: false,
    licenseServiceStatusCode: 0,
    dataCompass: fallbackDataCompass(),
  },
})

const normalizeAccount = (row: Partial<DataCenterAccountRow> | null | undefined): DataCenterAccountRow => ({
  uid: String(row?.uid || ''),
  nick: safeDisplayName(row?.uid, row?.nick),
  avatar: String(row?.avatar || ''),
  status: String(row?.status || 'unknown'),
  liveStatus: String(row?.liveStatus || ''),
  lastError: String(row?.lastError || ''),
  todayRevenue: numberOrZero(row?.todayRevenue),
  weekRevenue: numberOrZero(row?.weekRevenue),
  totalRevenue: numberOrZero(row?.totalRevenue),
  todayOrders: numberOrZero(row?.todayOrders),
  weekOrders: numberOrZero(row?.weekOrders),
  totalOrders: numberOrZero(row?.totalOrders),
  unreadMessages: numberOrZero(row?.unreadMessages),
  unreadSessions: numberOrZero(row?.unreadSessions),
  waitSendOrders: numberOrZero(row?.waitSendOrders),
  pendingPayOrders: numberOrZero(row?.pendingPayOrders),
  onlineItems: numberOrZero(row?.onlineItems),
  totalItems: numberOrZero(row?.totalItems),
  lastMessageAt: numberOrZero(row?.lastMessageAt),
  lastOrderAt: numberOrZero(row?.lastOrderAt),
  itemsSyncedAt: numberOrZero(row?.itemsSyncedAt),
  ordersSyncedAt: numberOrZero(row?.ordersSyncedAt),
})

const normalizeQueueItem = (item: Partial<DataCenterQueueItem> | null | undefined): DataCenterQueueItem => ({
  id: String(item?.id || `${item?.kind || 'item'}:${item?.uid || ''}:${item?.ts || ''}`),
  kind: String(item?.kind || 'order'),
  uid: String(item?.uid || ''),
  accountNick: safeDisplayName(item?.uid, item?.accountNick),
  title: String(item?.title || '待处理事项'),
  subtitle: String(item?.subtitle || ''),
  customer: String(item?.customer || '客户未知'),
  amount: numberOrZero(item?.amount),
  status: String(item?.status || ''),
  statusLabel: String(item?.statusLabel || '待处理'),
  targetPath: String(item?.targetPath || '/orders'),
  actionLabel: String(item?.actionLabel || '详情'),
  ts: numberOrZero(item?.ts),
  itemId: String(item?.itemId || ''),
  itemPic: String(item?.itemPic || ''),
  sessionId: String(item?.sessionId || ''),
  orderId: String(item?.orderId || ''),
})

const normalizeShareRange = (
  raw: Partial<DataCenterShareRange> | null | undefined,
  fallback: DataCenterShareRange,
): DataCenterShareRange => ({
  ...fallback,
  ...(raw || {}),
  totalAmount: numberOrZero(raw?.totalAmount),
  totalOrders: numberOrZero(raw?.totalOrders),
  segments: Array.isArray(raw?.segments)
    ? raw.segments.map((segment) => ({
        uid: String(segment?.uid || ''),
        nick: safeDisplayName(segment?.uid, segment?.nick, '账号'),
        amount: numberOrZero(segment?.amount),
        orders: numberOrZero(segment?.orders),
        percent: numberOrZero(segment?.percent),
      }))
    : [],
})

const normalizeTrendRange = (
  raw: Partial<DataCenterTrendRange> | null | undefined,
  fallback: DataCenterTrendRange,
): DataCenterTrendRange => ({
  ...fallback,
  ...(raw || {}),
  totalAmount: numberOrZero(raw?.totalAmount),
  totalOrders: numberOrZero(raw?.totalOrders),
  points: Array.isArray(raw?.points)
    ? raw.points.map((point) => ({
        date: String(point?.date || ''),
        label: String(point?.label || point?.date || ''),
        amount: numberOrZero(point?.amount),
        orders: numberOrZero(point?.orders),
        messages: numberOrZero(point?.messages),
      }))
    : [],
})

const normalizeSellerTrendRange = (
  raw: Partial<DataCenterSellerTrendRange> | null | undefined,
  fallback: DataCenterSellerTrendRange,
): DataCenterSellerTrendRange => ({
  ...fallback,
  ...(raw || {}),
  available: raw?.available === true,
  totalAmount: numberOrZero(raw?.totalAmount),
  totalOrders: numberOrZero(raw?.totalOrders),
  syncedAt: numberOrZero(raw?.syncedAt),
  points: Array.isArray(raw?.points)
    ? raw.points.map((point) => ({
        date: String(point?.date || ''),
        label: String(point?.label || point?.date || ''),
        amount: numberOrZero(point?.amount),
        orders: numberOrZero(point?.orders),
        messages: numberOrZero(point?.messages),
      }))
    : [],
})

const normalizeAutomationMetric = (raw: Partial<DataCenterAutomationMetric> | null | undefined): DataCenterAutomationMetric => ({
  enabled: optionalNumber(raw?.enabled),
  running: optionalNumber(raw?.running),
  pending: optionalNumber(raw?.pending),
  errors: optionalNumber(raw?.errors),
})

const normalizeAutomation = (raw: DataCenterStats['automation']): NonNullable<DataCenterStats['automation']> => ({
  delivery: normalizeAutomationMetric(raw?.delivery),
  scheduledTasks: normalizeAutomationMetric(raw?.scheduledTasks),
  replies: normalizeAutomationMetric(raw?.replies),
})

const normalizeDataCompass = (
  raw: Partial<DataCenterDataCompassSummary> | null | undefined,
): DataCenterDataCompassSummary => {
  const fallback = fallbackDataCompass()
  return {
    status: String(raw?.status || fallback.status).trim().toLowerCase() || fallback.status,
    accountCount: numberOrZero(raw?.accountCount),
    successCount: numberOrZero(raw?.successCount),
    failedCount: numberOrZero(raw?.failedCount),
    lastSuccessAt: numberOrZero(raw?.lastSuccessAt),
    errorSummary: typeof raw?.errorSummary === 'string' ? raw.errorSummary.trim() : '',
  }
}

const normalizeDataCenterStats = (raw: Partial<DataCenterStats> | null | undefined): DataCenterStats => {
  const base = fallbackDataCenterStats()
  const data = raw || {}
  const shareBase = fallbackShare()
  const trendBase = fallbackTrend()
  const sellerTrendBase = fallbackSellerTrend()
  return {
    date: String(data.date || ''),
    generatedAt: numberOrZero(data.generatedAt),
    totals: {
      ...base.totals,
      ...(data.totals || {}),
      todayRevenue: numberOrZero(data.totals?.todayRevenue),
      todayOrders: numberOrZero(data.totals?.todayOrders),
      waitSendOrders: numberOrZero(data.totals?.waitSendOrders),
      waitSendEarliestAt: numberOrZero(data.totals?.waitSendEarliestAt),
      pendingPayOrders: numberOrZero(data.totals?.pendingPayOrders),
      pendingPayAmount: numberOrZero(data.totals?.pendingPayAmount),
      unreadSessions: numberOrZero(data.totals?.unreadSessions),
      unreadMessages: numberOrZero(data.totals?.unreadMessages),
      overdueReplySessions: numberOrZero(data.totals?.overdueReplySessions),
      onlineItems: numberOrZero(data.totals?.onlineItems),
      totalItems: numberOrZero(data.totals?.totalItems),
      aftersaleOrders: numberOrZero(data.totals?.aftersaleOrders),
      closedOrders: numberOrZero(data.totals?.closedOrders),
      accountTotal: numberOrZero(data.totals?.accountTotal),
      activeAccounts: numberOrZero(data.totals?.activeAccounts),
      liveOnlineAccounts: numberOrZero(data.totals?.liveOnlineAccounts),
      sessionErrorAccounts: numberOrZero(data.totals?.sessionErrorAccounts),
    },
    accounts: Array.isArray(data.accounts) ? data.accounts.map(normalizeAccount).filter((row) => row.uid) : [],
    queue: Array.isArray(data.queue) ? data.queue.map(normalizeQueueItem) : [],
    share: {
      day: normalizeShareRange(data.share?.day, shareBase.day),
      week: normalizeShareRange(data.share?.week, shareBase.week),
      total: normalizeShareRange(data.share?.total, shareBase.total),
    },
    trend: {
      week: normalizeTrendRange(data.trend?.week, trendBase.week),
      month: normalizeTrendRange(data.trend?.month, trendBase.month),
      total: normalizeTrendRange(data.trend?.total, trendBase.total),
    },
    sellerTrend: {
      week: normalizeSellerTrendRange(data.sellerTrend?.week, sellerTrendBase.week),
      halfMonth: normalizeSellerTrendRange(data.sellerTrend?.halfMonth, sellerTrendBase.halfMonth),
      month: normalizeSellerTrendRange(data.sellerTrend?.month, sellerTrendBase.month),
    },
    ...(data.automation ? { automation: normalizeAutomation(data.automation) } : {}),
    sync: {
      status: String(data.sync?.status || base.sync.status),
      subtitle: String(data.sync?.subtitle || base.sync.subtitle),
      sqliteOk: Boolean(data.sync?.sqliteOk),
      liveOnline: numberOrZero(data.sync?.liveOnline),
      liveTotal: numberOrZero(data.sync?.liveTotal),
      staleAccounts: numberOrZero(data.sync?.staleAccounts),
      signServiceOk: Boolean(data.sync?.signServiceOk),
      signServiceStatusCode: numberOrZero(data.sync?.signServiceStatusCode),
      licenseServiceOk: Boolean(data.sync?.licenseServiceOk),
      licenseServiceStatusCode: numberOrZero(data.sync?.licenseServiceStatusCode),
      dataCompass: normalizeDataCompass(data.sync?.dataCompass),
    },
  }
}

export const getDataCenterStats = async (options: { trendUid?: string; refresh?: boolean } = {}): Promise<DataCenterStats> => {
  const query = new URLSearchParams()
  if (options.trendUid) query.set('trendUid', options.trendUid)
  if (options.refresh) query.set('refresh', 'true')
  const suffix = query.size ? `?${query.toString()}` : ''
  const data = await request<DataCenterStats>('GET', `/api/stats/data-center${suffix}`)
  return normalizeDataCenterStats(data)
}
