import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Navigate, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SuperPolishPage from './SuperPolishPage'
import { SuperPolishCampaignListPage } from './campaigns/SuperPolishCampaignListPage'
import { SuperPolishCampaignCreatePage } from './campaigns/SuperPolishCampaignCreatePage'
import { SuperPolishCampaignDetailPage } from './campaigns/SuperPolishCampaignDetailPage'
import { SuperPolishReportsPage } from './reports/SuperPolishReportsPage'
import { compactDate, daysAgoInput } from './model'

const { apiMocks, accountStoreMock, clientMocks, dialogMocks } = vi.hoisted(() => ({
  apiMocks: {
    getSuperPolishBootstrap: vi.fn(),
    getSuperPolishHome: vi.fn(),
    getSuperPolishAccountInfo: vi.fn(),
    getSuperPolishLoginInfo: vi.fn(),
    getSuperPolishAccountCard: vi.fn(),
    getSuperPolishUnreadMessages: vi.fn(),
    getSuperPolishMemberData: vi.fn(),
    getSuperPolishActivityReward: vi.fn(),
    getSuperPolishCoupons: vi.fn(),
    getSuperPolishCouponStatistics: vi.fn(),
    getSuperPolishQualificationCategories: vi.fn(),
    getSuperPolishQualificationRecords: vi.fn(),
    getSuperPolishCampaigns: vi.fn(),
    getSuperPolishCampaignGroups: vi.fn(),
    getSuperPolishCampaignPrecheck: vi.fn(),
    getSuperPolishRecommendedItems: vi.fn(),
    searchSuperPolishItems: vi.fn(),
    querySuperPolishPackages: vi.fn(),
    estimateSuperPolishBids: vi.fn(),
    createSuperPolishCampaign: vi.fn(),
    editSuperPolishCampaign: vi.fn(),
    getSuperPolishAccountReport: vi.fn(),
    getSuperPolishCampaignReport: vi.fn(),
    getSuperPolishItemReport: vi.fn(),
    getSuperPolishGroupReport: vi.fn(),
    unwrapMtopData: vi.fn((payload: unknown) => (payload as { data?: unknown })?.data ?? payload),
  },
  accountStoreMock: {
    activeAccounts: [{ uid: 'u1', nick: '主账号', status: 'active' }],
    loading: false,
    loadAccounts: vi.fn(),
  },
  clientMocks: {
    getAuthToken: vi.fn(() => 'token-a'),
    retStr: vi.fn((payload: unknown) => {
      const ret = (payload as { ret?: unknown })?.ret
      return Array.isArray(ret) ? String(ret[0] ?? '') : String(ret ?? '')
    }),
    retMsg: vi.fn((payload: unknown, fallback = '操作失败') => {
      const ret = Array.isArray((payload as { ret?: unknown })?.ret)
        ? String((payload as { ret?: unknown[] }).ret?.[0] ?? '')
        : String((payload as { ret?: unknown }).ret ?? '')
      if (!ret) return fallback
      const index = ret.indexOf('::')
      const message = index >= 0 ? ret.slice(index + 2) : ret
      return message.replace(/\|\|.*$/, '').trim() || fallback
    }),
    isSuccess: vi.fn((payload: unknown) => {
      const ret = Array.isArray((payload as { ret?: unknown })?.ret)
        ? String((payload as { ret?: unknown[] }).ret?.[0] ?? '')
        : String((payload as { ret?: unknown }).ret ?? '')
      return ret.toUpperCase().includes('SUCCESS')
    }),
  },
  dialogMocks: {
    showConfirm: vi.fn(),
  },
}))

const marketMocks = vi.hoisted(() => ({
  locationDivisions: vi.fn(),
  locationDivisionChildren: vi.fn(),
}))

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false }),
}))

vi.mock('../../components/Notify', () => ({
  useSafeNotify: () => vi.fn(),
}))

vi.mock('../../components/AppDialog', () => ({
  useAppDialog: () => dialogMocks,
}))

vi.mock('../../api/client', () => clientMocks)
vi.mock('../../api/superPolish', () => apiMocks)
vi.mock('../../api/market', () => marketMocks)

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function renderSuperPolish(initialEntry = '/super-polish') {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <Routes>
        <Route path="/super-polish" element={<SuperPolishPage />}>
          <Route index element={<Navigate to="campaigns" replace />} />
          <Route path="campaigns" element={<SuperPolishCampaignListPage />} />
          <Route path="campaigns/new" element={<SuperPolishCampaignCreatePage />} />
          <Route path="campaigns/:campaignId" element={<SuperPolishCampaignDetailPage />} />
          <Route path="account" element={<Navigate to="../campaigns" replace />} />
          <Route path="reports" element={<SuperPolishReportsPage />} />
        </Route>
      </Routes>
    </MemoryRouter>,
  )
}

function makeDetailCampaign() {
  return {
    campaignId: 75543917,
    adGroupId: 100990381,
    title: '鱼小铺测试计划',
    itemId: 2001,
    feedEntityId: 2001,
    catId: 123,
    price: 9900,
    bidTarget: 'chat',
    bidType: 'smart_bid',
    promotionMode: 'custom',
    creativeOptSwitch: true,
    showCreativeOptSwitch: true,
    maxDayBudget: 111100,
    budget: 111100,
    bid: 400,
    cpm: 400,
    statusDescTitle: '账户余额不足，系统暂停',
    statusGroup: 'balance_paused',
    targetInfo: {
      divisionList: [
        { province: '山东', area: '济南', divisionId: 370100 },
      ],
    },
    budgetScheduleDurations: [
      '0', '1', '2', '3', '4', '5',
      '6', '7', '8', '9', '10', '11',
      '12', '13', '14', '15', '16', '17',
      '18', '19', '20', '21', '22', '23',
    ],
    entityList: [
      {
        itemId: 2001,
        catId: 123,
        title: '当前宝贝A',
        price: 9900,
        imgUrl: 'https://example.com/a.jpg',
        publishTime: 1779873771000,
        saleCnt: 1,
        supportAd: true,
        supportAreaTarget: true,
        areaTarget: true,
        tags: ['可城市定向'],
      },
      {
        itemId: 2002,
        catId: 456,
        title: '当前宝贝B',
        price: 19900,
        imgUrl: 'https://example.com/b.jpg',
        publishTime: 1779873927000,
        saleCnt: 0,
        supportAd: true,
        supportAreaTarget: false,
        areaTarget: false,
        tags: [],
      },
    ],
    adDataDetailVO: {
      clickPv: 0,
      chatPv: 0,
      cost: 0,
    },
  }
}

function makeDetailGroups() {
  return [
    {
      adGroupId: 100990381,
      campaignId: 75543917,
      entityId: 2001,
      title: '当前宝贝A',
      imgUrl: 'https://example.com/a.jpg',
      price: 9900,
      saleCnt: 1,
      adData: {
        clickPv: 3,
        chatPv: 1,
        cost: 260,
      },
      supportAreaTarget: true,
      tags: ['可城市定向'],
    },
    {
      adGroupId: 100990382,
      campaignId: 75543917,
      entityId: 2002,
      title: '当前宝贝B',
      imgUrl: 'https://example.com/b.jpg',
      price: 19900,
      saleCnt: 0,
      adData: {
        clickPv: 2,
        chatPv: 0,
        cost: 140,
      },
      supportAreaTarget: false,
      tags: [],
    },
  ]
}

