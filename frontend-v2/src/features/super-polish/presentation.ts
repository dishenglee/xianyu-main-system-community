import {
  asArray,
  asRecord,
  formatMoney,
  pickNumberString,
  pickString,
  readPath,
  type JsonRecord,
} from './model'
import type { SuperPolishTone } from './components/SuperPolishUi'

export interface SuperPolishMetricItem {
  label: string
  value: string
  helper?: string
  tone?: SuperPolishTone
}

const BID_TARGET_LABELS: Record<string, string> = {
  click: '点击',
  exposure: '曝光',
  follow_collect: '关注收藏',
  chat: '咨询',
  order: '订单量',
}

const BID_TYPE_LABELS: Record<string, string> = {
  smart_bid: '智能出价',
  manual_bid: '手动出价',
}

function readBoolean(value: unknown): boolean | null {
  if (value === true || value === 'true' || value === 1 || value === '1') return true
  if (value === false || value === 'false' || value === 0 || value === '0') return false
  return null
}

function formatBidPrice(value: unknown) {
  const numeric = Number(value)
  if (!Number.isFinite(numeric)) return '待返回'
  return formatMoney(numeric)
}

function bidTypeLabel(value: unknown) {
  const text = String(value ?? '').trim()
  return BID_TYPE_LABELS[text] ?? (text || '未返回')
}

export function bidTargetLabel(value: unknown) {
  const text = String(value ?? '').trim()
  return BID_TARGET_LABELS[text] ?? (text || '未命名目标')
}

export function buildBooleanMetric({
  label,
  value,
  whenTrue,
  whenFalse,
  whenUnknown = '待确认',
  helperWhenTrue,
  helperWhenFalse,
  helperWhenUnknown,
}: {
  label: string
  value: unknown
  whenTrue: string
  whenFalse: string
  whenUnknown?: string
  helperWhenTrue?: string
  helperWhenFalse?: string
  helperWhenUnknown?: string
}): SuperPolishMetricItem {
  const state = readBoolean(value)
  if (state === true) {
    return { label, value: whenTrue, helper: helperWhenTrue, tone: 'success' }
  }
  if (state === false) {
    return { label, value: whenFalse, helper: helperWhenFalse, tone: 'warning' }
  }
  return { label, value: whenUnknown, helper: helperWhenUnknown, tone: 'neutral' }
}

export function buildPrecheckMetrics(data: unknown): SuperPolishMetricItem[] {
  const record = asRecord(data)
  const extraFunctions = asRecord(record.extraFunctions)
  const grayFunctions = asArray(record.grayFuncList).map((value) => String(value)).filter(Boolean)
  return [
    buildBooleanMetric({
      label: '账号策略',
      value: record.oldMember,
      whenTrue: '老客投放策略',
      whenFalse: '标准投放策略',
      whenUnknown: '待校验',
      helperWhenTrue: '创建请求会自动沿用老客账号的计划策略。',
      helperWhenFalse: '当前账号按普通计划策略发起。',
      helperWhenUnknown: '预检查尚未返回账号策略。',
    }),
    buildBooleanMetric({
      label: '智能投放能力',
      value: record.hasOcpmWhitelist ?? (grayFunctions.length > 0),
      whenTrue: '可用',
      whenFalse: '暂未开放',
      whenUnknown: '待确认',
      helperWhenTrue: grayFunctions.length > 0
        ? `当前账号可继续评估 ${grayFunctions.join(' / ')} 等智能策略。`
        : '当前账号可以继续评估智能投放方案。',
      helperWhenFalse: '当前账号暂不能直接使用更完整的智能投放策略。',
      helperWhenUnknown: '预检查没有返回智能投放状态。',
    }),
    buildBooleanMetric({
      label: '创意优化',
      value: extraFunctions.showCreativeOptSwitch,
      whenTrue: '已开放',
      whenFalse: '暂未开放',
      whenUnknown: '待确认',
      helperWhenTrue: '套餐和出价建议里可继续沿用创意优化能力。',
      helperWhenFalse: '当前账号暂不能直接使用创意优化。',
      helperWhenUnknown: '预检查没有返回创意优化状态。',
    }),
    buildBooleanMetric({
      label: '新客扶持',
      value: record.showNewUserSupport,
      whenTrue: '已开通',
      whenFalse: '当前未开通',
      whenUnknown: '待确认',
      helperWhenTrue: '当前账号返回了新客扶持能力，可在创建后继续关注对应优惠。',
      helperWhenFalse: '当前账号没有返回新客扶持能力。',
      helperWhenUnknown: '预检查没有返回新客扶持状态。',
    }),
  ]
}

