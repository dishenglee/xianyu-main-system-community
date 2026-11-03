type MetricKind = 'amount' | 'count' | 'percent'

export interface FishShopMetric {
  key: string
  label: string
  valueText: string
  comparison: string
}

export interface FishShopRankMetric {
  key: string
  label: string
  valueText: string
}

export interface FishShopRankItem {
  key: string
  title: string
  metrics: FishShopRankMetric[]
}

interface MetricDefinition {
  key: string
  label: string
  kind: MetricKind
  aliases: string[]
}

interface RawMetric {
  name?: unknown
  label?: unknown
  value?: unknown
  display?: unknown
  ratio?: unknown
}

const METRIC_DEFINITIONS: MetricDefinition[] = [
  {
    key: 'payAmount', label: '成交金额', kind: 'amount',
    aliases: ['payAmt', 'payAmount', 'tradeAmount', 'dealAmount', 'gmv', '成交金额', '支付金额'],
  },
  {
    key: 'payOrder', label: '成交订单', kind: 'count',
    aliases: ['payOrderCnt', 'payOrderCount', 'payCnt', 'orderCnt', 'orderCount', 'tradeCnt', 'dealCnt', '成交订单', '支付订单数', '成交单量'],
  },
  {
    key: 'payBuyer', label: '成交买家', kind: 'count',
    aliases: ['payBuyerCnt', 'payBuyerCount', 'buyerCnt', 'buyerCount', '成交买家', '支付买家数'],
  },
  {
    key: 'exposure', label: '曝光量', kind: 'count',
    aliases: ['exposureCnt', 'exposureCount', 'impressionCnt', 'impressionCount', 'impressions', 'expoCnt', '曝光量'],
  },
  {
    key: 'browse', label: '浏览量', kind: 'count',
    aliases: ['browseCnt', 'browseCount', 'viewCnt', 'viewCount', 'pageView', 'pv', '浏览量'],
  },
  {
    key: 'click', label: '点击量', kind: 'count',
    aliases: ['clickCnt', 'clickCount', 'clickNum', '点击量'],
  },
  {
    key: 'visitor', label: '访客数', kind: 'count',
    aliases: ['visitorCnt', 'visitorCount', 'visitorUv', 'visitorNum', 'uv', '访客数'],
  },
  {
    key: 'want', label: '想要数', kind: 'count',
    aliases: ['wantCnt', 'wantCount', 'wishCnt', 'wishCount', 'likeCnt', '想要数'],
  },
  {
    key: 'favorite', label: '收藏数', kind: 'count',
    aliases: ['collectCnt', 'collectCount', 'favoriteCnt', 'favoriteCount', 'favCnt', 'collectionCnt', '收藏数'],
  },
  {
    key: 'consult', label: '咨询数', kind: 'count',
    aliases: ['consultCnt', 'consultCount', 'askCnt', 'askCount', 'inquiryCnt', '咨询数'],
  },
  {
    key: 'item', label: '在售商品', kind: 'count',
    aliases: ['itemCnt', 'itemCount', 'goodsCnt', '商品数', '在售商品'],
  },
  {
    key: 'refundAmount', label: '退款金额', kind: 'amount',
    aliases: ['refundAmt', 'refundAmount', '退款金额'],
  },
  {
    key: 'refund', label: '退款订单', kind: 'count',
    aliases: ['refundCnt', 'refundCount', '退款笔数', '退款订单'],
  },
  {
    key: 'replyRate', label: '客服回复率', kind: 'percent',
    aliases: ['csReplyRate', 'replyRate', '客服回复率'],
  },
]

const SUMMARY_ORDER = [
  'payAmount', 'payOrder', 'payBuyer', 'visitor', 'browse', 'exposure', 'click',
  'consult', 'want', 'favorite', 'item', 'refundAmount', 'refund', 'replyRate',
]
const FLOW_ORDER = ['exposure', 'browse', 'click', 'visitor', 'favorite', 'want', 'consult', 'payOrder', 'payAmount']
const ITEM_ORDER = ['browse', 'visitor', 'click', 'want', 'favorite', 'consult', 'payOrder', 'payAmount']

const definitionByKey = new Map(METRIC_DEFINITIONS.map((definition) => [definition.key, definition] as const))

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {}
}

function normalizeKey(value: unknown): string {
  return String(value ?? '').trim().toLowerCase().replace(/[\s_\-./]/g, '')
}

function hasValue(value: unknown): boolean {
  return value !== undefined
    && value !== null
    && String(value).trim() !== ''
    && !['-', '—', '--', 'null', 'undefined'].includes(String(value).trim().toLowerCase())
}

function numberFrom(value: unknown): number | null {
  if (typeof value === 'number') return Number.isFinite(value) ? value : null
  if (!hasValue(value)) return null
  const source = String(value).trim().replace(/[￥¥,\s]/g, '')
  const multiplier = source.includes('亿') ? 100000000 : source.includes('万') ? 10000 : 1
  const parsed = Number.parseFloat(source.replace(/[亿万%]/g, ''))
  return Number.isFinite(parsed) ? parsed * multiplier : null
}

function formatNumber(value: unknown): string {
  const numeric = numberFrom(value)
  if (numeric === null) return '—'
  return new Intl.NumberFormat('zh-CN', {
    maximumFractionDigits: Number.isInteger(numeric) ? 0 : 2,
  }).format(numeric)
}