describe('SuperPolishPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    accountStoreMock.activeAccounts = [{ uid: 'u1', nick: '主账号', status: 'active' }]
    accountStoreMock.loading = false
    accountStoreMock.loadAccounts.mockResolvedValue(accountStoreMock.activeAccounts)
    clientMocks.getAuthToken.mockReturnValue('token-a')
    dialogMocks.showConfirm.mockResolvedValue(true)

    apiMocks.getSuperPolishBootstrap.mockResolvedValue({ data: { result: true } })
    apiMocks.getSuperPolishHome.mockResolvedValue({
      data: {
        allAccountBalanceDetails: [
          { accountName: '现金', balance: 188 },
          { accountName: '红包', balance: 20 },
        ],
        campaignInfo: {
          allCampaignNum: 4,
          activeCampaignNum: 2,
          pausedCampaignNum: 1,
        },
        memberData: {
          data: {
            clickPv: 12,
            cost: 45,
            chatPv: 3,
          },
        },
      },
    })
    apiMocks.getSuperPolishAccountInfo.mockResolvedValue({
      data: {
        usableBalance: 168,
        totalBalance: 200,
        needReCharge: false,
        rechargeAmountList: [5000, 10000],
      },
    })
    apiMocks.getSuperPolishLoginInfo.mockResolvedValue({ data: { displayNick: '主账号操作员' } })
    apiMocks.getSuperPolishAccountCard.mockResolvedValue({ data: { cashBalanceOfYuan: '168.00' } })
    apiMocks.getSuperPolishUnreadMessages.mockResolvedValue({ data: { unreadCount: 3 } })
    apiMocks.getSuperPolishMemberData.mockResolvedValue({ data: { data: { cost: 45 } } })
    apiMocks.getSuperPolishActivityReward.mockResolvedValue({ data: {} })
    apiMocks.getSuperPolishCoupons.mockResolvedValue({ data: { result: [] } })
    apiMocks.getSuperPolishCouponStatistics.mockResolvedValue({
      data: {
        activatedCouponAmountOfYuan: '8',
        expiringCouponAmountOfYuan: '0',
        specialCouponAmountOfYuan: '0',
      },
    })
    apiMocks.getSuperPolishQualificationCategories.mockResolvedValue({ data: { result: [] } })
    apiMocks.getSuperPolishQualificationRecords.mockResolvedValue({ data: { result: [] } })
    apiMocks.getSuperPolishCampaigns.mockResolvedValue({
      data: {
        result: [{ campaignId: 1001, itemTitle: '计划一', status: '投放中' }],
        nextPage: false,
      },
    })
    apiMocks.getSuperPolishCampaignGroups.mockResolvedValue({ data: { result: [] } })
    apiMocks.getSuperPolishCampaignPrecheck.mockResolvedValue({
      data: {
        oldMember: true,
        extraFunctions: { showCreativeOptSwitch: true },
      },
    })
    apiMocks.getSuperPolishRecommendedItems.mockResolvedValue({
      data: {
        result: [{
          itemId: 2001,
          title: '推荐商品',
          catId: 123,
          price: 998,
          checked: true,
          supportAd: true,
          supportAreaTarget: true,
        }],
      },
    })
    apiMocks.searchSuperPolishItems.mockResolvedValue({
      data: {
        result: {
          tabList: [
            { tab: 'all', tabName: '全部', count: 2 },
            { tab: 'platReco', tabName: '平台推荐', count: 2, tabTag: 'new' },
            { tab: 'hot', tabName: '爆品', count: 0 },
            { tab: 'new', tabName: '新品', count: 0 },
            { tab: 'top', tabName: '行业尖货', count: 0 },
            { tab: 'city', tabName: '可城市定向', count: 1 },
          ],
          itemList: [{
            itemId: 2002,
            title: '搜索结果商品',
            catId: 123,
            price: 1098,
            supportAd: true,
            supportAreaTarget: true,
          }, {
            itemId: 2003,
            title: '城市定向商品',
            catId: 124,
            price: 2098,
            supportAd: true,
            supportAreaTarget: true,
          }],
        },
      },
    })
    apiMocks.querySuperPolishPackages.mockResolvedValue({
      data: {
        bidTargets: ['click'],
        packageTargetVOList: [{
          bidTarget: 'chat',
          recommendPackageType: '标准推荐',
          orderVOList: [
            { orderPrice: 2990, bidPrice: 310, bidType: 'smart_bid', algoRecommend: 1, orderStartDuration: 3, orderEndDuration: 7, orderImprove: [8, 12], title: '智能推荐套餐', promotionText: '限时成本保障', packageType: 'chat_recommend' },
            { orderPrice: 5990, bidPrice: 310, bidType: 'smart_bid', algoRecommend: 0, orderStartDuration: 5, orderEndDuration: 10, orderImprove: [16, 24], title: '智能推荐套餐', promotionText: '限时成本保障', packageType: 'chat_plus' },
          ],
        }],
      },
    })
    apiMocks.estimateSuperPolishBids.mockResolvedValue({
      data: {
        bidTargets: ['chat'],
        recBidTarget: 'chat',
        customizeBidRecommendVOList: [{
          bidTarget: 'chat',
          bidPrice: 390,
          customBidPrice: 460,
          bidType: 'smart_bid',
          targetRecommendBudget: 5000,
          recommendBudgetList: [5000, 15000],
          extraFunctions: { showCreativeOptSwitch: true },
        }],
      },
    })
    apiMocks.createSuperPolishCampaign.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        createSuccess: true,
        campaignIds: [75543917],
      },
    })
    apiMocks.editSuperPolishCampaign.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: {} })
    apiMocks.getSuperPolishAccountReport.mockResolvedValue({ data: { data: { cost: 45 } } })
    apiMocks.getSuperPolishCampaignReport.mockResolvedValue({ data: { result: [] } })
    apiMocks.getSuperPolishItemReport.mockResolvedValue({ data: { result: [] } })
    apiMocks.getSuperPolishGroupReport.mockResolvedValue({ data: { data: { clickPv: 1 } } })
    marketMocks.locationDivisions.mockResolvedValue({
      divisionArea: [
        { id: 370000, divisionId: 370000, na: '山东省', level: 'province', prov: '山东省', hasChild: true },
      ],
    })
    marketMocks.locationDivisionChildren.mockImplementation(async (_uid: string, parentId: number) => {
      if (parentId === 370000) {
        return {
          items: [
            { id: 370100, divisionId: 370100, na: '济南市', level: 'city', prov: '山东省', city: '济南市', hasChild: false },
            { id: 370200, divisionId: 370200, na: '青岛市', level: 'city', prov: '山东省', city: '青岛市', hasChild: false },
          ],
        }
      }
      return { items: [] }
    })
  })

  it('renders the built-campaign workspace with only the three approved top-level entries', async () => {
    renderSuperPolish('/super-polish/campaigns')

    expect(await screen.findByText('超级擦亮')).toBeTruthy()
    expect(screen.getByRole('link', { name: '已建计划' })).toBeTruthy()
    expect(screen.getByRole('link', { name: '新建计划' })).toBeTruthy()
    expect(screen.getByRole('link', { name: '计划报表' })).toBeTruthy()
    expect(screen.queryByRole('link', { name: '首页' })).toBeNull()
    expect(screen.queryByRole('link', { name: '计划' })).toBeNull()
    expect(screen.queryByRole('link', { name: '账户与资质' })).toBeNull()
    expect(screen.getByRole('heading', { name: '已建计划' })).toBeTruthy()
    expect(await screen.findByText('计划一')).toBeTruthy()
    expect(await screen.findByText('可用余额')).toBeTruthy()
    expect(screen.getByText('¥1.68')).toBeTruthy()
    expect(screen.getByText('¥2')).toBeTruthy()
    expect(apiMocks.getSuperPolishCampaigns).toHaveBeenCalledWith('u1', expect.objectContaining({
      pageNum: 1,
      pageSize: 10,
      startDate: compactDate(daysAgoInput(365)),
      includeEndDate: true,
    }))
    expect(apiMocks.getSuperPolishHome).not.toHaveBeenCalled()
    expect(apiMocks.getSuperPolishAccountInfo).toHaveBeenCalledWith('u1')
  })

  it('loads dedicated creation helpers inside the new campaign route', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText('计划能力已校验，可直接从平台推荐或搜索商品开始创建。')).toBeTruthy()
    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    expect(screen.getByRole('button', { name: '新增宝贝' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '套餐擦亮' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '自定义擦亮' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '展开' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '展开' }))
    expect(screen.getByRole('button', { name: '全部地域' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '全部时段' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '立即擦亮' })).toBeTruthy()
    expect(apiMocks.getSuperPolishCampaignPrecheck).toHaveBeenCalledWith('u1')
    expect(apiMocks.getSuperPolishRecommendedItems).toHaveBeenCalledWith('u1')
  })

  it('shows an explicit session-expired error instead of treating failed campaign reads as empty results', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValue({
      ret: ['FAIL_SYS_SESSION_EXPIRED::Session过期'],
      data: {},
    })

    renderSuperPolish('/super-polish/campaigns')

    expect(await screen.findByText('账号会话已过期，无法读取超级擦亮数据，请先刷新账号后重试。')).toBeTruthy()
    expect(screen.getByText('当前无法读取计划列表，请先处理上方错误后重试。')).toBeTruthy()
    expect(screen.queryByText('当前筛选条件下没有返回计划。')).toBeNull()
  })

  it('loads campaign report automatically when entering the reports workspace and hides other report layers', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValue({
      data: {
        result: [
          {
            campaignId: 75543917,
            title: '擦亮宝贝A',
            statusDescTitle: '擦亮已完成',
          },
        ],
      },
    })
    let returnedReportRow = false
    apiMocks.getSuperPolishCampaignReport.mockImplementation(async () => {
      if (!returnedReportRow) {
        returnedReportRow = true
        return {
          data: {
            result: [
              {
                campaignId: 75543917,
                adGroupIds: [100990381],
                status: 2,
                title: '擦亮宝贝A',
                idleAdGroupDataVO: {
                  data: {
                    clickPv: 4,
                    chatPv: 1,
                    favorites: 2,
                    cost: 300,
                    showPv: 55,
                    inquiries: 1,
                    payPv: 0,
                    realCost: 300,
                  },
                  dataDetailVOList: [
                    {
                      time: '2026-06-16 00:00:00',
                      clickPv: 4,
                      chatPv: 1,
                      favorites: 2,
                      cost: 300,
                      showPv: 55,
                      inquiries: 1,
                      payPv: 0,
                      realCost: 300,
                    },
                  ],
                },
              },
            ],
            total: 1,
          },
        }
      }

      return {
        data: {
          result: [],
          total: 0,
        },
      }
    })

    renderSuperPolish('/super-polish/reports')

    await waitFor(() => expect(apiMocks.getSuperPolishCampaignReport).toHaveBeenCalledWith('u1', expect.objectContaining({
      pageNum: 1,
      pageSize: 50,
      includeEndDate: true,
      allPeriod: false,
    })))
    expect(screen.getByRole('heading', { name: '计划报表' })).toBeTruthy()
    expect(await screen.findByText('共 1 条计划记录')).toBeTruthy()
    expect(screen.getByText('擦亮宝贝A')).toBeTruthy()
    expect(screen.getByText('擦亮已完成')).toBeTruthy()
    expect(screen.getAllByText((_, element) => element?.textContent?.includes('曝光：55') ?? false).length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: '账户报表' })).toBeNull()
    expect(screen.queryByRole('button', { name: '商品报表' })).toBeNull()
    expect(screen.queryByRole('button', { name: '分组报表' })).toBeNull()
    expect(apiMocks.getSuperPolishCampaigns).toHaveBeenCalledWith('u1', expect.objectContaining({
      pageNum: 1,
      pageSize: 50,
      includeEndDate: true,
    }))
  })

  it('keeps the report workspace focused on campaign rows instead of exposing item or group report modes', async () => {
    apiMocks.getSuperPolishCampaignReport.mockResolvedValue({
      data: {
        result: [
          {
            campaignId: 75543917,
            adGroupIds: [100990381],
            status: -2,
            title: '擦亮宝贝A',
            idleAdGroupDataVO: {
              data: {
                clickPv: 4,
                chatPv: 1,
                favorites: 2,
                cost: 300,
                showPv: 55,
                inquiries: 1,
                payPv: 0,
                realCost: 300,
              },
              dataDetailVOList: [
                {
                  time: '2026-06-12 00:00:00',
                  clickPv: 4,
                  chatPv: 1,
                  favorites: 2,
                  cost: 300,
                  showPv: 55,
                  inquiries: 1,
                  payPv: 0,
                  realCost: 300,
                },
              ],
            },
          },
        ],
        total: 1,
      },
    })

    renderSuperPolish('/super-polish/reports')

    await waitFor(() => expect(apiMocks.getSuperPolishCampaignReport).toHaveBeenCalledWith('u1', expect.objectContaining({
      pageNum: 1,
      pageSize: 50,
      includeEndDate: true,
      allPeriod: false,
    })))
    expect(await screen.findByText('擦亮宝贝A')).toBeTruthy()
    expect(screen.getByText(/计划 ID 75543917/)).toBeTruthy()
    expect(screen.queryByText('报表类型')).toBeNull()
    expect(screen.queryByText('统计区间')).toBeNull()
    expect(screen.queryByText('返回计划')).toBeNull()
    expect(screen.getByText('暂停 / 完成 1')).toBeTruthy()
    expect(screen.getByText('共 1 条计划记录')).toBeTruthy()
    expect(screen.queryByText('商品报表')).toBeNull()
    expect(screen.queryByText('分组报表')).toBeNull()
    expect(apiMocks.getSuperPolishCampaignGroups).not.toHaveBeenCalled()
    expect(apiMocks.getSuperPolishGroupReport).not.toHaveBeenCalled()
  })

  it('keeps campaign list counts in the list header instead of a standalone metric card', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValue({
      data: {
        result: [
          {
            campaignId: 75543917,
            itemId: 2001,
            title: '擦亮宝贝A',
            statusDesc: '投放中',
            adDataDetailVO: { cost: 300, clickPv: 4, chatPv: 1, favorites: 2 },
          },
        ],
      },
    })

    renderSuperPolish('/super-polish/campaigns')

    expect(await screen.findByText('计划列表')).toBeTruthy()
    expect(screen.getByText('共 1 个计划')).toBeTruthy()
    expect(screen.queryByText('当前返回计划')).toBeNull()
  })

  it('prefers idleAdGroupDataVO metrics and statusDescTitle for older built campaigns', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValue({
      data: {
        result: [
          {
            campaignId: 53032012,
            itemId: 2001,
            itemTitle: '历史擦亮计划',
            statusDesc: '投放中',
            statusDescTitle: '手动暂停',
            adDataDetailVO: {
              cost: 0,
              clickPv: 0,
              chatPv: 0,
              favorites: 0,
            },
            idleAdGroupDataVO: {
              data: {
                cost: 536,
                clickPv: 8,
                chatPv: 2,
                favorites: 5,
              },
            },
          },
        ],
        total: 1,
      },
    })

    renderSuperPolish('/super-polish/campaigns')

    expect(await screen.findByText('历史擦亮计划')).toBeTruthy()
    expect(screen.getByText('手动暂停')).toBeTruthy()
    expect(screen.getByText('消耗：¥5.36')).toBeTruthy()
    expect(screen.getByText('点击：8')).toBeTruthy()
    expect(screen.getByText('咨询：2')).toBeTruthy()
    expect(screen.getByText('收藏：5')).toBeTruthy()
  })

  it('formats campaign list and report spend from cents to yuan', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValue({
      data: {
        result: [
          {
            campaignId: 75543917,
            itemId: 2001,
            title: '擦亮宝贝A',
            statusDescTitle: '擦亮已完成',
            adDataDetailVO: {
              cost: 300,
              clickPv: 4,
              chatPv: 1,
              favorites: 2,
            },
          },
        ],
        total: 1,
      },
    })
    apiMocks.getSuperPolishCampaignReport.mockResolvedValue({
      data: {
        result: [
          {
            campaignId: 75543917,
            adGroupIds: [100990381],
            status: 99,
            title: '擦亮宝贝A',
            idleAdGroupDataVO: {
              data: {
                clickPv: 4,
                chatPv: 1,
                favorites: 2,
                cost: 300,
                showPv: 55,
                inquiries: 1,
                payPv: 0,
                realCost: 300,
              },
              dataDetailVOList: [
                {
                  time: '2026-06-12 00:00:00',
                  clickPv: 4,
                  chatPv: 1,
                  favorites: 2,
                  cost: 300,
                  showPv: 55,
                  inquiries: 1,
                  payPv: 0,
                  realCost: 300,
                },
              ],
            },
          },
        ],
        total: 1,
      },
    })

    renderSuperPolish('/super-polish/campaigns')
    expect(await screen.findByText('消耗：¥3')).toBeTruthy()

    renderSuperPolish('/super-polish/reports')
    expect(await screen.findByText('消耗：¥3')).toBeTruthy()
  })

  it('filters built campaigns by completed campaign status', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValue({
      data: { result: [], total: 0 },
    })

    renderSuperPolish('/super-polish/campaigns')

    const statusSelect = await screen.findByLabelText('状态组')
    fireEvent.change(statusSelect, { target: { value: 'COMPLETED' } })

    await waitFor(() => {
      expect(apiMocks.getSuperPolishCampaigns).toHaveBeenCalledWith(
        'u1',
        expect.objectContaining({
          statusGroup: [],
          campaignStatus: [99],
        }),
      )
    })
  })

  it('loads picker tabs from item.searchV2 and merges visible items into the selected pool', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '新增宝贝' }))

    await waitFor(() => expect(apiMocks.searchSuperPolishItems).toHaveBeenCalledWith('u1', {
      pageNum: 1,
      pageSize: 10,
      tab: 'platReco',
      needTabs: true,
      searchKey: '',
    }))
    expect(await screen.findByRole('button', { name: /平台推荐/ })).toBeTruthy()
    expect(screen.getByRole('button', { name: /可城市定向/ })).toBeTruthy()
    expect(await screen.findByText('搜索结果商品')).toBeTruthy()
    expect(screen.getByRole('button', { name: '确定' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '全部添加' }))

    expect(await screen.findByText(/选择宝贝\(3\/10\)/)).toBeTruthy()
    expect(screen.getByText('推荐商品')).toBeTruthy()
    expect(screen.getAllByText('搜索结果商品').length).toBeGreaterThan(0)
  })

  it('disables creation when the selection pool is empty', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '清空' }))
    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalled())

    expect(await screen.findByText('当前还没有添加宝贝。')).toBeTruthy()
    expect((screen.getByRole('button', { name: '立即擦亮' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('renders package mode with editable package amounts from orderPrice', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    await waitFor(() => expect(apiMocks.querySuperPolishPackages).toHaveBeenCalledWith('u1', {
      entityType: 1,
      isGoodsSelectedTabChange: true,
      entityList: [{
        entityId: 2001,
        catId: 123,
        price: 998,
        areaTarget: false,
        startTime: expect.any(Number),
      }],
    }))
    await waitFor(() => expect(apiMocks.estimateSuperPolishBids).toHaveBeenCalledWith('u1', {
      entityType: 1,
      pcFirstRequest: true,
      entityList: [{
        entityId: 2001,
        catId: 123,
        price: 998,
        areaTarget: false,
        startTime: expect.any(Number),
      }],
    }))
    expect(await screen.findByText('¥29.9')).toBeTruthy()
    expect(screen.getByRole('button', { name: '编辑套餐' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '编辑套餐' }))
    expect(await screen.findByText('选择套餐')).toBeTruthy()
    expect(screen.getByRole('button', { name: '确定' })).toBeTruthy()
  })

  it('submits the package create payload with orderImprove from the selected package', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    expect(await screen.findByText('¥29.9')).toBeTruthy()

    await waitFor(() => expect((screen.getByRole('button', { name: '立即擦亮' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: '立即擦亮' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalled())
    await waitFor(() => expect(apiMocks.createSuperPolishCampaign).toHaveBeenCalled())

    const [, payload] = apiMocks.createSuperPolishCampaign.mock.calls[0] as [string, Record<string, unknown>]
    expect(payload).toMatchObject({
      entityType: 1,
      fromScene: 'pc_campaign-manage_yuxiaopu',
      oldMember: true,
      baseInfo: {
        productId: 1,
        promotionMode: 'order',
        bidType: 'smart_bid',
        bidTarget: 'chat',
      },
      orderInfo: {
        orderPrice: 2990,
        orderStartDuration: 3,
        orderEndDuration: 7,
        orderImprove: [8, 12],
        packageType: 'chat_recommend',
      },
      clueInfo: {
        chatButton: true,
        bookButton: false,
        bookText: '',
      },
      extraFunctions: {
        creativeOptSwitch: true,
      },
    })
  })

  it('keeps city targeting enabled when only part of the selected items support it and marks targeting per item in strategy requests', async () => {
    apiMocks.getSuperPolishRecommendedItems.mockResolvedValueOnce({
      data: {
        result: [
          {
            itemId: 2001,
            title: '推荐商品',
            catId: 123,
            price: 998,
            checked: true,
            supportAd: true,
            supportAreaTarget: true,
          },
          {
            itemId: 2009,
            title: '普通商品',
            catId: 456,
            price: 1998,
            checked: true,
            supportAd: true,
            supportAreaTarget: false,
          },
        ],
      },
    })

    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(2\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '展开' }))
    expect(screen.getByText('支持城市定向 1/2')).toBeTruthy()
    expect(screen.getByText('支持定向的宝贝：')).toBeTruthy()
    expect((screen.getByRole('button', { name: '全部地域' }) as HTMLButtonElement).disabled).toBe(false)

    fireEvent.click(screen.getByRole('button', { name: '全部地域' }))
    expect((await screen.findAllByText('其余 1 个宝贝仍按全部地域投放。')).length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '山东' }))
    await waitFor(() => expect(marketMocks.locationDivisionChildren).toHaveBeenCalledWith('u1', 370000))
    fireEvent.click(await screen.findByLabelText('济南市'))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    await waitFor(() => expect(apiMocks.querySuperPolishPackages).toHaveBeenLastCalledWith('u1', {
      entityType: 1,
      isGoodsSelectedTabChange: true,
      entityList: [
        {
          entityId: 2001,
          catId: 123,
          price: 998,
          areaTarget: true,
          startTime: expect.any(Number),
        },
        {
          entityId: 2009,
          catId: 456,
          price: 1998,
          areaTarget: false,
          startTime: expect.any(Number),
        },
      ],
    }))
    await waitFor(() => expect(apiMocks.estimateSuperPolishBids).toHaveBeenLastCalledWith('u1', {
      entityType: 1,
      pcFirstRequest: true,
      entityList: [
        {
          entityId: 2001,
          catId: 123,
          price: 998,
          areaTarget: true,
          startTime: expect.any(Number),
        },
        {
          entityId: 2009,
          catId: 456,
          price: 1998,
          areaTarget: false,
          startTime: expect.any(Number),
        },
      ],
    }))
  })

  it('switches city-targeted create to custom mode and keeps targeting scoped per supported item', async () => {
    apiMocks.getSuperPolishRecommendedItems.mockResolvedValueOnce({
      data: {
        result: [
          {
            itemId: 2001,
            title: '推荐商品',
            catId: 123,
            price: 998,
            checked: true,
            supportAd: true,
            supportAreaTarget: true,
          },
          {
            itemId: 2009,
            title: '普通商品',
            catId: 456,
            price: 1998,
            checked: true,
            supportAd: true,
            supportAreaTarget: false,
          },
        ],
      },
    })

    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(2\/10\)/)).toBeTruthy()
    fireEvent.change(screen.getByLabelText('计划名称'), { target: { value: '擦亮宝贝2026061222350011' } })
    fireEvent.click(screen.getByRole('button', { name: '展开' }))
    fireEvent.click(screen.getByRole('button', { name: '全部地域' }))
    fireEvent.click(await screen.findByRole('button', { name: '山东' }))
    await waitFor(() => expect(marketMocks.locationDivisionChildren).toHaveBeenCalledWith('u1', 370000))
    fireEvent.click(await screen.findByLabelText('济南市'))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))
    await waitFor(() => expect((screen.getByRole('button', { name: '立即擦亮' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: '立即擦亮' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalled())
    await waitFor(() => expect(apiMocks.createSuperPolishCampaign).toHaveBeenCalledWith('u1', {
      entityType: 1,
      fromScene: 'pc_campaign-manage_yuxiaopu',
      oldMember: true,
      baseInfo: {
        productId: 1,
        promotionMode: 'custom',
        bidType: 'smart_bid',
        bidTarget: 'chat',
        title: '擦亮宝贝2026061222350011',
      },
      customInfo: {
        bidPrice: 390,
        maxDayBudget: 5000,
        budgetScheduleDurations: [
          0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
          12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
        ],
      },
      targetInfo: {
        divisionList: [
          { province: '山东', area: '济南', divisionId: 370100 },
        ],
      },
      clueInfo: {
        chatButton: true,
        bookButton: false,
        bookText: '',
      },
      extraFunctions: {
        creativeOptSwitch: true,
      },
      entityList: [
        {
          entityId: 2001,
          catId: 123,
          price: 998,
          areaTarget: true,
          startTime: expect.any(Number),
        },
        {
          entityId: 2009,
          catId: 456,
          price: 1998,
          areaTarget: false,
          startTime: expect.any(Number),
        },
      ],
    }))
  })

  it('submits the custom create payload with advanced settings after confirmation', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    expect(await screen.findByText('¥29.9')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '自定义擦亮' }))
    fireEvent.change(screen.getByLabelText('计划名称'), { target: { value: '擦亮宝贝2026061222350011' } })
    fireEvent.click(screen.getByRole('button', { name: '自定义' }))
    fireEvent.change(screen.getByLabelText('自定义出价'), { target: { value: '4' } })
    fireEvent.change(screen.getByLabelText('自定义预算'), { target: { value: '1111' } })
    fireEvent.click(screen.getByRole('button', { name: '展开' }))
    fireEvent.click(screen.getByRole('button', { name: '全部时段' }))
    fireEvent.click(screen.getByRole('button', { name: '20时' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))
    expect(await screen.findByRole('button', { name: '部分时段' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '全部地域' }))
    fireEvent.click(await screen.findByRole('button', { name: '山东' }))
    await waitFor(() => expect(marketMocks.locationDivisionChildren).toHaveBeenCalledWith('u1', 370000))
    fireEvent.click(await screen.findByText('济南市'))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))
    await waitFor(() => expect((screen.getByRole('button', { name: '立即擦亮' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: '立即擦亮' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalled())
    await waitFor(() => expect(apiMocks.createSuperPolishCampaign).toHaveBeenCalledWith('u1', {
      entityType: 1,
      fromScene: 'pc_campaign-manage_yuxiaopu',
      oldMember: true,
      baseInfo: {
        productId: 1,
        promotionMode: 'custom',
        bidType: 'smart_bid',
        bidTarget: 'chat',
        title: '擦亮宝贝2026061222350011',
      },
      customInfo: {
        bidPrice: 400,
        maxDayBudget: 111100,
        budgetScheduleDurations: [
          0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
          12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23,
        ],
      },
      targetInfo: {
        divisionList: [
          { province: '山东', area: '济南', divisionId: 370100 },
        ],
      },
      clueInfo: {
        chatButton: true,
        bookButton: false,
        bookText: '',
      },
      extraFunctions: {
        creativeOptSwitch: true,
      },
      entityList: [{
        entityId: 2001,
        catId: 123,
        price: 998,
        areaTarget: true,
        startTime: expect.any(Number),
      }],
    }))

    const link = await screen.findByRole('link', { name: '查看新建计划' })
    expect(link.getAttribute('href')).toContain('/super-polish/campaigns/75543917')
  })

  it('returns to the campaign list after a low-balance create success and keeps the created paused plan visible', async () => {
    apiMocks.getSuperPolishAccountInfo.mockImplementation(async () => ({
      data: {
        usableBalance: 0,
        totalBalance: 0,
        needReCharge: true,
        rechargeAmountList: [5500],
      },
    }))
    apiMocks.getSuperPolishCampaigns.mockImplementation(async (_uid: string, payload: Record<string, unknown>) => {
      const campaignIds = Array.isArray(payload.campaignIds) ? payload.campaignIds.map((value) => Number(value)) : []
      if (campaignIds.includes(75543917)) {
        return {
          data: {
            result: [{
              campaignId: 75543917,
              itemTitle: '低余额新计划',
              statusDescTitle: '账户余额不足，系统暂停',
            }],
            nextPage: false,
          },
        }
      }

      return {
        data: {
          result: [{ campaignId: 1001, itemTitle: '计划一', status: '投放中' }],
          nextPage: false,
        },
      }
    })

    renderSuperPolish('/super-polish/campaigns/new?uid=u1')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    fireEvent.change(screen.getByLabelText('计划名称'), {
      target: { value: '低余额新计划' },
    })
    await waitFor(() => expect((screen.getByRole('button', { name: '立即擦亮' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: '立即擦亮' }))
    await waitFor(() => expect(apiMocks.createSuperPolishCampaign).toHaveBeenCalled())

    expect(await screen.findByText('余额不足，计划已创建但暂未生效。')).toBeTruthy()
    expect(await screen.findByText('低余额新计划')).toBeTruthy()
    expect(screen.getByText('账户余额不足，系统暂停')).toBeTruthy()
    await waitFor(() => expect(apiMocks.getSuperPolishCampaigns).toHaveBeenCalledWith('u1', expect.objectContaining({
      campaignIds: [75543917],
    })))
  })

  it('does not send the create request when confirmation is cancelled', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(false)
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    expect(await screen.findByText('¥29.9')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '立即擦亮' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalled())
    expect(apiMocks.createSuperPolishCampaign).not.toHaveBeenCalled()
    expect(screen.queryByRole('link', { name: '查看新建计划' })).toBeNull()
  })

  it('clears the previous success state after the plan title changes', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    expect(await screen.findByText('¥29.9')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '立即擦亮' }))

    const link = await screen.findByRole('link', { name: '查看新建计划' })
    expect(link.getAttribute('href')).toContain('/super-polish/campaigns/75543917')

    fireEvent.change(screen.getByLabelText('计划名称'), {
      target: { value: '切换后的新计划名称' },
    })

    await waitFor(() => expect(screen.queryByRole('link', { name: '查看新建计划' })).toBeNull())
  })

  it('resets the create draft when switching to another account', async () => {
    accountStoreMock.activeAccounts = [
      { uid: 'u1', nick: '主账号', status: 'active' },
      { uid: 'u2', nick: '副账号', status: 'active' },
    ]
    accountStoreMock.loadAccounts.mockResolvedValue(accountStoreMock.activeAccounts)
    apiMocks.getSuperPolishRecommendedItems.mockImplementation(async (uid: string) => ({
      data: {
        result: [{
          itemId: uid === 'u2' ? 3001 : 2001,
          title: uid === 'u2' ? '副账号推荐商品' : '主账号推荐商品',
          catId: 123,
          price: 998,
          checked: true,
          supportAd: true,
          supportAreaTarget: true,
        }],
      },
    }))

    renderSuperPolish('/super-polish/campaigns/new?uid=u1')

    expect(await screen.findByText('主账号推荐商品')).toBeTruthy()
    fireEvent.change(screen.getByLabelText('计划名称'), {
      target: { value: '主账号自定义标题' },
    })
    fireEvent.click(screen.getByRole('button', { name: '立即擦亮' }))
    expect(await screen.findByRole('link', { name: '查看新建计划' })).toBeTruthy()

    fireEvent.change(screen.getByRole('combobox', { name: '当前账号' }), {
      target: { value: 'u2' },
    })

    expect(await screen.findByText('副账号推荐商品')).toBeTruthy()
    expect(screen.queryByRole('link', { name: '查看新建计划' })).toBeNull()
    expect((screen.getByLabelText('计划名称') as HTMLInputElement).value).not.toBe('主账号自定义标题')
  })

  it('shows a loading notice before precheck completes', async () => {
    const pending = deferred<{ data: { oldMember: boolean; extraFunctions: { showCreativeOptSwitch: boolean } } }>()
    apiMocks.getSuperPolishCampaignPrecheck.mockReturnValueOnce(pending.promise)

    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText('正在校验建计划能力，请稍候继续配置。')).toBeTruthy()
    expect(screen.queryByText('计划能力已校验，可直接从平台推荐或搜索商品开始创建。')).toBeNull()

    pending.resolve({
      data: {
        oldMember: true,
        extraFunctions: { showCreativeOptSwitch: true },
      },
    })

    expect(await screen.findByText('计划能力已校验，可直接从平台推荐或搜索商品开始创建。')).toBeTruthy()
  })

  it('clears stale picker rows when a refresh request fails', async () => {
    apiMocks.searchSuperPolishItems
      .mockResolvedValueOnce({
        data: {
          result: {
            tabList: [
              { tab: 'platReco', tabName: '平台推荐', count: 1, tabTag: 'new' },
            ],
            itemList: [{
              itemId: 2002,
              title: '搜索结果商品',
              catId: 123,
              price: 1098,
              supportAd: true,
              supportAreaTarget: true,
            }],
          },
        },
      })
      .mockRejectedValueOnce(new Error('搜索失败'))

    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '新增宝贝' }))
    expect(await screen.findByText('搜索结果商品')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '搜索' }))

    expect(await screen.findByText('搜索失败')).toBeTruthy()
    expect(screen.queryByText('搜索结果商品')).toBeNull()
  })

  it('does not auto-search on each keyword change and uses needTabs=false for manual search', async () => {
    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '新增宝贝' }))

    await waitFor(() => expect(apiMocks.searchSuperPolishItems).toHaveBeenCalledTimes(1))
    fireEvent.change(screen.getByPlaceholderText('搜索宝贝标题'), {
      target: { value: '鱼小铺擦亮' },
    })
    await waitFor(() => expect(apiMocks.searchSuperPolishItems).toHaveBeenCalledTimes(1))

    fireEvent.click(screen.getByRole('button', { name: '搜索' }))

    await waitFor(() => expect(apiMocks.searchSuperPolishItems).toHaveBeenCalledTimes(2))
    expect(apiMocks.searchSuperPolishItems).toHaveBeenLastCalledWith('u1', {
      pageNum: 1,
      pageSize: 10,
      tab: 'platReco',
      needTabs: false,
      searchKey: '鱼小铺擦亮',
    })
  })

  it('keeps the first loaded tab set and switches tabs with needTabs=false afterwards', async () => {
    apiMocks.searchSuperPolishItems
      .mockReset()
      .mockResolvedValueOnce({
        data: {
          result: {
            tabList: [
              { tab: 'all', tabName: '全部', count: 2 },
              { tab: 'platReco', tabName: '平台推荐', count: 2, tabTag: 'new' },
              { tab: 'hot', tabName: '爆品', count: 1 },
            ],
            itemList: [{
              itemId: 2002,
              title: '搜索结果商品',
              catId: 123,
              price: 1098,
              supportAd: true,
              supportAreaTarget: true,
            }],
          },
        },
      })
      .mockResolvedValueOnce({
        data: {
          result: {
            itemList: [{
              itemId: 2004,
              title: '全部页商品',
              catId: 123,
              price: 888,
              supportAd: true,
              supportAreaTarget: false,
            }],
          },
        },
      })

    renderSuperPolish('/super-polish/campaigns/new')

    expect(await screen.findByText(/选择宝贝\(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '新增宝贝' }))

    expect(await screen.findByRole('button', { name: /爆品/ })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /全部\s*\(2\)/ }))

    await waitFor(() => expect(apiMocks.searchSuperPolishItems).toHaveBeenCalledTimes(2))
    expect(apiMocks.searchSuperPolishItems).toHaveBeenLastCalledWith('u1', {
      pageNum: 1,
      pageSize: 10,
      tab: 'all',
      needTabs: false,
      searchKey: '',
    })
    expect(await screen.findByText('全部页商品')).toBeTruthy()
    expect(screen.getByRole('button', { name: /爆品/ })).toBeTruthy()
  })

  it('loads campaign detail from campaign search and submits structured item edits without exposing raw maintenance fields', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValueOnce({
      data: {
        result: [makeDetailCampaign()],
        nextPage: false,
      },
    })
    apiMocks.getSuperPolishCampaignGroups.mockResolvedValueOnce({
      data: {
        result: makeDetailGroups(),
      },
    })
    apiMocks.searchSuperPolishItems.mockResolvedValueOnce({
      data: {
        result: {
          tabList: [
            { tab: 'all', tabName: '全部', count: 2 },
            { tab: 'platReco', tabName: '平台推荐', count: 2, tabTag: 'new' },
          ],
          itemList: [
            {
              itemId: 3003,
              title: '新增候选宝贝',
              catId: 789,
              price: 30900,
              supportAd: true,
              supportAreaTarget: true,
              broseCnt: 6,
              exposureCnt: 12,
              saleCnt: 2,
            },
            {
              itemId: 4004,
              title: '冲突候选宝贝',
              catId: 790,
              price: 40900,
              supportAd: true,
              supportAreaTarget: false,
              notSupportAdInfo: [
                { notSupportReason: '已存在擦亮计划', showButton: false },
              ],
            },
          ],
        },
      },
    })

    renderSuperPolish('/super-polish/campaigns/75543917')

    expect(await screen.findByText(/当前宝贝 \(2\/10\)/)).toBeTruthy()
    expect(screen.getByText('当前宝贝A')).toBeTruthy()
    expect(screen.getByText('当前宝贝B')).toBeTruthy()
    expect(screen.getByText('消耗：¥2.6')).toBeTruthy()
    expect(screen.getByText('消耗：¥1.4')).toBeTruthy()
    expect(screen.queryByText('高级维护字段')).toBeNull()
    await waitFor(() => expect(apiMocks.getSuperPolishCampaigns).toHaveBeenCalledWith('u1', expect.objectContaining({
      campaignIds: [75543917],
      pageNum: 1,
      pageSize: 10,
    })))
    await waitFor(() => expect(apiMocks.getSuperPolishCampaignGroups).toHaveBeenCalledWith('u1', '75543917', expect.objectContaining({
      campaignId: 75543917,
      includeEndDate: true,
    })))

    fireEvent.click(screen.getAllByRole('button', { name: '删除' })[0])
    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.queryByText('当前宝贝A')).toBeNull())

    fireEvent.click(screen.getByRole('button', { name: '编辑宝贝' }))

    expect(await screen.findByRole('button', { name: '移除' })).toBeTruthy()
    await waitFor(() => expect(apiMocks.getSuperPolishCampaignPrecheck).toHaveBeenCalledWith('u1'))
    await waitFor(() => expect(apiMocks.searchSuperPolishItems).toHaveBeenCalledWith('u1', {
      pageNum: 1,
      pageSize: 10,
      tab: 'platReco',
      needTabs: true,
      searchKey: '',
    }))
    expect(await screen.findByText('新增候选宝贝')).toBeTruthy()
    expect(screen.getAllByText('已存在擦亮计划').length).toBeGreaterThan(0)

    fireEvent.click(screen.getByRole('button', { name: '移除' }))
    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalledTimes(2))
    fireEvent.click(screen.getByRole('button', { name: '添加' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    expect(await screen.findByText(/当前宝贝 \(1\/10\)/)).toBeTruthy()
    expect(screen.getByText('新增候选宝贝')).toBeTruthy()
    expect(screen.queryByText('当前宝贝B')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: '保存计划修改' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalledTimes(3))
    await waitFor(() => expect(apiMocks.editSuperPolishCampaign).toHaveBeenCalledWith('u1', '75543917', expect.objectContaining({
      itemCampaigns: [
        expect.objectContaining({
          campaignId: 75543917,
          entityIds: [3003],
          targetInfo: {
            divisionList: [
              { province: '山东', area: '济南', divisionId: 370100 },
            ],
          },
          entityList: [
            expect.objectContaining({
              itemId: 3003,
              title: '新增候选宝贝',
              catId: 789,
            }),
          ],
        }),
      ],
    })))

    expect(await screen.findByText('已提交')).toBeTruthy()
  })

  it('edits campaign detail settings and submits the structured payload with updated title budget targeting and creative options', async () => {
    const editableCampaign = makeDetailCampaign()
    editableCampaign.entityList = [editableCampaign.entityList[0]]
    const editableGroups = makeDetailGroups().slice(0, 1).map((group) => ({
      ...group,
      supportAreaTarget: true,
    }))

    apiMocks.getSuperPolishCampaigns.mockResolvedValueOnce({
      data: {
        result: [editableCampaign],
        nextPage: false,
      },
    })
    apiMocks.getSuperPolishCampaignGroups.mockResolvedValueOnce({
      data: {
        result: editableGroups,
      },
    })

    renderSuperPolish('/super-polish/campaigns/75543917')

    expect(await screen.findByText(/当前宝贝 \(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '编辑设置' }))

    expect(await screen.findByText('编辑计划设置')).toBeTruthy()
    fireEvent.change(screen.getByLabelText('计划名称'), {
      target: { value: '新版擦亮计划' },
    })
    fireEvent.change(screen.getByLabelText('自定义出价'), {
      target: { value: '6' },
    })
    fireEvent.change(screen.getByLabelText('日预算'), {
      target: { value: '2222' },
    })

    fireEvent.click(screen.getByRole('button', { name: '全部时段' }))
    fireEvent.click(await screen.findByRole('button', { name: '20时' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    fireEvent.click(screen.getByRole('button', { name: '山东 济南' }))
    fireEvent.click(await screen.findByRole('button', { name: '山东' }))
    await waitFor(() => expect(marketMocks.locationDivisionChildren).toHaveBeenCalledWith('u1', 370000))
    fireEvent.click(await screen.findByLabelText('青岛市'))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    fireEvent.click(screen.getByLabelText('创意优化'))
    fireEvent.click(screen.getByRole('button', { name: '保存设置' }))

    expect(await screen.findByText('新版擦亮计划')).toBeTruthy()
    expect(screen.getByText('山东等2城')).toBeTruthy()
    expect(screen.getByText('23个时段')).toBeTruthy()
    expect(screen.getByText('未开启')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '保存计划修改' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(apiMocks.editSuperPolishCampaign).toHaveBeenCalledWith('u1', '75543917', expect.objectContaining({
      itemCampaigns: [
        expect.objectContaining({
          title: '新版擦亮计划',
          entityIds: [2001],
          bid: 600,
          cpm: 600,
          maxDayBudget: 222200,
          budget: 222200,
          creativeOptSwitch: false,
          targetInfo: {
            divisionList: [
              { province: '山东', area: '济南', divisionId: 370100 },
              { province: '山东', area: '青岛', divisionId: 370200 },
            ],
          },
          budgetScheduleDurations: [
            '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11',
            '12', '13', '14', '15', '16', '17', '18', '19', '21', '22', '23',
          ],
          infoTab: expect.objectContaining({
            bidPrice: 600,
            cpm: 600,
            maxDayBudget: 222200,
            budgetScheduleDurations: [
              '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11',
              '12', '13', '14', '15', '16', '17', '18', '19', '21', '22', '23',
            ],
          }),
        }),
      ],
    })))

    expect(await screen.findByText('已提交')).toBeTruthy()
  })

  it('keeps city targeting editable in campaign detail when only part of the current items support it and only marks supported items as targeted', async () => {
    apiMocks.getSuperPolishCampaigns.mockResolvedValueOnce({
      data: {
        result: [makeDetailCampaign()],
        nextPage: false,
      },
    })
    apiMocks.getSuperPolishCampaignGroups.mockResolvedValueOnce({
      data: {
        result: makeDetailGroups(),
      },
    })

    renderSuperPolish('/super-polish/campaigns/75543917')

    expect(await screen.findByText(/当前宝贝 \(2\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '编辑设置' }))

    expect(await screen.findByText('编辑计划设置')).toBeTruthy()
    expect(screen.getByText('支持城市定向 1/2')).toBeTruthy()
    expect(screen.getByText('支持定向的宝贝：')).toBeTruthy()
    expect((screen.getByRole('button', { name: '山东 济南' }) as HTMLButtonElement).disabled).toBe(false)

    fireEvent.click(screen.getByRole('button', { name: '山东 济南' }))
    expect((await screen.findAllByText('其余 1 个宝贝仍按全部地域投放。')).length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '山东' }))
    await waitFor(() => expect(marketMocks.locationDivisionChildren).toHaveBeenCalledWith('u1', 370000))
    fireEvent.click(await screen.findByLabelText('青岛市'))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))
    fireEvent.click(screen.getByRole('button', { name: '保存设置' }))
    fireEvent.click(screen.getByRole('button', { name: '保存计划修改' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(apiMocks.editSuperPolishCampaign).toHaveBeenCalledWith('u1', '75543917', expect.objectContaining({
      itemCampaigns: [
        expect.objectContaining({
          entityList: [
            expect.objectContaining({
              itemId: 2001,
              areaTarget: true,
            }),
            expect.objectContaining({
              itemId: 2002,
              areaTarget: false,
            }),
          ],
        }),
      ],
    })))
  })

  it('edits campaign detail strategy and submits package mode fields with the selected target and package option', async () => {
    const editableCampaign = makeDetailCampaign()
    editableCampaign.entityList = [editableCampaign.entityList[0]]

    apiMocks.getSuperPolishCampaigns.mockResolvedValueOnce({
      data: {
        result: [editableCampaign],
        nextPage: false,
      },
    })
    apiMocks.getSuperPolishCampaignGroups.mockResolvedValueOnce({
      data: {
        result: makeDetailGroups().slice(0, 1),
      },
    })
    apiMocks.querySuperPolishPackages.mockResolvedValueOnce({
      data: {
        bidTargets: ['click', 'chat'],
        packageTargetVOList: [
          {
            bidTarget: 'click',
            recommendPackageType: 'click-recommend',
            orderVOList: [
              {
                orderPrice: 4990,
                bidPrice: 510,
                bidType: 'smart_bid',
                algoRecommend: 1,
                orderStartDuration: 3,
                orderEndDuration: 7,
                orderImprove: [8, 12],
                title: '点击推荐套餐',
                promotionText: '推荐点击拉量',
                packageType: 'click_recommend',
                extraFunctions: { showCreativeOptSwitch: true },
              },
              {
                orderPrice: 6990,
                bidPrice: 610,
                bidType: 'smart_bid',
                algoRecommend: 0,
                orderStartDuration: 5,
                orderEndDuration: 10,
                orderImprove: [16, 24],
                title: '点击加强套餐',
                promotionText: '加速点击提升',
                packageType: 'click_plus',
                extraFunctions: { showCreativeOptSwitch: true },
              },
            ],
          },
          {
            bidTarget: 'chat',
            recommendPackageType: 'chat-recommend',
            orderVOList: [
              {
                orderPrice: 2990,
                bidPrice: 310,
                bidType: 'smart_bid',
                algoRecommend: 1,
                orderStartDuration: 3,
                orderEndDuration: 7,
                orderImprove: [8, 12],
                title: '咨询推荐套餐',
                promotionText: '咨询稳定提升',
                packageType: 'chat_recommend',
                extraFunctions: { showCreativeOptSwitch: true },
              },
            ],
          },
        ],
      },
    })
    apiMocks.estimateSuperPolishBids.mockResolvedValueOnce({
      data: {
        bidTargets: ['click', 'chat'],
        recBidTarget: 'chat',
        customizeBidRecommendVOList: [
          {
            bidTarget: 'click',
            bidPrice: 480,
            customBidPrice: 560,
            bidType: 'smart_bid',
            targetRecommendBudget: 8800,
            recommendBudgetList: [8800, 12000],
            extraFunctions: { showCreativeOptSwitch: true },
          },
          {
            bidTarget: 'chat',
            bidPrice: 390,
            customBidPrice: 460,
            bidType: 'smart_bid',
            targetRecommendBudget: 5000,
            recommendBudgetList: [5000, 15000],
            extraFunctions: { showCreativeOptSwitch: true },
          },
        ],
      },
    })

    renderSuperPolish('/super-polish/campaigns/75543917')

    expect(await screen.findByText(/当前宝贝 \(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '编辑设置' }))

    expect(await screen.findByText('编辑计划设置')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /点击|浏览量/ }))
    fireEvent.click(screen.getByRole('button', { name: '套餐擦亮' }))

    await waitFor(() => expect(apiMocks.querySuperPolishPackages).toHaveBeenCalledWith('u1', {
      entityType: 1,
      isGoodsSelectedTabChange: true,
      entityList: [{
        entityId: 2001,
        catId: 123,
        price: 9900,
        areaTarget: true,
        startTime: expect.any(Number),
      }],
    }))

    expect(await screen.findByText('¥49.9')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '编辑套餐' }))
    expect(await screen.findByText('选择套餐')).toBeTruthy()
    fireEvent.click(screen.getByText('¥69.9'))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))
    fireEvent.click(screen.getByRole('button', { name: '保存设置' }))

    expect(await screen.findByText('套餐擦亮')).toBeTruthy()
    expect(screen.getAllByText(/点击|浏览量/).length).toBeGreaterThan(0)
    expect(screen.getByText('¥69.9')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '保存计划修改' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(apiMocks.editSuperPolishCampaign).toHaveBeenCalledWith('u1', '75543917', expect.objectContaining({
      itemCampaigns: [
        expect.objectContaining({
          bidTarget: 'click',
          promotionMode: 'order',
          bidType: 'smart_bid',
          orderPrice: 6990,
          orderStartDuration: 5,
          orderEndDuration: 10,
          packageType: 'click_plus',
          bid: 610,
          cpm: 610,
        }),
      ],
    })))

    expect(await screen.findByText('已提交')).toBeTruthy()
  })

  it('edits campaign detail custom strategy with recommended bidding and keeps account status in sync', async () => {
    const editableCampaign = makeDetailCampaign()
    editableCampaign.entityList = [editableCampaign.entityList[0]]

    apiMocks.getSuperPolishCampaigns.mockResolvedValueOnce({
      data: {
        result: [editableCampaign],
        nextPage: false,
      },
    })
    apiMocks.getSuperPolishCampaignGroups.mockResolvedValueOnce({
      data: {
        result: makeDetailGroups().slice(0, 1),
      },
    })

    renderSuperPolish('/super-polish/campaigns/75543917')

    expect(await screen.findByText(/当前宝贝 \(1\/10\)/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '编辑设置' }))

    expect(await screen.findByText('编辑计划设置')).toBeTruthy()
    expect(await screen.findByText(/可用余额：/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '系统推荐' }))
    fireEvent.click(screen.getByRole('button', { name: '¥50' }))

    await waitFor(() => expect(apiMocks.getSuperPolishAccountInfo).toHaveBeenCalledWith('u1', {
      orderPrice: 5000,
      promotionMode: 'custom',
      bidType: 'smart_bid',
      scene: 'creationRealTime',
    }))

    fireEvent.click(screen.getByRole('button', { name: '保存设置' }))
    fireEvent.click(screen.getByRole('button', { name: '保存计划修改' }))

    await waitFor(() => expect(dialogMocks.showConfirm).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(apiMocks.editSuperPolishCampaign).toHaveBeenCalledWith('u1', '75543917', expect.objectContaining({
      itemCampaigns: [
        expect.objectContaining({
          promotionMode: 'custom',
          bidTarget: 'chat',
          bidType: 'smart_bid',
          bid: 390,
          cpm: 390,
          maxDayBudget: 5000,
          budget: 5000,
          infoTab: expect.objectContaining({
            bidPrice: 390,
            cpm: 390,
            maxDayBudget: 5000,
          }),
        }),
      ],
    })))

    expect(await screen.findByText('已提交')).toBeTruthy()
  })
})
