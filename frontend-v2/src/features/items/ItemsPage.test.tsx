// @ts-nocheck
import React from 'react'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { apiMocks, accountStoreMock, notifyMock } = vi.hoisted(() => ({
  apiMocks: {
    itemList: vi.fn(),
    itemAllList: vi.fn(),
    itemSoldOutList: vi.fn(),
    itemPolish: vi.fn(),
    itemDownshelf: vi.fn(),
    itemUpshelf: vi.fn(),
    itemDelete: vi.fn(),
    syncItems: vi.fn(),
    syncAllItems: vi.fn(),
    reducePrice: vi.fn(),
    itemEdit: vi.fn(),
    itemOpen: vi.fn(),
    republishItem: vi.fn(),
    republishItemWithDetail: vi.fn(),
    itemEditDetail: vi.fn(),
    itemPublish: vi.fn(),
    itemImportLink: vi.fn(),
    uploadItemImage: vi.fn(),
    poiSearch: vi.fn(),
    itemShare: vi.fn(),
    itemCoinDeductionOpen: vi.fn(),
    itemCoinDeductionClose: vi.fn(),
    itemGrouponStatus: vi.fn(),
    itemGrouponGuide: vi.fn(),
    itemGrouponOpen: vi.fn(),
    itemGrouponClose: vi.fn(),
    itemProperties: vi.fn(),
    coinDeductionRatio: vi.fn(),
    itemKgraph: vi.fn(),
    itemAiDescription: vi.fn(),
    publishCheck: vi.fn(),
    publishBadwords: vi.fn(),
    isSuccess: vi.fn((response) => String(response?.ret?.[0] || '').toUpperCase().startsWith('SUCCESS')),
    retMsg: vi.fn((_response, fallback) => fallback),
  },
  accountStoreMock: {
    accounts: [
      {
        uid: '90000000002',
        nick: '测试账号',
        status: 'active',
        location: { province: '湖南省', city: '长沙市', area: '岳麓区', divisionId: '430104' },
      },
    ],
    loadAccounts: vi.fn().mockResolvedValue(undefined),
  },
  notifyMock: vi.fn(),
}))

const defaultAccounts = [
  {
    uid: '90000000002',
    nick: '测试账号',
    status: 'active',
    location: { province: '湖南省', city: '长沙市', area: '岳麓区', divisionId: '430104' },
  },
]

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../api/index', () => apiMocks)
vi.mock('../../components/Notify', () => ({
  useSafeNotify: () => notifyMock,
}))
// ItemsPage 现在是按设备类型分流的 router shim — 测试目标是桌面发布 / 编辑流，
// 直接 mock 成 desktop 即可（jsdom 默认没有 matchMedia）。
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ device: 'desktop', isMobile: false, isTablet: false, isDesktop: true }),
  getDeviceTypeForWidth: () => 'desktop',
}))

import ItemsPage from './ItemsPage'
import { KnifeSheet } from './mobile/ItemSheets'