function formatAmount(value: unknown): string {
  const numeric = numberFrom(value)
  if (numeric === null) return '—'
  return new Intl.NumberFormat('zh-CN', {
    style: 'currency',
    currency: 'CNY',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(numeric)
}

function formatPercent(value: unknown): string {
  const numeric = numberFrom(value)
  if (numeric === null) return '—'
  const percent = Math.abs(numeric) <= 1 && !String(value).includes('%') ? numeric * 100 : numeric
  return `${new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 }).format(percent)}%`
}

function formatValue(value: unknown, kind: MetricKind): string {
  if (kind === 'amount') return formatAmount(value)
  if (kind === 'percent') return formatPercent(value)
  return formatNumber(value)
}

function formatComparison(value: unknown): string {
  const numeric = numberFrom(value)
  if (numeric === null) return '暂无上期对比'
  const percent = Math.abs(numeric) <= 1 && !String(value).includes('%') ? numeric * 100 : numeric
  const text = `${new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 }).format(Math.abs(percent))}%`
  if (percent > 0) return `较上期上涨 ${text}`
  if (percent < 0) return `较上期下降 ${text}`
  return '与上期持平'
}

function metricRows(payload: unknown): RawMetric[] {
  const root = asRecord(payload)
  const data = asRecord(root.data ?? root)
  const normalized = asRecord(data.normalized)
  return Array.isArray(normalized.metrics) ? normalized.metrics.map(asRecord) : []
}

function readMetricValue(metric: RawMetric): unknown {
  return metric.value ?? metric.display
}

function metricForDefinition(metrics: RawMetric[], definition: MetricDefinition): RawMetric | undefined {
  for (const alias of definition.aliases) {
    const normalizedAlias = normalizeKey(alias)
    const namedMetric = metrics.find((metric) => (
      normalizeKey(metric.name) === normalizedAlias && hasValue(readMetricValue(metric))
    ))
    if (namedMetric) return namedMetric

    const labeledMetric = metrics.find((metric) => (
      normalizeKey(metric.label) === normalizedAlias && hasValue(readMetricValue(metric))
    ))
    if (labeledMetric) return labeledMetric
  }
  return undefined
}

function presentMetrics(payload: unknown, order: string[]): FishShopMetric[] {
  const metrics = metricRows(payload)
  return order.flatMap((key) => {
    const definition = definitionByKey.get(key)
    const metric = definition ? metricForDefinition(metrics, definition) : undefined
    if (!definition || !metric) return []
    return [{
      key,
      label: definition.label,
      valueText: formatValue(readMetricValue(metric), definition.kind),
      comparison: formatComparison(metric.ratio),
    }]
  })
}

function rankingRows(payload: unknown): Record<string, unknown>[] {
  const root = asRecord(payload)
  const data = asRecord(root.data ?? root)
  const result = asRecord(data.result)
  const candidates = [data.list, data.items, data.promotionList, data.packageList, result.list, result.items]
  return candidates.find(Array.isArray)?.map(asRecord) ?? []
}

function readRowMetric(row: Record<string, unknown>, definition: MetricDefinition): unknown {
  const rowKeys = Object.keys(row).sort()
  for (const alias of definition.aliases) {
    const key = rowKeys.find((candidate) => normalizeKey(candidate) === normalizeKey(alias))
    if (key && hasValue(row[key])) return row[key]
  }

  for (const collection of [row.metrics, row.indicators, row.metricList]) {
    if (!Array.isArray(collection)) continue
    const entries = collection.map(asRecord)
    for (const alias of definition.aliases) {
      const normalizedAlias = normalizeKey(alias)
      const namedEntry = entries.find((entry) => (
        normalizeKey(entry.name) === normalizedAlias && hasValue(entry.value ?? entry.display)
      ))
      if (namedEntry) return namedEntry.value ?? namedEntry.display

      const labeledEntry = entries.find((entry) => (
        normalizeKey(entry.label) === normalizedAlias && hasValue(entry.value ?? entry.display)
      ))
      if (labeledEntry) return labeledEntry.value ?? labeledEntry.display
    }
  }
  return undefined
}

function rowTitle(row: Record<string, unknown>): string {
  for (const key of ['title', 'itemTitle', 'itemName', 'name', 'itemId', 'id']) {
    if (hasValue(row[key])) return String(row[key]).trim()
  }
  return '未命名商品'
}

function rowIdentifier(row: Record<string, unknown>): string {
  for (const key of ['itemId', 'id']) {
    if (hasValue(row[key])) return String(row[key]).trim()
  }
  return rowTitle(row)
}

export function presentFishShopSummary(payload: unknown): FishShopMetric[] {
  return presentMetrics(payload, SUMMARY_ORDER).slice(0, 8)
}

export function presentFishShopFlow(payload: unknown): FishShopMetric[] {
  return presentMetrics(payload, FLOW_ORDER)
}

export function presentFishShopItemRanking(payload: unknown): FishShopRankItem[] {
  const usedKeys = new Map<string, number>()
  return rankingRows(payload)
    .map((row) => ({
      key: rowIdentifier(row),
      title: rowTitle(row),
      metrics: ITEM_ORDER.flatMap((key) => {
        const definition = definitionByKey.get(key)
        if (!definition) return []
        const value = readRowMetric(row, definition)
        if (!hasValue(value)) return []
        return [{ key, label: definition.label, valueText: formatValue(value, definition.kind) }]
      }),
    }))
    .filter((item) => item.metrics.length > 0)
    .map((item) => {
      const occurrence = (usedKeys.get(item.key) ?? 0) + 1
      usedKeys.set(item.key, occurrence)
      return occurrence === 1 ? item : { ...item, key: `${item.key}-${occurrence}` }
    })
}
