import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { applyPostPublishExtras, buildRepublishRequest } from './market'

function ok(payload: any) {
  return {
    ok: true,
    json: async () => payload,
  }
}

function fail(detail: string) {
  return {
    ok: false,
    json: async () => ({ detail }),
  }
}

describe('applyPostPublishExtras', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    vi.useFakeTimers()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
      clear: vi.fn(),
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('syncs and retries when a freshly published item is not yet visible in local cache', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(ok({ ok: true, total: 1, synced_at: 1 }))
      .mockResolvedValueOnce(fail('商品不存在'))
      .mockResolvedValueOnce(fail('商品不存在'))
      .mockResolvedValueOnce(fail('商品不存在'))
      .mockResolvedValueOnce(ok({ ok: true, total: 1, synced_at: 2 }))
      .mockResolvedValueOnce(ok({
        data: {
          deductionRatioList: [{ ratio: '5' }],
        },
      }))
      .mockResolvedValueOnce(ok({ ret: ['SUCCESS::调用成功'] }))
      .mockResolvedValueOnce(ok({
        ret: ['SUCCESS::调用成功'],
        data: {
          drawer: {
            mode: 'amount',
            options: [{ price: '1.00', grouponPrice: '1.00' }],
          },
        },
      }))
      .mockResolvedValueOnce(ok({
        ret: ['SUCCESS::调用成功'],
        data: {
          guide: {
            grouponPrice: '1.00',
          },
        },
      }))
      .mockResolvedValueOnce(ok({ ret: ['SUCCESS::调用成功'] }))
    vi.stubGlobal('fetch', fetchMock)

    const promise = applyPostPublishExtras({
      uid: 'uid-publish',
      publishResponse: {
        ret: ['SUCCESS::调用成功'],
        data: { itemId: 'new-live-item' },
      },
      price: '19.90',
      coinEnabled: true,
      preferredCoinPercent: 5,
      grouponEnabled: true,
      grouponAmount: '1.00',
      grouponDelayMs: 2000,
    })
    await vi.runAllTimersAsync()
    const result = await promise

    expect(result).toEqual({
      itemId: 'new-live-item',
      results: [
        { label: '闲鱼币5%', ok: true },
        { label: '小刀¥1.00', ok: true, appliedPrice: '1.00', mode: 'amount' },
      ],
    })

    const urls = fetchMock.mock.calls.map(([url]) => String(url))
    expect(urls.filter((url) => url.includes('/api/items/sync'))).toHaveLength(2)
    expect(urls[0]).toContain('/api/items/sync')
    expect(urls[1]).toContain('/api/coin/deduction/ratio')
    expect(urls[4]).toContain('/api/items/sync')
  })
})

describe('buildRepublishRequest', () => {
  it('uses the full edit-detail data instead of abbreviated sold-out list fields', () => {
    const request = buildRepublishRequest(
      {
        title: '列表里的旧标题',
        price: '1',
        detail: { rePublishText: '重新上架' },
      },
      {
        data: {
          itemTextDTO: { title: '编辑详情标题', desc: '完整商品描述' },
          itemPriceDTO: { priceInCent: '8800', origPriceInCent: '9900', noBargain: false },
          itemCatDTO: { tbCatId: '50018264', channelCatId: '201460608', leafId: '50018264' },
          imageInfoDOList: [{ url: 'https://img.example/item.jpg' }],
          quantity: '3',
          simpleItem: true,
        },
      },
    )

    expect(request.payload).toMatchObject({
      title: '编辑详情标题',
      desc: '完整商品描述',
      price: '88',
      orig_price: '99',
      image_urls: ['https://img.example/item.jpg'],
      quantity: 3,
      detail: expect.any(Object),
    })
    expect(request.itemContext).toMatchObject({
      tbCatId: '50018264',
      channelCatId: '201460608',
      leafId: '50018264',
      simpleItem: true,
    })
  })
})
