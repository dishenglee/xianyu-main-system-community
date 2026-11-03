import { pickNumberString, pickString, readPath } from '../model'

export type CampaignMetricKey = 'cost' | 'clickPv' | 'chatPv' | 'favorites'

const STATUS_PATHS = [['statusDescTitle'], ['statusDesc'], ['status']]

function metricPaths(metric: CampaignMetricKey): string[][] {
  return [
    ['idleAdGroupDataVO', 'data', metric],
    ['adDataDetailVO', metric],
    ['todayAdDataDetail', metric],
    ['adData', metric],
  ]
}

export function campaignMetricValuePaths(metric: CampaignMetricKey) {
  return metricPaths(metric)
}

export function campaignMetricNumber(source: unknown, metric: CampaignMetricKey) {
  for (const path of metricPaths(metric)) {
    const value = Number(readPath(source, path))
    if (Number.isFinite(value)) return value
  }
  return 0
}

export function campaignMetricText(source: unknown, metric: CampaignMetricKey, fallback = '0') {
  return pickNumberString(source, metricPaths(metric), fallback)
}

export function campaignStatusText(source: unknown, fallback = '未返回') {
  return pickString(source, STATUS_PATHS, fallback)
}