describe('ItemsPage publish flow', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    accountStoreMock.accounts = [...defaultAccounts]
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM001',
          title: '测试商品',
          price: '88.00',
          online: true,
          browseCount: 1,
          wantCount: 0,
        },
      ],
    })
    apiMocks.uploadItemImage.mockResolvedValue({ success: true, url: 'https://img.alicdn.com/test-keyboard.jpg' })
    apiMocks.itemKgraph.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        category: { catName: '键盘', channelCatId: '201460608', tbCatId: '50018264' },
        cardList: [
          {
            propertyId: '-10000',
            propertyName: '分类',
            valuesList: [{ catName: '键盘', channelCatId: '201460608', tbCatId: '50018264' }],
          },
        ],
      },
    })
    apiMocks.itemAiDescription.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: { desc: '罗技键盘，成色如图，功能正常，适合办公和游戏。' },
    })
    apiMocks.publishCheck.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: { canPublish: true, message: '可发布' },
    })
    apiMocks.publishBadwords.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: { safe: true, matched: [] },
    })
    apiMocks.itemEditDetail.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        itemTextDTO: { title: '测试商品', desc: '测试描述', shortTitle: '测试商品' },
        itemPriceDTO: { priceInCent: '8800', origPriceInCent: '9900', noBargain: false },
        itemCatDTO: { catId: '50018264', tbCatId: '50018264', channelCatId: '201460608', leafId: '50018264' },
        imageInfoDOList: [{ url: 'https://img.alicdn.com/test-keyboard.jpg' }],
        quantity: '1',
      },
    })
    apiMocks.itemEdit.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: {} })
    apiMocks.itemOpen.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { itemStatus: '0' } })
    apiMocks.republishItem.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { itemStatus: '0' } })
    apiMocks.republishItemWithDetail.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { itemStatus: '0' } })
    apiMocks.itemGrouponStatus.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        canGroupon: true,
        groupon: false,
        price: '28.00',
        itemId: 'ITEM002',
        drawer: {
          title: '设置小刀优惠',
          itemPrice: '28.00',
          recommendedPrice: '23.8',
          selectedPrice: '23.8',
          recommendedDiscount: '4.2',
          selectedDiscount: '4.2',
          helperText: '暂未设置优惠，系统自动推荐优惠档位',
          closeOptionLabel: '不开',
          buttonText: '确定',
          descLines: ['2人拼手气，成交更快'],
          freeShipping: true,
          historyGroupon: true,
          options: [
            { price: '6.66', grouponPrice: '6.66', label: '¥6.66' },
            { price: '15.00', grouponPrice: '15.00', label: '¥15.00' },
            { price: '50.00', grouponPrice: '50.00', label: '¥50.00' },
          ],
        },
      },
    })
    apiMocks.itemGrouponGuide.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        found: true,
        guide: {
          grouponPrice: '23.80',
          buttonText: '立刻开启',
          descText: ['2人拼手气，成交更快'],
        },
      },
    })
    apiMocks.itemGrouponOpen.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: {} })
    apiMocks.syncAllItems.mockResolvedValue({ accounts: [], success_count: 0, total: 0 })
  })

  it('keeps desktop item KPIs in the goods header without the redundant hub tab', async () => {
    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    const heading = await screen.findByRole('heading', { name: '商品' })
    const headerSection = heading.closest('section')

    expect(screen.queryByRole('link', { name: '我的商品' })).toBeNull()
    expect(screen.queryByText('我的商品')).toBeNull()
    expect(headerSection).toBeTruthy()

    const header = within(headerSection!)
    const kpiLabels = ['商品总数', '浏览', '想要', '最低价']
    for (const label of kpiLabels) {
      expect(await header.findByText(label)).toBeTruthy()
    }

    const kpiGrid = header.getByText('商品总数').closest('.grid')
    expect(kpiGrid).toBeTruthy()
    expect(headerSection!.contains(kpiGrid)).toBe(true)
    expect(within(kpiGrid!).getAllByText('在售').length).toBeGreaterThan(0)
  })

  it('keeps desktop item KPIs and header actions together without the redundant hub tab', async () => {
    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    const heading = await screen.findByRole('heading', { name: '商品' })
    const headerSection = heading.closest('section')

    expect(screen.queryByRole('link', { name: '我的商品' })).toBeNull()
    expect(screen.queryByText('我的商品')).toBeNull()
    expect(headerSection).toBeTruthy()

    const header = within(headerSection!)
    expect(header.getByRole('button', { name: /隐藏敏感信息|显示敏感信息/ })).toBeTruthy()
    expect(header.getByRole('button', { name: '同步' })).toBeTruthy()

    const kpiLabels = ['商品总数', '浏览', '想要', '最低价']
    for (const label of kpiLabels) {
      expect(await header.findByText(label)).toBeTruthy()
    }

    const kpiGrid = header.getByText('商品总数').closest('.grid')
    expect(kpiGrid).toBeTruthy()
    expect(within(kpiGrid!).getAllByText('在售').length).toBeGreaterThan(0)
  })

  it('keeps link import and removes H5 live entry', async () => {
    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    expect(screen.queryByText('H5实时')).toBeNull()

    fireEvent.click(screen.getAllByRole('button', { name: '发布' }).at(-1)!)
    expect(screen.getByPlaceholderText('粘贴闲鱼商品链接或口令，自动填充信息')).toBeTruthy()
    expect(screen.getByRole('button', { name: '导入' })).toBeTruthy()
  })

  it('defaults商品排序 to latest publish time first', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'OLDER',
          title: '旧发布商品',
          price: '88.00',
          online: true,
          browseCount: 999,
          wantCount: 0,
          publishTime: '1710000000',
        },
        {
          itemId: 'NEWER',
          title: '最新发布商品',
          price: '99.00',
          online: true,
          browseCount: 1,
          wantCount: 0,
          detail: { itemContext: { publishTime: '1710000100' } },
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findByText('最新发布商品')
    const sortSelect = screen.getByRole('combobox') as HTMLSelectElement
    expect(sortSelect.value).toBe('published_desc')
    expect(screen.getByRole('option', { name: '按发布时间' })).toBeTruthy()

    const cards = Array.from(container.querySelectorAll('.items-grid > *'))
    expect(cards).toHaveLength(2)
    expect(cards[0].textContent).toContain('最新发布商品')
    expect(cards[1].textContent).toContain('旧发布商品')
  })

  it('removes the title input and refreshes category from description without using title', async () => {
    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    fireEvent.click(screen.getAllByRole('button', { name: '发布' })[0])
    expect(screen.queryByPlaceholderText('商品标题（最多60字）')).toBeNull()
    fireEvent.change(screen.getByPlaceholderText('详细描述商品…'), { target: { value: '罗技键盘，功能正常，成色不错。' } })
    fireEvent.click(screen.getByRole('button', { name: '重新识别' }))

    await waitFor(() => {
      expect(apiMocks.itemKgraph).toHaveBeenCalledWith('90000000002', expect.objectContaining({
        title: '',
        desc: '罗技键盘，功能正常，成色不错。',
        imageUrls: [],
      }))
    })
    expect(apiMocks.itemAiDescription).not.toHaveBeenCalled()
  })

  it('fills publish description from the AI description action', async () => {
    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    fireEvent.click(screen.getAllByRole('button', { name: '发布' })[0])
    fireEvent.change(screen.getByPlaceholderText('详细描述商品…'), { target: { value: '上一版旧描述，不应该继续续写' } })

    const fileInput = document.body.querySelector('input[type="file"]')
    const file = new File(['fake image'], 'keyboard.jpg', { type: 'image/jpeg' })
    fireEvent.change(fileInput, { target: { files: [file] } })

    await waitFor(() => {
      expect(apiMocks.itemKgraph).toHaveBeenCalledWith('90000000002', expect.objectContaining({
        title: '',
        desc: '上一版旧描述，不应该继续续写',
        imageUrls: ['https://img.alicdn.com/test-keyboard.jpg'],
      }))
    })
    expect(apiMocks.itemAiDescription).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'AI写描述' }))
    await waitFor(() => {
      expect(apiMocks.itemAiDescription).toHaveBeenCalledWith('90000000002', expect.objectContaining({
        title: '',
        desc: '上一版旧描述，不应该继续续写',
        imageUrls: ['https://img.alicdn.com/test-keyboard.jpg'],
      }))
    })
    expect(screen.getByPlaceholderText('详细描述商品…').value).toContain('罗技键盘，成色如图')
  })

  it('shows the upstream AI error inline under description without adding a local prefix', async () => {
    apiMocks.itemAiDescription.mockRejectedValueOnce(new Error('官方 AI 描述服务暂时不可用，请稍后重试'))

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    fireEvent.click(screen.getAllByRole('button', { name: '发布' })[0])
    fireEvent.change(screen.getByPlaceholderText('详细描述商品…'), {
      target: { value: '小米17 Max 16+512 白色 国补款。' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'AI写描述' }))

    const errorNode = await screen.findByText('官方 AI 描述服务暂时不可用，请稍后重试')
    expect(screen.queryByText('AI 描述生成失败：官方 AI 描述服务暂时不可用，请稍后重试')).toBeNull()

    const descInput = screen.getByPlaceholderText('详细描述商品…')
    const priceLabel = screen.getByText('售价 ¥ *')
    expect(descInput.compareDocumentPosition(errorNode) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(errorNode.compareDocumentPosition(priceLabel) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('reclassifies current description before AI generation instead of reusing stale category', async () => {
    apiMocks.itemKgraph
      .mockResolvedValueOnce({
        ret: ['SUCCESS::调用成功'],
        data: {
          category: { catName: '手机', channelCatId: '126862528', tbCatId: '1512' },
          cardList: [
            {
              propertyId: '-10000',
              propertyName: '分类',
              valuesList: [{ catName: '手机', channelCatId: '126862528', tbCatId: '1512', isClicked: '1' }],
            },
          ],
        },
      })
      .mockResolvedValueOnce({
        ret: ['SUCCESS::调用成功'],
        data: {
          category: { catName: 'USB风扇', channelCatId: '126854019', tbCatId: '50018922' },
          cardList: [
            {
              propertyId: '-10000',
              propertyName: '分类',
              valuesList: [{ catName: 'USB风扇', channelCatId: '126854019', tbCatId: '50018922', isClicked: '1' }],
            },
          ],
        },
      })
    apiMocks.itemAiDescription.mockResolvedValueOnce({
      ret: ['SUCCESS::调用成功'],
      data: {
        title: '米家小米小风扇手持风扇',
        desc: '米家手持小风扇，直流变频，户外宿舍都适用。',
      },
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    fireEvent.click(screen.getAllByRole('button', { name: '发布' })[0])

    fireEvent.change(screen.getByPlaceholderText('详细描述商品…'), {
      target: { value: '苹果15 256G 国行蓝色，功能正常。' },
    })
    fireEvent.click(screen.getByRole('button', { name: '重新识别' }))
    await waitFor(() => {
      expect(apiMocks.itemKgraph).toHaveBeenCalledWith('90000000002', expect.objectContaining({
        desc: '苹果15 256G 国行蓝色，功能正常。',
      }))
    })

    fireEvent.change(screen.getByPlaceholderText('详细描述商品…'), {
      target: { value: '米家小米小风扇手持风扇 直流变频户外便携挂脖宿舍充电风扇 可随身上飞机40h长续航100挡调速' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'AI写描述' }))

    await waitFor(() => {
      expect(apiMocks.itemKgraph).toHaveBeenNthCalledWith(2, '90000000002', expect.objectContaining({
        title: '',
        desc: '米家小米小风扇手持风扇 直流变频户外便携挂脖宿舍充电风扇 可随身上飞机40h长续航100挡调速',
        imageUrls: [],
      }))
    })
    await waitFor(() => {
      expect(apiMocks.itemAiDescription).toHaveBeenCalledTimes(1)
    })
    const [, aiPayload] = apiMocks.itemAiDescription.mock.calls[0]
    expect(aiPayload).toMatchObject({
      title: '',
      desc: '米家小米小风扇手持风扇 直流变频户外便携挂脖宿舍充电风扇 可随身上飞机40h长续航100挡调速',
      category: expect.objectContaining({ catName: 'USB风扇' }),
    })
    expect(aiPayload.cardList).toEqual([
      expect.objectContaining({
        propertyId: '-10000',
        propertyName: '分类',
        valuesList: [
          expect.objectContaining({
            catName: 'USB风扇',
            channelCatId: '126854019',
            isClicked: '1',
          }),
        ],
      }),
    ])
    expect(aiPayload.propSelections).toEqual({
      '-10000': '126854019',
    })
    expect(screen.getByPlaceholderText('详细描述商品…').value).toContain('米家手持小风扇')
  })

  it('runs category recognition once per upload batch without auto AI generation', async () => {
    apiMocks.uploadItemImage
      .mockResolvedValueOnce({ success: true, url: 'https://img.alicdn.com/test-keyboard-a.jpg' })
      .mockResolvedValueOnce({ success: true, url: 'https://img.alicdn.com/test-keyboard-b.jpg' })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    fireEvent.click(screen.getAllByRole('button', { name: '发布' })[0])
    fireEvent.change(screen.getByPlaceholderText('详细描述商品…'), { target: { value: '图片实拍，罗技机械键盘成色清晰可见。' } })

    const fileInput = document.body.querySelector('input[type="file"]')
    const fileA = new File(['fake image a'], 'keyboard-a.jpg', { type: 'image/jpeg' })
    const fileB = new File(['fake image b'], 'keyboard-b.jpg', { type: 'image/jpeg' })
    fireEvent.change(fileInput, { target: { files: [fileA, fileB] } })

    await waitFor(() => {
      expect(apiMocks.itemKgraph).toHaveBeenCalledWith('90000000002', expect.objectContaining({
        title: '',
        desc: '图片实拍，罗技机械键盘成色清晰可见。',
        imageUrls: [
          'https://img.alicdn.com/test-keyboard-a.jpg',
          'https://img.alicdn.com/test-keyboard-b.jpg',
        ],
      }))
    })
    await waitFor(() => {
      expect(apiMocks.itemKgraph).toHaveBeenCalledTimes(1)
    })
    await new Promise((resolve) => setTimeout(resolve, 180))
    expect(apiMocks.itemAiDescription).not.toHaveBeenCalled()
    expect(screen.queryByPlaceholderText('商品标题（最多60字）')).toBeNull()
    expect(screen.getByPlaceholderText('详细描述商品…').value).toBe('图片实拍，罗技机械键盘成色清晰可见。')
  })

  it('blocks publish when badwords check finds forbidden words', async () => {
    apiMocks.publishBadwords.mockResolvedValueOnce({
      ret: ['SUCCESS::调用成功'],
      data: { safe: false, matched: ['微信'] },
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    fireEvent.click(screen.getAllByRole('button', { name: '发布' }).at(-1)!)
    fireEvent.change(screen.getByPlaceholderText('详细描述商品…'), { target: { value: '支持微信联系' } })
    fireEvent.change(screen.getByPlaceholderText('0.00'), { target: { value: '88' } })
    fireEvent.click(screen.getAllByRole('button', { name: '发布' }).at(-1)!)

    await waitFor(() => {
      expect(apiMocks.publishBadwords).toHaveBeenCalledWith('90000000002', '支持微信联系')
    })
    expect(apiMocks.itemPublish).not.toHaveBeenCalled()
    expect(await screen.findByText('标题包含疑似敏感词：微信')).toBeTruthy()
  })

  it('prefers the most recently healthy account as default publish account', async () => {
    accountStoreMock.accounts = [
      {
        uid: '2706670786',
        nick: '左岸文学社',
        status: 'active',
        has_h5_tk: true,
        last_session_status: 'ok',
        last_session_ok_at: 1777746824.122125,
        last_session_check_at: 1777746824.122125,
        location: {},
      },
      {
        uid: '2217481990821',
        nick: '小熊小熊lw版',
        status: 'active',
        has_h5_tk: true,
        last_session_status: 'ok',
        last_session_ok_at: 1777746877.439107,
        last_session_check_at: 1777746877.439107,
        location: { province: '湖南省', city: '长沙市', area: '岳麓区', divisionId: '430104' },
      },
    ]

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    fireEvent.click(screen.getAllByRole('button', { name: '发布' }).at(-1)!)
    const preferredRow = screen.getAllByText('小熊小熊lw版')
      .map((node) => node.closest('label'))
      .find(Boolean)
    expect(preferredRow?.textContent || '').toContain('主')
  })

  it('renders the 2人小刀 action when actionType 16 or 17 is available', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          actions: [
            { actionType: '16', actionName: '小刀优惠', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('支持 2人小刀 的商品')
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)

    expect(await screen.findByText('降价')).toBeTruthy()
    expect(await screen.findByText('2人小刀')).toBeTruthy()
  })

  it('opens desktop 2人小刀 with one status request and no guide request', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(await screen.findByText('2人小刀'))

    expect(await screen.findByText('设置小刀优惠')).toBeTruthy()
    expect(await screen.findByText('最多可刀')).toBeTruthy()
    expect(await screen.findByText('请选择小刀优惠金额，买家下单前可随机获得不高于该金额的优惠')).toBeTruthy()
    expect(screen.getAllByText('¥6.66').length).toBeGreaterThan(0)
    expect(screen.getAllByText('¥15.00').length).toBeGreaterThan(0)
    expect(screen.getAllByText('¥50.00').length).toBeGreaterThan(0)
    expect(apiMocks.itemGrouponStatus).toHaveBeenCalledTimes(1)
    expect(apiMocks.itemGrouponStatus).toHaveBeenCalledWith('90000000002', 'ITEM002')
    expect(apiMocks.itemGrouponGuide).not.toHaveBeenCalled()
    fireEvent.click(screen.getAllByText('¥15.00')[0])
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    await waitFor(() => {
      expect(apiMocks.itemGrouponOpen).toHaveBeenCalledWith(
        '90000000002',
        'ITEM002',
        '15.00',
        expect.objectContaining({ mode: 'amount' }),
      )
    })
  })

  it('deduplicates repeated desktop 2人小刀 clicks while status is pending', async () => {
    let resolveStatus: ((value: any) => void) | undefined
    apiMocks.itemGrouponStatus.mockImplementation(() => new Promise((resolve) => {
      resolveStatus = resolve
    }))
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          actions: [{ actionType: '17', actionName: '2人小刀', enable: '1' }],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    const grouponButton = await screen.findByText('2人小刀')
    fireEvent.click(grouponButton)
    fireEvent.click(grouponButton)

    expect(apiMocks.itemGrouponStatus).toHaveBeenCalledTimes(1)
    await act(async () => {
      resolveStatus?.({
        ret: ['SUCCESS::调用成功'],
        data: {
          price: '28.00',
          drawer: { options: [{ price: '15.00', grouponPrice: '15.00', label: '¥15.00' }] },
        },
      })
    })
  })

  it('notifies 2人小刀 success before its background item sync completes', async () => {
    let resolveSync: ((value: { ok: true }) => void) | undefined
    apiMocks.syncItems.mockImplementation(() => new Promise((resolve) => {
      resolveSync = resolve
    }))
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(await screen.findByText('2人小刀'))
    await screen.findByText('设置小刀优惠')
    fireEvent.click(screen.getAllByText('¥15.00')[0])
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    await waitFor(() => {
      expect(apiMocks.syncItems).toHaveBeenCalledWith('90000000002')
    })
    try {
      await waitFor(() => {
        expect(notifyMock).toHaveBeenCalledWith('已开启 2人小刀，最高优惠 ¥15.00', 'success', 3000)
      })
    } finally {
      await act(async () => {
        resolveSync?.({ ok: true })
      })
    }
  })

  it('opens mobile 2人小刀 from status data without requesting guide', async () => {
    render(
      <KnifeSheet
        open
        item={{ _uid: '90000000002', itemId: 'ITEM002', price: '28.00' }}
        onClose={vi.fn()}
        onReload={vi.fn()}
        notify={notifyMock}
      />,
    )

    expect(await screen.findByText('小刀优惠')).toBeTruthy()
    await waitFor(() => {
      expect(screen.queryByText('加载中…')).toBeNull()
    })
    expect(apiMocks.itemGrouponStatus).toHaveBeenCalledTimes(1)
    expect(apiMocks.itemGrouponGuide).not.toHaveBeenCalled()
  })

  it('keeps mobile 2人小刀 success feedback when background reload fails', async () => {
    const onClose = vi.fn()
    const onReload = vi.fn(() => {
      throw new Error('reload failed')
    })
    render(
      <KnifeSheet
        open
        item={{ _uid: '90000000002', itemId: 'ITEM002', price: '28.00' }}
        onClose={onClose}
        onReload={onReload}
        notify={notifyMock}
      />,
    )

    await waitFor(() => {
      expect(screen.queryByText('加载中…')).toBeNull()
    })
    fireEvent.click(screen.getAllByText('¥6.66')[0])
    fireEvent.click(screen.getByRole('button', { name: '开启小刀' }))

    await waitFor(() => {
      expect(onReload).toHaveBeenCalledTimes(1)
    })
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(notifyMock).toHaveBeenCalledTimes(1)
    expect(notifyMock).toHaveBeenCalledWith('小刀已开启：¥6.66', 'success')
  })

  it('supports custom amount when opening 2人小刀', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(await screen.findByText('2人小刀'))

    expect(await screen.findByText('设置小刀优惠')).toBeTruthy()
    expect(await screen.findByText('请选择小刀优惠金额，买家下单前可随机获得不高于该金额的优惠')).toBeTruthy()
    fireEvent.click(screen.getByLabelText('编辑小刀金额'))
    fireEvent.change(screen.getByPlaceholderText('例如 9.90'), { target: { value: '9.9' } })
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    await waitFor(() => {
      expect(apiMocks.itemGrouponOpen).toHaveBeenCalledWith(
        '90000000002',
        'ITEM002',
        '9.90',
        expect.objectContaining({ mode: 'amount' }),
      )
    })
  })

  it('allows custom groupon amount equal to item price under current submit flow', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM005',
          title: '低价商品',
          price: '18.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(await screen.findByText('2人小刀'))
    await screen.findByText('设置小刀优惠')

    fireEvent.click(screen.getByLabelText('编辑小刀金额'))
    fireEvent.change(screen.getByPlaceholderText('例如 9.90'), { target: { value: '18' } })
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    await waitFor(() => {
      expect(apiMocks.itemGrouponOpen).toHaveBeenCalledWith(
        '90000000002',
        'ITEM005',
        '18.00',
        expect.objectContaining({ mode: 'amount' }),
      )
    })
  })

  it('shows enabled 2人小刀 on the item card like coin deduction', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          grouponOpen: true,
          grouponPrice: '15.00',
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('小刀·¥15.00')).toBeTruthy()
  })

  it('shows 不开 in the summary area when the close option is selected', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          grouponOpen: true,
          grouponPrice: '15.00',
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(await screen.findByText('小刀·开'))

    await screen.findByText('设置小刀优惠')
    fireEvent.click(screen.getAllByText('不开')[0])

    expect(await screen.findByText('当前选择 · 不开')).toBeTruthy()
    expect(screen.getByText('当前将关闭 2人小刀')).toBeTruthy()
    expect(screen.queryByText('说明')).toBeNull()
  })

  it('closes the groupon sheet after confirm even when the request fails', async () => {
    apiMocks.itemGrouponOpen.mockRejectedValueOnce(new Error('2人小刀操作失败'))
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM002',
          title: '支持 2人小刀 的商品',
          price: '28.00',
          online: true,
          browseCount: 12,
          wantCount: 3,
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(await screen.findByText('2人小刀'))

    await screen.findByText('设置小刀优惠')
    fireEvent.click(screen.getAllByText('¥15.00')[0])
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    await waitFor(() => {
      expect(notifyMock).toHaveBeenCalledWith('2人小刀操作失败', 'error', 4200)
    })
    await waitFor(() => {
      expect(screen.queryByText('设置小刀优惠')).toBeNull()
    })
  })

  it('disables fixed groupon amounts that are higher than the item price', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM004',
          title: '低价商品',
          price: '18.00',
          online: true,
          browseCount: 3,
          wantCount: 1,
          actions: [
            { actionType: '17', actionName: '2人小刀', enable: '1' },
            { actionType: '3', actionName: '降价', enable: '1' },
          ],
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(await screen.findByText('2人小刀'))
    await screen.findByText('设置小刀优惠')

    const tooHigh = screen.getAllByText('¥50.00')[0]?.closest('button')
    expect(tooHigh?.hasAttribute('disabled')).toBe(true)
    expect(await screen.findByText('请选择小刀优惠金额，买家下单前可随机获得不高于该金额的优惠')).toBeTruthy()
  })

  it('uses the shared edit-detail republish request instead of itemEdit', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [],
    })
    apiMocks.itemSoldOutList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM003',
          title: '已下架商品',
          price: '28.00',
          online: false,
          status: 'OFF_SHELF',
          detail: { rePublishText: '重新上架' },
        },
      ],
    })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    fireEvent.click(await screen.findByRole('button', { name: /已下架/i }))
    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const offCard = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(offCard!)
    fireEvent.click(await screen.findByText('重新上架'))
    await screen.findByDisplayValue('测试商品')
    fireEvent.click(screen.getByRole('button', { name: '保存并上架' }))

    await waitFor(() => {
      expect(apiMocks.republishItemWithDetail).toHaveBeenCalledWith(
        '90000000002',
        'ITEM003',
        expect.objectContaining({ itemId: 'ITEM003' }),
        expect.any(Object),
        expect.objectContaining({
          title: '测试商品',
          price: '88',
          detail: expect.any(Object),
        }),
      )
    })
    expect(apiMocks.itemEdit).not.toHaveBeenCalled()
  })

  it('refreshes the sold-out cache and removes a republished item from the off-shelf tab', async () => {
    const soldOut = {
      itemId: 'ITEM-REPUBLISH-CACHE',
      title: '重新上架缓存商品',
      price: '28.00',
      online: false,
      status: 'SOLD_OUT',
      detail: { rePublishText: '重新上架' },
    }
    apiMocks.itemAllList
      .mockResolvedValueOnce({ _last_synced_at: 0, items: [] })
      .mockResolvedValue({
        _last_synced_at: 1,
        items: [{ ...soldOut, online: true, status: 'ON_SALE' }],
      })
    apiMocks.itemSoldOutList
      .mockResolvedValueOnce({ _last_synced_at: 0, items: [soldOut] })
      .mockResolvedValue({ _last_synced_at: 1, items: [] })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    fireEvent.click(await screen.findByRole('button', { name: /已下架/i }))
    await waitFor(() => expect(container.querySelector('.items-grid > div')).toBeTruthy())
    fireEvent.mouseEnter(container.querySelector('.items-grid > div')!)
    fireEvent.click(await screen.findByText('重新上架'))
    await screen.findByDisplayValue('测试商品')
    fireEvent.click(screen.getByRole('button', { name: '保存并上架' }))

    await waitFor(() => {
      expect(screen.queryByText('重新上架缓存商品')).toBeNull()
      expect(apiMocks.itemSoldOutList).toHaveBeenCalledTimes(2)
    })
  })

  it('blocks only the account requiring validation and keeps failed items selected for retry', async () => {
    accountStoreMock.accounts = [
      ...defaultAccounts,
      { uid: 'u2', nick: '第二账号', status: 'active', location: {} },
    ]
    apiMocks.itemAllList.mockResolvedValue({ _last_synced_at: 0, items: [] })
    const soldOutCalls = new Map<string, number>()
    apiMocks.itemSoldOutList.mockImplementation(async (uid: string) => {
      const count = (soldOutCalls.get(uid) || 0) + 1
      soldOutCalls.set(uid, count)
      if (uid === 'u2' && count > 1) return { _last_synced_at: 1, items: [] }
      return {
        _last_synced_at: 0,
        items: uid === 'u2'
          ? [{ itemId: 'U2-1', title: '其他账号商品', online: false, status: 'SOLD_OUT' }]
          : [
              { itemId: 'U1-1', title: '验证失败商品', online: false, status: 'SOLD_OUT' },
              { itemId: 'U1-2', title: '同账号待重试商品', online: false, status: 'SOLD_OUT' },
            ],
      }
    })
    apiMocks.republishItem.mockImplementation(async (uid: string, itemId: string) => {
      if (uid === '90000000002' && itemId === 'U1-1') {
        return { ret: ['FAIL_SYS_USER_VALIDATE::请先完成安全验证'] }
      }
      return { ret: ['SUCCESS::调用成功'], data: { itemStatus: '0' } }
    })
    apiMocks.syncItems.mockResolvedValue({ ok: true })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    fireEvent.click(await screen.findByRole('button', { name: /已下架/i }))
    await screen.findByText('其他账号商品')
    fireEvent.click(screen.getByRole('button', { name: '选择' }))
    fireEvent.click(screen.getByRole('button', { name: '全选' }))
    fireEvent.click(screen.getByRole('button', { name: '上架' }))
    fireEvent.click((await screen.findAllByRole('button', { name: '上架' })).at(-1)!)

    await waitFor(() => {
      expect(apiMocks.republishItem).toHaveBeenCalledWith('u2', 'U2-1', expect.any(Object))
    }, { timeout: 5000 })
    expect(apiMocks.republishItem).toHaveBeenCalledWith('90000000002', 'U1-1', expect.any(Object))
    expect(apiMocks.republishItem).not.toHaveBeenCalledWith('90000000002', 'U1-2', expect.any(Object))
    await waitFor(() => expect(screen.getByText(/已选 2 \/ 2/)).toBeTruthy())
    expect(notifyMock).toHaveBeenCalledWith(
      expect.stringContaining('批量上架完成：1 成功，1 失败，1 跳过'),
      'error',
      4200,
    )
  })

  it('removes the deleted item from the list immediately after success', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM-DELETE-1',
          title: '待删除商品',
          price: '28.00',
          online: true,
          browseCount: 1,
          wantCount: 0,
          actions: [],
        },
      ],
    })
    apiMocks.itemDelete.mockResolvedValue({ ret: ['SUCCESS::删除成功'] })

    const { container } = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await waitFor(() => {
      expect(container.querySelector('.items-grid > div')).toBeTruthy()
    })
    const card = container.querySelector('.items-grid > div')
    fireEvent.mouseEnter(card!)
    fireEvent.click(screen.getByTitle('删除'))
    fireEvent.click((await screen.findAllByRole('button', { name: '删除' })).at(-1)!)

    await waitFor(() => {
      expect(notifyMock).toHaveBeenCalledWith('删除成功：删除成功', 'success', 3000)
    })
    expect(screen.queryByText('待删除商品')).toBeNull()
  })

  it('does not leak off-shelf items into the on-sale tab after switching tabs', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM001',
          title: '在售商品',
          price: '88.00',
          online: true,
          status: 'ON_SALE',
          browseCount: 1,
          wantCount: 0,
        },
      ],
    })
    apiMocks.itemSoldOutList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM009',
          title: '已下架商品',
          price: '28.00',
          online: true,
          status: '',
        },
      ],
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('在售商品')).toBeTruthy()
    expect(screen.queryByText('已下架商品')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /已下架/i }))
    expect(await screen.findByText('已下架商品')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /^在售/i }))
    expect(await screen.findByText('在售商品')).toBeTruthy()
    expect(screen.queryByText('已下架商品')).toBeNull()
  })

  it('does not duplicate sold-out items when ALL already contains them and the off tab loads extra data', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM001',
          title: '在售商品',
          price: '88.00',
          online: true,
          status: 'ON_SALE',
          browseCount: 1,
          wantCount: 0,
        },
        {
          itemId: 'ITEM009',
          title: '已下架商品',
          price: '28.00',
          online: false,
          status: 'SOLD_OUT',
          detail: {
            headTitle: '小二下架',
            offReason: '小二下架',
            itemStatus: '-3',
          },
        },
      ],
    })
    apiMocks.itemSoldOutList.mockResolvedValue({
      _last_synced_at: 0,
      items: [
        {
          itemId: 'ITEM009',
          title: '已下架商品',
          price: '28.00',
          online: false,
          status: 'SOLD_OUT',
          detail: {
            headTitle: '小二下架',
            offReason: '小二下架',
            itemStatus: '-3',
          },
        },
      ],
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('在售商品')).toBeTruthy()
    expect(screen.queryByText('已下架商品')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /已下架/i }))
    await waitFor(async () => {
      expect((await screen.findAllByText('已下架商品')).length).toBe(1)
    })

    fireEvent.click(screen.getByRole('button', { name: /^在售/i }))
    expect(await screen.findByText('在售商品')).toBeTruthy()
    expect(screen.queryByText('已下架商品')).toBeNull()
  })

  it('prefers a confirmed on-sale record over a stale sold-out copy of the same item', async () => {
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 2,
      items: [{
        itemId: 'ITEM-STALE-SOLDOUT',
        title: '状态已转在售商品',
        price: '28.00',
        online: true,
        status: 'ON_SALE',
      }],
    })
    apiMocks.itemSoldOutList.mockResolvedValue({
      _last_synced_at: 1,
      items: [{
        itemId: 'ITEM-STALE-SOLDOUT',
        title: '状态已转在售商品',
        price: '28.00',
        online: false,
        status: 'SOLD_OUT',
      }],
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('状态已转在售商品')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /已下架/i }))
    await waitFor(() => expect(apiMocks.itemSoldOutList).toHaveBeenCalled())
    expect(screen.queryByText('状态已转在售商品')).toBeNull()
    expect(screen.getByRole('button', { name: /已下架 0/i })).toBeTruthy()
  })

  it('links default, single, multiple and restored-all account scopes to KPIs, tabs, list and batch selection', async () => {
    accountStoreMock.accounts = [
      { uid: 'u1', nick: '甲账号', status: 'active', location: {} },
      { uid: 'u2', nick: '乙账号', status: 'active', location: {} },
      { uid: 'u3', nick: '丙账号', status: 'restricted', location: {} },
    ]
    const itemsByUid = {
      u1: [{ itemId: 'U1-ITEM', title: '甲账号商品', price: '88.00', online: true, status: 'ON_SALE', browseCount: 10, wantCount: 1 }],
      u2: [{ itemId: 'U2-ITEM', title: '乙账号商品', price: '66.00', online: true, status: 'ON_SALE', browseCount: 20, wantCount: 2 }],
      u3: [{ itemId: 'U3-ITEM', title: '丙账号下架商品', price: '22.00', online: false, status: 'SOLD_OUT', browseCount: 30, wantCount: 3 }],
    }
    apiMocks.itemAllList.mockImplementation(async (uid: string) => ({
      _last_synced_at: 0,
      items: itemsByUid[uid] || [],
    }))

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findByText('甲账号商品')
    const header = screen.getByRole('heading', { name: '商品' }).closest('section')!
    const totalKpi = within(header).getByText('商品总数').parentElement!
    expect(within(totalKpi).getByText('3')).toBeTruthy()
    expect(screen.getByRole('button', { name: /在售 2/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /已下架 1/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: '商品账号选择：全部账号' })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '商品账号选择：全部账号' }))
    fireEvent.click(screen.getByRole('checkbox', { name: '选择商品账号 甲账号' }))

    expect(screen.getByRole('button', { name: '商品账号选择：甲账号' })).toBeTruthy()
    expect(within(totalKpi).getByText('1')).toBeTruthy()
    expect(screen.getByRole('button', { name: /在售 1/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /已下架 0/i })).toBeTruthy()
    expect(screen.getByText('甲账号商品')).toBeTruthy()
    expect(screen.queryByText('乙账号商品')).toBeNull()

    fireEvent.click(screen.getByRole('checkbox', { name: '选择商品账号 乙账号' }))
    expect(screen.getByRole('button', { name: '商品账号选择：已选 2 个账号' })).toBeTruthy()
    expect(within(totalKpi).getByText('2')).toBeTruthy()
    expect(screen.getByText('甲账号商品')).toBeTruthy()
    expect(screen.getByText('乙账号商品')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '选择' }))
    fireEvent.click(screen.getByRole('button', { name: '全选' }))
    expect(screen.getByText('已选 2 / 2')).toBeTruthy()

    fireEvent.click(screen.getByRole('checkbox', { name: '选择商品账号 甲账号' }))
    fireEvent.click(screen.getByRole('checkbox', { name: '选择商品账号 乙账号' }))

    expect(screen.getByRole('button', { name: '商品账号选择：全部账号' })).toBeTruthy()
    expect(within(totalKpi).getByText('3')).toBeTruthy()
    expect(screen.getByRole('button', { name: /在售 2/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /已下架 1/i })).toBeTruthy()
    expect(screen.queryByText(/已选 \d+ \/ \d+/)).toBeNull()
  })

  it('restores all accounts when the available account collection invalidates a desktop selection', async () => {
    accountStoreMock.accounts = [
      { uid: 'u1', nick: '保留账号', status: 'active', location: {} },
      { uid: 'u2', nick: '即将移除账号', status: 'active', location: {} },
    ]
    const itemsByUid = {
      u1: [{ itemId: 'U1-ITEM', title: '保留账号商品', price: '88.00', online: true, status: 'ON_SALE' }],
      u2: [{ itemId: 'U2-ITEM', title: '即将移除商品', price: '66.00', online: true, status: 'ON_SALE' }],
      u3: [{ itemId: 'U3-ITEM', title: '新增账号商品', price: '55.00', online: true, status: 'ON_SALE' }],
    }
    apiMocks.itemAllList.mockImplementation(async (uid: string) => ({
      _last_synced_at: 0,
      items: itemsByUid[uid] || [],
    }))

    const view = render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findByText('即将移除商品')
    fireEvent.click(screen.getByRole('button', { name: '商品账号选择：全部账号' }))
    fireEvent.click(screen.getByRole('checkbox', { name: '选择商品账号 即将移除账号' }))
    expect(screen.getByRole('button', { name: '商品账号选择：即将移除账号' })).toBeTruthy()

    accountStoreMock.accounts = [
      { uid: 'u1', nick: '保留账号', status: 'active', location: {} },
      { uid: 'u3', nick: '新增账号', status: 'active', location: {} },
    ]
    view.rerender(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    expect(await screen.findByRole('button', { name: '商品账号选择：全部账号' })).toBeTruthy()
    expect(await screen.findByText('新增账号商品')).toBeTruthy()
    expect(screen.getByText('保留账号商品')).toBeTruthy()
    expect(screen.queryByText('即将移除商品')).toBeNull()
  })

  it('keeps long account summaries fixed-width with truncation and a full-name title', async () => {
    const longNick = '这是一个非常非常长用于验证商品账号选择器不会挤压搜索框的账号昵称'
    accountStoreMock.accounts = [{ uid: 'u-long', nick: longNick, status: 'active', location: {} }]
    apiMocks.itemAllList.mockResolvedValue({
      _last_synced_at: 0,
      items: [{ itemId: 'LONG-ITEM', title: '长昵称账号商品', price: '88.00', online: true, status: 'ON_SALE' }],
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findByText('长昵称账号商品')
    fireEvent.click(screen.getByRole('button', { name: '商品账号选择：全部账号' }))
    const option = screen.getByRole('checkbox', { name: `选择商品账号 ${longNick}` })
    expect(option.closest('label')?.querySelector('.truncate')).toBeTruthy()
    fireEvent.click(option)

    const trigger = screen.getByRole('button', { name: `商品账号选择：${longNick}` })
    expect(trigger.getAttribute('title')).toBe(longNick)
    expect(trigger.classList.contains('w-[168px]')).toBe(true)
    expect(trigger.classList.contains('max-w-[168px]')).toBe(true)
    expect(trigger.querySelector('.truncate')).toBeTruthy()
  })

  it('closes the account menu on outside click and restores trigger focus on Escape', async () => {
    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findAllByText('测试商品')
    const trigger = screen.getByRole('button', { name: '商品账号选择：全部账号' })
    fireEvent.click(trigger)
    expect(screen.getByRole('group', { name: '商品账号选项' })).toBeTruthy()
    fireEvent.mouseDown(document.body)
    expect(screen.queryByRole('group', { name: '商品账号选项' })).toBeNull()

    fireEvent.click(trigger)
    const option = screen.getByRole('checkbox', { name: '选择商品账号 测试账号' })
    option.focus()
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByRole('group', { name: '商品账号选项' })).toBeNull()
    expect(document.activeElement).toBe(trigger)
  })

  it('scopes sync and polish actions to the selected desktop accounts', async () => {
    accountStoreMock.accounts = [
      { uid: 'u1', nick: '甲账号', status: 'active', location: {} },
      { uid: 'u2', nick: '乙账号', status: 'active', location: {} },
    ]
    apiMocks.itemAllList.mockImplementation(async (uid: string) => ({
      _last_synced_at: 0,
      items: [{ itemId: `${uid}-ITEM`, title: `${uid}商品`, price: '88.00', online: true, status: 'ON_SALE' }],
    }))
    apiMocks.syncItems.mockResolvedValue({ ok: true })
    apiMocks.itemPolish.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: {} })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    await screen.findByText('u2商品')
    fireEvent.click(screen.getByRole('button', { name: '商品账号选择：全部账号' }))
    fireEvent.click(screen.getByRole('checkbox', { name: '选择商品账号 乙账号' }))

    fireEvent.click(screen.getAllByRole('button', { name: '同步' }).at(-1)!)
    await waitFor(() => expect(apiMocks.syncItems).toHaveBeenCalledWith('u2'))
    expect(apiMocks.syncItems).not.toHaveBeenCalledWith('u1')

    fireEvent.click(screen.getByRole('button', { name: '擦亮' }))
    expect(await screen.findByText('确认对 1 个账号的 1 件在售商品逐个擦亮？')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '确认' }))

    await waitFor(() => expect(apiMocks.itemPolish).toHaveBeenCalledWith('u2', 'u2-ITEM'))
    expect(apiMocks.itemPolish).not.toHaveBeenCalledWith('u1', 'u1-ITEM')
  })

  it('counts restricted accounts in items totals and off-shelf tab', async () => {
    accountStoreMock.accounts = [
      ...defaultAccounts,
      {
        uid: 'u-restricted',
        nick: '受限账号',
        status: 'restricted',
        location: { province: '湖北省', city: '武汉市', area: '武昌区', divisionId: '420106' },
      },
    ]
    apiMocks.itemAllList.mockImplementation(async (uid: string) => {
      if (uid === 'u-restricted') {
        return {
          _last_synced_at: 0,
          items: [
            {
              itemId: 'ITEM-R1',
              title: '受限账号下架商品',
              price: '28.00',
              online: false,
              status: 'SOLD_OUT',
              detail: {
                headTitle: '小二下架',
                offReason: '小二下架',
                itemStatus: '-3',
              },
            },
          ],
        }
      }
      return {
        _last_synced_at: 0,
        items: [
          {
            itemId: 'ITEM001',
            title: '测试商品',
            price: '88.00',
            online: true,
            status: 'ON_SALE',
            browseCount: 1,
            wantCount: 0,
          },
        ],
      }
    })
    apiMocks.itemSoldOutList.mockResolvedValue({
      _last_synced_at: 0,
      items: [],
    })

    render(
      <MemoryRouter>
        <ItemsPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText(/商品总数/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /已下架 1/i })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '商品账号选择：全部账号' }))
    expect(screen.getByRole('checkbox', { name: '选择商品账号 受限账号' })).toBeTruthy()
  })
})
