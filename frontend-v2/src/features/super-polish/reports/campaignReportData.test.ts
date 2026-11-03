import { describe, expect, it, vi } from 'vitest'

import {
  buildCampaignReportWindows,
  extractCampaignReportRows,
  loadCampaignReportRange,
  mergeCampaignReportRows,
} from './campaignReportData'

function makeRawReportRow(overrides: Record<string, unknown> = {}) {
  return {
    campaignId: 45444934,
    title: '论文指导降重修改服务',
    status: 99,
    adGroupIds: [45144810],
    idleAdGroupDataVO: {
      data: {
        clickPv: 8,
        chatPv: 1,
        favorites: 0,
        cost: 2990,
        showPv: 1183,
        inquiries: 0,
        payPv: 0,
        realCost: 2990,
      },
      dataDetailVOList: [
        {
          time: '2025-12-04 00:00:00',
          clickPv: 8,
          chatPv: 1,
          favorites: 0,
          cost: 2990,
          showPv: 1183,
          inquiries: 0,
          payPv: 0,
          realCost: 2990,
        },
      ],
    },
    ...overrides,
  }
}

describe('campaignReportData', () => {
  it('splits report ranges into upstream-safe 7-day windows', () => {
    expect(buildCampaignReportWindows('2025-12-01', '2025-12-20')).toEqual([
      { startDate: '2025-12-01', endDate: '2025-12-07' },
      { startDate: '2025-12-08', endDate: '2025-12-14' },
      { startDate: '2025-12-15', endDate: '2025-12-20' },
    ])
  })

  it('extracts campaign report metrics from idleAdGroupDataVO instead of the top level', () => {
    const rows = extractCampaignReportRows({
      result: [makeRawReportRow()],
    })

    expect(rows).toEqual([
      expect.objectContaining({
        campaignId: '45444934',
        title: '论文指导降重修改服务',
        statusCode: 99,
        adGroupCount: 1,
        clickPv: 8,
        chatPv: 1,
        favorites: 0,
        cost: 2990,
        showPv: 1183,
        realCost: 2990,
        detailItems: [
          expect.objectContaining({
            time: '2025-12-04 00:00:00',
            clickPv: 8,
            chatPv: 1,
            cost: 2990,
          }),
        ],
      }),
    ])
  })

  it('merges the same campaign across multiple windows by summing the true report metrics', () => {
    const merged = mergeCampaignReportRows([
      {
        campaignId: '53032012',
        title: '擦亮宝贝20260124081825',
        statusCode: 1,
        adGroupCount: 2,
        adGroupIds: ['54822247', '54822248'],
        clickPv: 8,
        chatPv: 2,
        favorites: 0,
        cost: 489,
        showPv: 486,
        inquiries: 2,
        payPv: 1,
        realCost: 489,
        detailItems: [
          {
            time: '2026-01-24 00:00:00',
            clickPv: 8,
            chatPv: 2,
            favorites: 0,
            cost: 489,
            showPv: 486,
            inquiries: 2,
            payPv: 1,
            realCost: 489,
          },
        ],
      },
      {
        campaignId: '53032012',
        title: '擦亮宝贝20260124081825',
        statusCode: 1,
        adGroupCount: 2,
        adGroupIds: ['54822247', '54822248'],
        clickPv: 0,
        chatPv: 0,
        favorites: 0,
        cost: 47,
        showPv: 77,
        inquiries: 0,
        payPv: 0,
        realCost: 47,
        detailItems: [
          {
            time: '2026-01-25 00:00:00',
            clickPv: 0,
            chatPv: 0,
            favorites: 0,
            cost: 47,
            showPv: 77,
            inquiries: 0,
            payPv: 0,
            realCost: 47,
          },
        ],
      },
    ])

    expect(merged).toEqual([
      expect.objectContaining({
        campaignId: '53032012',
        clickPv: 8,
        chatPv: 2,
        favorites: 0,
        cost: 536,
        showPv: 563,
        inquiries: 2,
        payPv: 1,
        realCost: 536,
        detailItems: [
          expect.objectContaining({ time: '2026-01-24 00:00:00', cost: 489 }),
          expect.objectContaining({ time: '2026-01-25 00:00:00', cost: 47 }),
        ],
      }),
    ])
  })

  it('loads long ranges by splitting them into multiple campaign report requests and merging the rows', async () => {
    const requestMock = vi.fn(async (_uid: string, payload: Record<string, unknown>) => {
      if (payload.startDate === '20251201' && payload.endDate === '20251207') {
        return {
          ret: ['SUCCESS::调用成功'],
          data: {
            total: 1,
            result: [makeRawReportRow()],
          },
        }
      }

      if (payload.startDate === '20251208' && payload.endDate === '20251210') {
        return {
          ret: ['SUCCESS::调用成功'],
          data: {
            total: 1,
            result: [
              makeRawReportRow({
                idleAdGroupDataVO: {
                  data: {
                    clickPv: 2,
                    chatPv: 0,
                    favorites: 1,
                    cost: 300,
                    showPv: 120,
                    inquiries: 0,
                    payPv: 0,
                    realCost: 300,
                  },
                  dataDetailVOList: [
                    {
                      time: '2025-12-09 00:00:00',
                      clickPv: 2,
                      chatPv: 0,
                      favorites: 1,
                      cost: 300,
                      showPv: 120,
                      inquiries: 0,
                      payPv: 0,
                      realCost: 300,
                    },
                  ],
                },
              }),
            ],
          },
        }
      }

      throw new Error(`unexpected payload: ${JSON.stringify(payload)}`)
    })

    const result = await loadCampaignReportRange(
      'u1',
      { startDate: '2025-12-01', endDate: '2025-12-10' },
      requestMock,
    )

    expect(requestMock).toHaveBeenCalledTimes(2)
    expect(result.windows).toEqual([
      { startDate: '2025-12-01', endDate: '2025-12-07' },
      { startDate: '2025-12-08', endDate: '2025-12-10' },
    ])
    expect(result.rows).toEqual([
      expect.objectContaining({
        campaignId: '45444934',
        clickPv: 10,
        chatPv: 1,
        favorites: 1,
        cost: 3290,
        showPv: 1303,
        realCost: 3290,
      }),
    ])
    expect(result.summary).toEqual(
      expect.objectContaining({
        clickPv: 10,
        chatPv: 1,
        favorites: 1,
        cost: 3290,
        showPv: 1303,
      }),
    )
  })
})
