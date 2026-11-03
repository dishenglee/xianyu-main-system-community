import { describe, expect, it } from 'vitest'
import { normalizeLegacyCampaignListWindow } from './campaignDateWindow'

describe('normalizeLegacyCampaignListWindow', () => {
  it('upgrades the legacy 7-day built-list default to the 365-day default', () => {
    expect(normalizeLegacyCampaignListWindow({
      startDate: '2026-06-16',
      endDate: '2026-06-22',
      today: '2026-06-22',
    })).toEqual({
      startDate: '2025-06-22',
      endDate: '2026-06-22',
    })
  })

  it('keeps manually selected ranges unchanged', () => {
    expect(normalizeLegacyCampaignListWindow({
      startDate: '2026-05-01',
      endDate: '2026-06-22',
      today: '2026-06-22',
    })).toEqual({
      startDate: '2026-05-01',
      endDate: '2026-06-22',
    })
  })
})
