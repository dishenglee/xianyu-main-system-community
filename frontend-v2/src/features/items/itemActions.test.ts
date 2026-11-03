// @ts-nocheck
import { beforeEach, describe, expect, it, vi } from 'vitest'

const apiMocks = vi.hoisted(() => ({
  itemPublish: vi.fn(),
  publishCheck: vi.fn(),
  enableCoinDeduction: vi.fn(),
  enableGroupon: vi.fn(),
  applyPostPublishExtras: vi.fn(),
  refreshAccount: vi.fn(),
  isSuccess: vi.fn((resp: any) => String(resp?.ret?.[0] || '').startsWith('SUCCESS')),
  retMsg: vi.fn((_resp: any, fallback = '') => fallback),
}))

vi.mock('../../api/index', () => apiMocks)

import { runBatchPublishJobs } from './itemActions'

describe('runBatchPublishJobs', () => {
  const activeAccounts = [
    {
      uid: 'u1',
      nick: '测试账号',
      status: 'active',
      location: {
        province: '湖南省',
        city: '长沙市',
        area: '岳麓区',
        divisionId: '430104',
        latitude: '28.21',
        longitude: '112.93',
      },
    },
  ]

  const taskProgressStore = {
    start: vi.fn(),
    tick: vi.fn(),
    finish: vi.fn(),
  }

  const params = {
    selectedUids: ['u1'],
    form: {
      title: '测试纸巾',
      desc: '测试描述',
      price: '19.90',
      origPrice: '',
      simpleItem: true,
      noBargain: false,
      shippingType: 'free',
      postPrice: '',
      freightTemplateId: '',
      quantity: 1,
    },
    imageUrls: ['https://img.alicdn.com/test.jpg'],
    cardList: [
      {
        propertyId: '2001',
        propertyName: '成色',
        valuesList: [
          { valueId: '1', valueName: '全新' },
          { valueId: '2', valueName: '95新' },
        ],
      },
    ],
    propSelections: {
      '2001': '2',
    },
    coinEnabled: true,
    coinDeductionPercent: '5',
    grouponEnabled: true,
    grouponAmount: '1.00',
  }

  beforeEach(() => {
    vi.clearAllMocks()
    apiMocks.publishCheck.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: { canPublish: true },
    })
    apiMocks.itemPublish.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: { itemId: 'new-live-item' },
    })
    apiMocks.applyPostPublishExtras.mockResolvedValue({
      itemId: 'new-live-item',
      results: [
        { label: '闲鱼币5%', ok: true },
        { label: '小刀¥1.00', ok: true },
      ],
    })
  })

  it('applies shared post-publish extras when publish returns data.itemId', async () => {
    const doSync = vi.fn().mockResolvedValue(undefined)

    await runBatchPublishJobs(params, activeAccounts, taskProgressStore, doSync)

    expect(apiMocks.itemPublish).toHaveBeenCalledWith('u1', expect.objectContaining({
      cardList: params.cardList,
      propSelections: params.propSelections,
    }))

    expect(apiMocks.applyPostPublishExtras).toHaveBeenCalledWith({
      uid: 'u1',
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
    expect(doSync).toHaveBeenCalledWith('u1')
    expect(taskProgressStore.finish).toHaveBeenCalledWith([
      { uid: 'u1', ok: true, msg: '成功 (闲鱼币5%✓ 小刀¥1.00✓)' },
    ])
  })

  it('keeps successful publish even when no extra options are selected', async () => {
    const doSync = vi.fn().mockResolvedValue(undefined)
    apiMocks.applyPostPublishExtras.mockResolvedValueOnce({
      itemId: 'new-live-item',
      results: [],
    })

    await runBatchPublishJobs({
      ...params,
      coinEnabled: false,
      grouponEnabled: false,
    }, activeAccounts, taskProgressStore, doSync)

    expect(apiMocks.applyPostPublishExtras).toHaveBeenCalledWith({
      uid: 'u1',
      publishResponse: {
        ret: ['SUCCESS::调用成功'],
        data: { itemId: 'new-live-item' },
      },
      price: '19.90',
      coinEnabled: false,
      preferredCoinPercent: 5,
      grouponEnabled: false,
      grouponAmount: '1.00',
      grouponDelayMs: 2000,
    })
    expect(taskProgressStore.finish).toHaveBeenCalledWith([
      { uid: 'u1', ok: true, msg: '成功' },
    ])
  })
})