export function buildCreateResultMetrics(data: unknown): SuperPolishMetricItem[] {
  const record = asRecord(data)
  const campaignIds = asArray(record.campaignIds).map((value) => String(value)).filter(Boolean)
  return [
    buildBooleanMetric({
      label: '创建结果',
      value: record.createSuccess,
      whenTrue: '创建成功',
      whenFalse: '创建未完成',
      whenUnknown: '待返回',
      helperWhenTrue: campaignIds.length > 0 ? `已生成 ${campaignIds.length} 个计划。` : '创建请求已提交。',
      helperWhenFalse: '请稍后刷新计划列表确认创建状态。',
    }),
    {
      label: '计划数量',
      value: String(campaignIds.length),
      helper: campaignIds.length > 0 ? `计划 ID：${campaignIds.join(' / ')}` : '当前没有返回计划 ID。',
      tone: campaignIds.length > 0 ? 'info' : 'neutral',
    },
    buildBooleanMetric({
      label: '押金处理',
      value: record.needGameRecycleDeposit,
      whenTrue: '需要处理',
      whenFalse: '无需处理',
      whenUnknown: '未返回',
      helperWhenTrue: '创建后还需要处理押金相关事项。',
      helperWhenFalse: '当前创建结果没有额外押金动作。',
    }),
  ]
}

export function packageRecommendationMetrics(data: unknown): SuperPolishMetricItem[] {
  return asArray<JsonRecord>(asRecord(data).packageTargetVOList).map((row) => {
    const orders = asArray<JsonRecord>(row.orderVOList)
    const firstOrder = asRecord(orders[0])
    const creativeState = readBoolean(asRecord(firstOrder.extraFunctions).showCreativeOptSwitch)
    return {
      label: `${bidTargetLabel(row.bidTarget)}套餐`,
      value: formatBidPrice(readPath(firstOrder, ['bidPrice'])),
      helper: [
        orders.length > 0 ? `候选 ${orders.length} 档` : '',
        bidTypeLabel(firstOrder.bidType),
        creativeState === true ? '支持创意优化' : creativeState === false ? '不支持创意优化' : '',
      ].filter(Boolean).join(' · ') || '暂无更多套餐说明',
      tone: orders.length > 0 ? 'info' : 'neutral',
    }
  })
}

export function bidEstimateMetrics(data: unknown): SuperPolishMetricItem[] {
  return asArray<JsonRecord>(asRecord(data).customizeBidRecommendVOList).map((row) => {
    const customBidPrice = Number(readPath(row, ['customBidPrice']))
    return {
      label: `${bidTargetLabel(row.bidTarget)}出价`,
      value: formatBidPrice(readPath(row, ['bidPrice'])),
      helper: Number.isFinite(customBidPrice)
        ? `自定义最高可参考 ${formatMoney(customBidPrice)} · ${bidTypeLabel(row.bidType)}`
        : bidTypeLabel(row.bidType),
      tone: 'success',
    }
  })
}

export function campaignStatusMeta(status: unknown): { label: string; tone: SuperPolishTone } {
  const label = String(status ?? '').trim() || '未返回'
  if (/暂停|冻结|不足/.test(label)) return { label, tone: 'warning' }
  if (/失败|异常|关闭/.test(label)) return { label, tone: 'danger' }
  if (/投放|进行|开启|生效/.test(label)) return { label, tone: 'success' }
  return { label, tone: 'neutral' }
}

export function reportSummaryMetrics(summary: unknown): SuperPolishMetricItem[] {
  const record = asRecord(summary)
  if (Object.keys(record).length === 0) return []
  return [
    { label: '点击量', value: pickNumberString(record, [['clickPv']], '0') },
    { label: '咨询量', value: pickNumberString(record, [['chatPv']], '0') },
    { label: '收藏量', value: pickNumberString(record, [['favorites']], '0') },
    { label: '订单量', value: pickNumberString(record, [['orderInquiries']], '0') },
    { label: '消耗', value: formatMoney(readPath(record, ['cost']) ?? 0), tone: 'info' },
    { label: '点击成本', value: formatMoney(readPath(record, ['costPerClick']) ?? 0) },
  ]
}

export function reportRowTitle(row: unknown) {
  return pickString(
    row,
    [['itemTitle'], ['campaignName'], ['groupName'], ['title'], ['name'], ['time']],
    '未命名记录',
  )
}

export function reportRowIdentity(row: unknown) {
  return pickString(
    row,
    [['itemId'], ['campaignId'], ['adGroupId'], ['groupId'], ['id'], ['time']],
    '',
  )
}

export function sumNumber(rows: JsonRecord[], paths: string[][]) {
  return rows.reduce((total, row) => {
    for (const path of paths) {
      const value = Number(readPath(row, path))
      if (Number.isFinite(value)) return total + value
    }
    return total
  }, 0)
}
