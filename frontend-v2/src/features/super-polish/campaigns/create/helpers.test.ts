import { describe, expect, it } from 'vitest'
import {
  bidTargetLabel as createBidTargetLabel,
  buildBidTargetOrder,
  type SuperPolishBidEstimate,
  type SuperPolishPackageGroup,
} from './helpers'
import { bidTargetLabel as presentationBidTargetLabel, reportSummaryMetrics } from '../../presentation'

describe('super polish create helpers', () => {
  it('renders the official order bid target as a Chinese business label', () => {
    expect(createBidTargetLabel('order')).toBe('订单量')
    expect(presentationBidTargetLabel('order')).toBe('订单量')
  })

  it('keeps order in the selectable bid target list instead of dropping it as an unknown key', () => {
    const groups = [
      { bidTarget: 'order', label: '订单量', recommendPackageType: '', options: [] },
      { bidTarget: 'chat', label: '咨询量', recommendPackageType: '', options: [] },
    ] satisfies SuperPolishPackageGroup[]
    const estimates = [
      {
        bidTarget: 'click',
        label: '浏览量',
        bidPrice: null,
        customBidPrice: null,
        bidType: 'smart_bid',
        targetRecommendBudget: null,
        recommendBudgetList: [],
        creativeOptimizationAvailable: false,
        raw: {},
      },
    ] satisfies SuperPolishBidEstimate[]

    expect(buildBidTargetOrder(groups, estimates, '')).toEqual(['click', 'chat', 'order'])
    expect(buildBidTargetOrder(groups, estimates, 'order')[0]).toBe('order')
  })

  it('uses the same order label for report summary metrics', () => {
    const metrics = reportSummaryMetrics({ orderInquiries: 3 })
    const legacyOrderLabel = ['成交', '咨询量'].join('')
    expect(metrics.some((item) => item.label === '订单量' && item.value === '3')).toBe(true)
    expect(metrics.some((item) => item.label === legacyOrderLabel)).toBe(false)
  })
})
