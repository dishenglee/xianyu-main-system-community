import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPromotion, getPromotionView } from './shopGrowth'
import { request } from './client'

vi.mock('./client', () => ({
  request: vi.fn(),
}))

const requestMock = vi.mocked(request)

describe('shopGrowth api', () => {
  beforeEach(() => {
    requestMock.mockReset()
    requestMock.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
  })

  it('queries promotion.view with the selected item id before create', async () => {
    await getPromotionView('u1', '1055797812835')

    expect(requestMock).toHaveBeenCalledWith(
      'GET',
      '/api/growth/coin/packages?uid=u1&itemId=1055797812835',
      undefined,
      {},
    )
  })

  it('sends create confirmation and expected cost to the backend safety guard', async () => {
    await createPromotion({
      uid: 'u1',
      itemId: '1055797812835',
      promotionNum: '50',
      costCoinNum: 1500,
    })

    expect(requestMock).toHaveBeenCalledWith(
      'POST',
      '/api/growth/coin/create',
      {
        uid: 'u1',
        itemId: '1055797812835',
        promotionNum: '50',
        costCoinNum: 1500,
        confirm: true,
        expectedCostCoinNum: 1500,
      },
    )
  })
})
