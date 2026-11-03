import { describe, expect, it } from 'vitest'

import {
  buildExposureDetailQueryFromHistory,
  findExposureDetailRequestFromHistory,
} from './AccountDetailContent'

describe('AccountDetailContent exposure detail helpers', () => {
  it('extracts a usable exposure detail query from one history row', () => {
    expect(buildExposureDetailQueryFromHistory({
      itemTitle: '可查曝光商品',
      instanceId: 'inst-1',
      bizCode: 'biz-1',
      itemId: 'item-1',
      date: '2026-06-11',
    })).toEqual({
      instanceId: 'inst-1',
      bizCode: 'biz-1',
      itemId: 'item-1',
      date: '2026-06-11',
    })
  })

  it('can find the first queryable request from history rows', () => {
    expect(findExposureDetailRequestFromHistory([
      { itemTitle: '没有明细标识', exposureCnt: 3, date: '2026-06-10' },
      { itemTitle: '可查曝光商品', exposureCnt: 8, ds: '2026-06-11', detail: { taskInstanceId: 'task-1', itemId: 'item-1' } },
    ])).toEqual({
      label: '可查曝光商品',
      query: {
        taskInstanceId: 'task-1',
        itemId: 'item-1',
        date: '2026-06-11',
      },
    })
  })

  it('returns null when history rows do not contain usable detail identifiers', () => {
    expect(findExposureDetailRequestFromHistory([
      { itemTitle: '只有曝光数', exposureCnt: 5, date: '2026-06-10' },
      { title: '只有日期', ds: '2026-06-11' },
    ])).toBeNull()
  })
})
