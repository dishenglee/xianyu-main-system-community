import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  bulkCreateCards,
  createDeliveryRule,
  discoverAiModels,
  createItemReply,
  itemPublish,
  createKeyword,
  itemImportLink,
  testAiSettingsConnection,
  updateAiSettings,
  updateDeliveryRule,
  updateItemReply,
  updateKeyword,
} from './market'

describe('itemImportLink', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
      clear: vi.fn(),
    })
  })

  it('rejects suspicious fallback payloads that echo the raw import link', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        title: 'https://www.goofish.com/item?id=778899',
        desc: 'https://www.goofish.com/item?id=778899',
        price: '99.00',
        origPrice: '99.00',
        imageUrls: [],
      }),
    }))

    await expect(
      itemImportLink('uid-1', 'https://www.goofish.com/item?id=778899'),
    ).rejects.toThrow('导入商品详情失败，请稍后重试')
  })

  it('keeps valid imported detail payloads', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        title: '苹果 x 国行双卡',
        desc: '成色不错，插卡即用',
        price: '603.00',
        origPrice: '699.00',
        imageUrls: ['https://img.alicdn.com/item.jpg'],
      }),
    }))

    await expect(
      itemImportLink('uid-1', 'https://www.goofish.com/item?id=778899'),
    ).resolves.toMatchObject({
      title: '苹果 x 国行双卡',
      desc: '成色不错，插卡即用',
      imageUrls: ['https://img.alicdn.com/item.jpg'],
    })
  })

  it('defaults virtual-service flag to false when publish caller omits it', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ret: ['SUCCESS::调用成功'], data: { itemId: 'new-item' } }),
    })
    vi.stubGlobal('fetch', fetchMock)

    await itemPublish('uid-1', {
      title: '测试商品',
      desc: '测试描述',
      price: '19.90',
      imageUrls: ['https://img.alicdn.com/item.jpg'],
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [, init] = fetchMock.mock.calls[0]
    const payload = JSON.parse(String(init?.body || '{}'))
    expect(payload.simple_item).toBe(false)
    expect(payload.no_bargain).toBe(false)
  })
})

describe('business config API wrappers', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
      clear: vi.fn(),
    })
  })

  it('unwraps keyword create and update envelopes', async () => {
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', keyword: { id: 7, cookieId: 'uid-1', keyword: '价格', reply: '可以优惠' } }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', keyword: { id: 7, cookieId: 'uid-1', keyword: '费用', reply: '按需求报价' } }),
      }))

    await expect(createKeyword('uid-1', { keyword: '价格', reply: '可以优惠' })).resolves.toMatchObject({
      id: 7,
      cookieId: 'uid-1',
      keyword: '价格',
    })
    await expect(updateKeyword(7, { keyword: '费用' })).resolves.toMatchObject({
      id: 7,
      keyword: '费用',
    })
  })

  it('unwraps reply, card, delivery, and AI setting envelopes', async () => {
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', itemReply: { id: 11, cookieId: 'uid-1', reply: '默认文字' } }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', itemReply: { id: 11, cookieId: 'uid-1', itemId: '100', reply: '商品回复' } }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', cards: [{ id: 21, cookieId: '__global__', cardContent: 'CARD-1' }] }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', deliveryRule: { id: 31, cookieId: '__global__', itemId: '100', cardPoolTag: 'vip' } }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', deliveryRule: { id: 31, cookieId: '__global__', itemId: '101', cardPoolTag: 'vip' } }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', settings: { cookieId: '__global__', aiEnabled: true, modelName: 'gpt-test' } }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', models: ['gpt-4o-mini', 'gpt-4.1'], endpoint: 'https://api.example.com/v1/models' }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ status: 'ok', ok: true, message: '连接成功，模型回复：ok', modelName: 'gpt-4o-mini' }),
      }))

    await expect(createItemReply('uid-1', { reply: '默认文字' })).resolves.toMatchObject({ id: 11, reply: '默认文字' })
    await expect(updateItemReply(11, { itemId: '100' })).resolves.toMatchObject({ id: 11, itemId: '100' })
    await expect(bulkCreateCards('__global__', [{ cardContent: 'CARD-1' }])).resolves.toMatchObject([{ id: 21, cardContent: 'CARD-1' }])
    await expect(createDeliveryRule('__global__', { itemId: '100', cardPoolTag: 'vip' })).resolves.toMatchObject({ id: 31, itemId: '100' })
    await expect(updateDeliveryRule(31, { itemId: '101' })).resolves.toMatchObject({ id: 31, itemId: '101' })
    await expect(updateAiSettings('__global__', { aiEnabled: true })).resolves.toMatchObject({ cookieId: '__global__', aiEnabled: true })
    await expect(discoverAiModels({ apiKey: 'sk-test', baseUrl: 'https://api.example.com/v1' })).resolves.toMatchObject({
      models: ['gpt-4o-mini', 'gpt-4.1'],
      endpoint: 'https://api.example.com/v1/models',
    })
    await expect(testAiSettingsConnection({ apiKey: 'sk-test', baseUrl: 'https://api.example.com/v1', modelName: 'gpt-4o-mini' })).resolves.toMatchObject({
      ok: true,
      modelName: 'gpt-4o-mini',
    })
  })
})
