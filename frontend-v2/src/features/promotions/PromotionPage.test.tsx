import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import PromotionPage from './PromotionPage'

const { apiMocks, accountStoreMock, clientMocks, marketMocks, notifyMock } = vi.hoisted(() => ({
  apiMocks: {
    getCoinSigninHome: vi.fn(),
    getCoinChallengeInfo: vi.fn(),
    getCoinActivityUserInfo: vi.fn(),
    getCoinUserDetailList: vi.fn(),
    getCoinVirtualCardList: vi.fn(),
    signInCoin: vi.fn(),
    getCoinTasks: vi.fn(),
    getCoinSubscribeStatus: vi.fn(),
    completeCoinViewTasks: vi.fn(),
    claimCoinTask: vi.fn(),
    getPromotionPage: vi.fn(),
    getPromotionView: vi.fn(),
    getPromotionDetail: vi.fn(),
    createPromotion: vi.fn(),
    getExposureHome: vi.fn(),
    getExposureAmountOptions: vi.fn(),
    getExposureItemRendering: vi.fn(),
    createExposureOrder: vi.fn(),
    getExposureTasks: vi.fn(),
    completeSafeViewTasks: vi.fn(),
    runExposureTask: vi.fn(),
    receiveExposureReward: vi.fn(),
    getShopWorkbench: vi.fn(),
    getSuperPolishAccount: vi.fn(),
    getExposureHistory: vi.fn(),
    getExposureDetails: vi.fn(),
    triggerSuperExposure: vi.fn(),
    mtopRetText: vi.fn((payload: unknown) => String((payload as { ret?: string[] })?.ret?.[0] ?? '')),
    mtopSucceeded: vi.fn((payload: unknown) => String((payload as { ret?: string[] })?.ret?.[0] ?? '').includes('SUCCESS')),
    unwrapMtopData: vi.fn((payload: unknown) => (payload as { data?: unknown })?.data ?? payload),
  },
  accountStoreMock: {
    activeAccounts: [{ uid: 'u1', nick: '主账号', status: 'active' }],
    loading: false,
    loadAccounts: vi.fn(),
  },
  clientMocks: {
    getAuthToken: vi.fn(() => 'token-a'),
  },
  marketMocks: {
    getItems: vi.fn(),
  },
  notifyMock: vi.fn(),
}))

const dialogMocks = vi.hoisted(() => ({
  showConfirm: vi.fn(),
}))

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false }),
}))

vi.mock('../../components/Notify', () => ({
  useSafeNotify: () => notifyMock,
}))

vi.mock('../../components/AppDialog', () => ({
  useAppDialog: () => dialogMocks,
}))

vi.mock('../../api/shopGrowth', () => apiMocks)
vi.mock('../../api/client', () => clientMocks)
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

function localDateKey(date = new Date()) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function offsetDateKey(offsetDays: number) {
  const date = new Date()
  date.setDate(date.getDate() + offsetDays)
  return localDateKey(date)
}

describe('PromotionPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clientMocks.getAuthToken.mockReturnValue('token-a')
    accountStoreMock.activeAccounts = [{ uid: 'u1', nick: '主账号', status: 'active' }]
    accountStoreMock.loading = false
    accountStoreMock.loadAccounts.mockResolvedValue(accountStoreMock.activeAccounts)
    apiMocks.getCoinSigninHome.mockResolvedValue({ data: { coinNum: 8 } })
    apiMocks.getCoinChallengeInfo.mockResolvedValue({ data: { failedFlag: false } })
    apiMocks.getCoinActivityUserInfo.mockResolvedValue({ data: { myIdleCoin: 88, todaySignIn: false, alreadySignInDays: 6 } })
    apiMocks.getCoinUserDetailList.mockResolvedValue({ data: { userScoreDetailDOList: [] } })
    apiMocks.getCoinVirtualCardList.mockResolvedValue({ data: { data: [] } })
    apiMocks.getCoinTasks.mockResolvedValue({ data: { taskList: [{ taskId: 'task-1', taskName: '浏览商品' }] } })
    apiMocks.getCoinSubscribeStatus.mockResolvedValue({ data: { subscribe: true } })
    apiMocks.getPromotionPage.mockResolvedValue({ data: { promotionList: [] } })
    apiMocks.getExposureTasks.mockResolvedValue({ data: { taskList: [] } })
    apiMocks.getShopWorkbench.mockResolvedValue({ data: { availableExposureFlow: 20 } })
    apiMocks.getExposureHistory.mockResolvedValue({ data: [] })
    apiMocks.getExposureDetails.mockResolvedValue({ data: [] })
    apiMocks.signInCoin.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    apiMocks.completeCoinViewTasks.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    apiMocks.completeSafeViewTasks.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { success: true } })
    apiMocks.claimCoinTask.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    apiMocks.getPromotionView.mockResolvedValue({ data: { promotionPriceMap: {} } })
    apiMocks.createPromotion.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { activityId: 'activity-1' } })
    apiMocks.getExposureHome.mockResolvedValue({
      data: {
        exposureCoinNum: 120,
        exposeableItemDOList: [],
        exposureOrderList: [],
      },
    })
    apiMocks.getExposureAmountOptions.mockResolvedValue({
      data: {
        hasQuantity: true,
        userAccount: 120,
        exposureAmountDOList: [
          { exposureNum: 50, title: '推荐补量' },
          { exposureNum: 100, title: '集中投放' },
        ],
      },
    })
    apiMocks.getExposureItemRendering.mockResolvedValue({
      data: {
        itemActivityInstanceVOList: [
          { itemId: 'item-exposure-1', isPolishing: false },
        ],
      },
    })
    apiMocks.createExposureOrder.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { orderId: 'exposure-order-1' } })
    apiMocks.runExposureTask.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    apiMocks.triggerSuperExposure.mockResolvedValue({ ret: ['SUCCESS::调用成功'] })
    marketMocks.getItems.mockResolvedValue({ items: [], pagination: { page: 1, page_size: 20, total: 0 } })
    dialogMocks.showConfirm.mockResolvedValue(false)
  })

  it('runs coin signin directly without a confirmation dialog', async () => {
    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    fireEvent.click(await screen.findByRole('button', { name: '签到领币' }))

    await waitFor(() => expect(apiMocks.signInCoin).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(notifyMock).toHaveBeenCalledWith('签到成功，闲鱼币状态已更新', 'success'))
    expect(notifyMock).not.toHaveBeenCalledWith('操作成功', 'success')
    expect(dialogMocks.showConfirm).not.toHaveBeenCalled()
  })

  it('does not fan out secondary growth modules on initial page load', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-independent-1',
        uid: 'u1',
        title: '独立加载商品',
        price: '12.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getCoinSigninHome).toHaveBeenCalledWith('u1', expect.anything()))
    await waitFor(() => expect(marketMocks.getItems).toHaveBeenCalledWith('u1', 1, 20, 'ON_SALE', { force: false }))

    expect(apiMocks.getCoinUserDetailList).not.toHaveBeenCalled()
    expect(apiMocks.getExposureHistory).not.toHaveBeenCalled()
    expect(apiMocks.getShopWorkbench).not.toHaveBeenCalled()
  })

  it('does not render numeric step badges in promotion sections', async () => {
    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByLabelText('今日任务台')).toBeTruthy()
    const taskTitleRow = screen.getByRole('heading', { name: '今日任务台' }).parentElement
    const coinTitleRow = (await screen.findByRole('heading', { name: '闲鱼币推广' })).parentElement
    const exposureTitleRow = (await screen.findByRole('heading', { name: '鱼小铺曝光推广' })).parentElement
    expect(taskTitleRow ? within(taskTitleRow).queryByText('1') : null).toBeNull()
    expect(coinTitleRow ? within(coinTitleRow).queryByText('2') : null).toBeNull()
    expect(exposureTitleRow ? within(exposureTitleRow).queryByText('2B') : null).toBeNull()
  })

  it('keeps today task columns short enough for three visible task rows', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: Array.from({ length: 5 }).map((_, index) => ({
          deliveryId: `coin-height-${index}`,
          idleTaskStatus: 'ACCEPTED',
          materialDTO: {
            title: `高度测试闲鱼币任务 ${index + 1}`,
            actionType: 'VISIT',
            desc: '80',
            action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&adCode=mmOrder_view01',
          },
          progressDTO: { times: 0, needTimes: 1 },
        })),
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const coinTaskColumn = await screen.findByRole('group', { name: '闲鱼币任务区' })
    expect(coinTaskColumn.className).toContain('h-[300px]')
    expect(coinTaskColumn.className).not.toContain('h-[360px]')
    expect(within(coinTaskColumn).getByText('高度测试闲鱼币任务 1')).toBeTruthy()
  })

  it('keeps completed exposure tasks visible as completed or claimed states', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        incentiveTask: {
          learningTaskList: [
            {
              taskInstanceId: 'exposure-finished-1',
              taskStatus: 'FINISHED',
              extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
              materialVO: { title: '已完成小铺数据任务' },
              benefitVO: { activityId: 100769, rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }] },
              progressVO: { times: 1, needTimes: 1 },
            },
            {
              taskInstanceId: 'exposure-claimed-1',
              taskStatus: 'RECEIVED',
              extend: { taskType: 'shop_visit_diagnosis', flowNum: '10' },
              materialVO: { title: '已领取小铺诊断任务' },
              benefitVO: { activityId: 100769, rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }] },
              progressVO: { times: 1, needTimes: 1 },
            },
          ],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('已完成小铺数据任务')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已完成 已完成小铺数据任务' }).getAttribute('disabled')).not.toBeNull()
    expect(await screen.findByText('已领取小铺诊断任务')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已领取 已领取小铺诊断任务' }).getAttribute('disabled')).not.toBeNull()
  })

  it('treats completed exposure tasks as claimed when the upstream unclaimed benefit list is empty', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        currentFlow: 1400,
        serverTime: '2026-06-30 18:55:31',
        unclaimedBenefits: {
          benefitList: [],
          claimEffectType: 0,
          hasPlanReward: false,
        },
        incentiveTask: {
          learningTaskList: [{
            taskInstanceId: 1007693770,
            taskStatus: 'COMPLETE',
            benefitVO: {
              activityId: 100769,
              taskInstanceId: 1007693770,
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }],
            },
            extend: {
              taskType: 'shop_visit_my_data_new',
              flowNum: '10',
            },
            materialVO: {
              title: '每天查看「小铺数据」',
              desc: '点击“查看完整数据”，浏览15秒以上',
              awardBtn: '领取奖励',
              completeBtn: '已完成',
            },
            progressVO: { times: 1, needTimes: 0, maxTimes: 1, reachLimit: true },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('每天查看「小铺数据」')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已领取 每天查看「小铺数据」' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.queryByRole('button', { name: '领奖 每天查看「小铺数据」' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: '已领取 每天查看「小铺数据」' }))
    expect(apiMocks.receiveExposureReward).not.toHaveBeenCalled()
  })

  it('falls back to claimed exposure task rewards when platform exposure history is empty', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        serverTime: '2026-06-30 18:55:31',
        unclaimedBenefits: { benefitList: [] },
        incentiveTask: {
          learningTaskList: [{
            taskInstanceId: 1007694526,
            taskStatus: 'COMPLETE',
            benefitVO: {
              activityId: 100769,
              taskInstanceId: 1007694526,
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }],
            },
            extend: { taskType: 'shop_visit_xy_coin', flowNum: '10' },
            materialVO: { title: '访问闲鱼币频道', awardBtn: '领取奖励' },
            progressVO: { times: 1, needTimes: 0 },
          }],
        },
      },
    })
    apiMocks.getExposureHistory.mockResolvedValue({
      data: {
        exposureHistoryList: {
          result: [],
          success: true,
          total: 0,
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('访问闲鱼币频道')).toBeTruthy()
    const exposureColumn = screen.getByRole('group', { name: '鱼小铺曝光任务区' })
    fireEvent.click(within(exposureColumn).getByRole('button', { name: '明细' }))

    await waitFor(() => expect(apiMocks.getExposureHistory).toHaveBeenCalledTimes(1))
    expect(within(exposureColumn).getByText('访问闲鱼币频道')).toBeTruthy()
    expect(within(exposureColumn).getByText(/10 曝光/)).toBeTruthy()
    expect(within(exposureColumn).queryByText('暂无曝光明细')).toBeNull()
  })

  it('centers promotion table headers and names fish-shop exposure as a promotion area', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-header-1',
        uid: 'u1',
        title: '表头测试商品',
        price: '66.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const coinBuilder = await screen.findByLabelText('闲鱼币推广')
    const exposureBuilder = await screen.findByLabelText('鱼小铺曝光推广')
    expect(within(exposureBuilder).queryByRole('heading', { name: '新建鱼小铺曝光' })).toBeNull()

    const coinHeader = within(coinBuilder).getByText('套餐').parentElement
    const exposureHeader = within(exposureBuilder).getByText('曝光量').parentElement
    expect(coinHeader?.className).toContain('place-items-center')
    expect(exposureHeader?.className).toContain('place-items-center')
    expect(within(coinBuilder).getByText('套餐').className).toContain('text-center')
    expect(within(exposureBuilder).getByText('曝光量').className).toContain('text-center')
  })

  it('runs task one-click actions directly without confirmation dialogs', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [{
          deliveryId: 79493,
          sceneId: 496,
          idleTaskStatus: 'ACCEPTED',
          materialDTO: {
            title: '直接领币任务',
            actionType: 'VISIT',
            desc: '80',
            subTitle: '闲鱼币+80',
            action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&orderCode=mmOrderView02&adCode=mmOrder_view01&isView=true&source=xybrworder',
          },
          progressDTO: { times: 0, needTimes: 1 },
        }],
      },
    })
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        incentiveTask: {
          learningTaskList: [{
            taskStatus: 'ACCEPTED',
            extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
            materialVO: { title: '直接领取曝光任务' },
            benefitVO: { activityId: 100769, rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }] },
            progressVO: { times: 0, needTimes: 1 },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    fireEvent.click(await screen.findByRole('button', { name: '一键领币' }))
    await waitFor(() => expect(apiMocks.completeCoinViewTasks).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(notifyMock).toHaveBeenCalledWith('闲鱼币任务已处理，奖励状态已刷新', 'success'))

    fireEvent.click(await screen.findByRole('button', { name: '一键领取曝光' }))
    await waitFor(() => expect(apiMocks.completeSafeViewTasks).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(notifyMock).toHaveBeenCalledWith('鱼小铺曝光任务已处理，奖励状态已刷新', 'success'))
    expect(notifyMock).not.toHaveBeenCalledWith('操作成功', 'success')
    expect(dialogMocks.showConfirm).not.toHaveBeenCalled()
  })

  it('keeps coin tasks visible as claimed after one-click claiming even when refreshed task list is empty', async () => {
    apiMocks.getCoinTasks
      .mockResolvedValueOnce({
        data: {
          taskList: [{
            deliveryId: 79493,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '领取后保留闲鱼币任务',
              actionType: 'VISIT',
              desc: '80',
              subTitle: '闲鱼币+80',
              action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&adCode=coinKeep01',
            },
            progressDTO: { times: 0, needTimes: 1 },
          }],
        },
      })
      .mockResolvedValueOnce({ data: { taskList: [] } })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('领取后保留闲鱼币任务')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '一键领币' }))

    await waitFor(() => expect(apiMocks.completeCoinViewTasks).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.getByRole('button', { name: '已领取 领取后保留闲鱼币任务' })).toBeTruthy())
    expect(screen.getByRole('button', { name: '已领取 领取后保留闲鱼币任务' }).getAttribute('disabled')).not.toBeNull()
  })

  it('keeps fish-shop exposure tasks visible as claimed after one-click claiming even when refreshed task list is empty', async () => {
    apiMocks.getExposureTasks
      .mockResolvedValueOnce({
        data: {
          incentiveTask: {
            learningTaskList: [{
              taskStatus: 'ACCEPTED',
              taskId: 'exposure-keep-1',
              taskInstanceId: 'exposure-keep-1',
              benefitId: 'benefit-exposure-keep-1',
              benefitVO: { activityId: 100769, rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }] },
              extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
              materialVO: { title: '领取后保留曝光任务' },
              progressVO: { times: 0, needTimes: 1 },
            }],
          },
        },
      })
      .mockResolvedValueOnce({ data: { incentiveTask: { learningTaskList: [] } } })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('领取后保留曝光任务')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '一键领取曝光' }))

    await waitFor(() => expect(apiMocks.completeSafeViewTasks).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.getByRole('button', { name: '已领取 领取后保留曝光任务' })).toBeTruthy())
    expect(screen.getByRole('button', { name: '已领取 领取后保留曝光任务' }).getAttribute('disabled')).not.toBeNull()
  })

  it('keeps task one-click actions available while signin is running', async () => {
    const signin = deferred<unknown>()
    apiMocks.signInCoin.mockReturnValue(signin.promise)
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [{
          deliveryId: 79493,
          sceneId: 496,
          idleTaskStatus: 'ACCEPTED',
          materialDTO: {
            title: '签到同时可领币',
            actionType: 'VISIT',
            desc: '80',
            subTitle: '闲鱼币+80',
            action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&orderCode=mmOrderView02&adCode=mmOrder_view01&isView=true&source=xybrworder',
          },
          progressDTO: { times: 0, needTimes: 1 },
        }],
      },
    })
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        incentiveTask: {
          learningTaskList: [{
            taskStatus: 'ACCEPTED',
            extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
            materialVO: { title: '签到同时可领曝光' },
            benefitVO: { activityId: 100769, rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }] },
            progressVO: { times: 0, needTimes: 1 },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('签到同时可领币')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '签到领币' }))
    await waitFor(() => expect(apiMocks.signInCoin).toHaveBeenCalledTimes(1))

    const coinOneClickButton = screen.getByRole('button', { name: '一键领币' }) as HTMLButtonElement
    const exposureOneClickButton = screen.getByRole('button', { name: '一键领取曝光' }) as HTMLButtonElement
    expect(coinOneClickButton.disabled).toBe(false)
    expect(exposureOneClickButton.disabled).toBe(false)

    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    fireEvent.click(coinOneClickButton)

    await waitFor(() => expect(apiMocks.completeCoinViewTasks).toHaveBeenCalledTimes(1))
    expect((screen.getByRole('button', { name: '签到领币' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('does not render write result summaries on the promotion page', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    fireEvent.click(await screen.findByRole('button', { name: '签到领币' }))

    await waitFor(() => expect(apiMocks.signInCoin).toHaveBeenCalledTimes(1))
    expect(screen.queryByText('最近一次执行结果')).toBeNull()
    expect(screen.queryByText('闲鱼币签到')).toBeNull()
    expect(screen.queryByText('查看原始返回')).toBeNull()
  })

  it('renders the approved growth workspace sections in order without super-polish controls', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-layout-1',
        uid: 'u1',
        title: '四区布局商品',
        price: '128.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const coinBuilder = await screen.findByLabelText('闲鱼币推广')
    expect(within(coinBuilder).getAllByText('四区布局商品').length).toBeGreaterThan(0)
    const taskBoard = screen.getByLabelText('今日任务台')
    expect(within(taskBoard).queryByText('签到提醒')).toBeNull()
    expect(within(taskBoard).queryByText('已订阅')).toBeNull()
    expect(within(taskBoard).queryByText('未订阅')).toBeNull()
    const sectionTitles = screen.getAllByRole('heading', { level: 2 }).map((heading) => heading.textContent)
    expect(sectionTitles.slice(0, 2)).toEqual(['今日任务台', '闲鱼币推广'])
    expect(screen.queryByLabelText('已购与曝光')).toBeNull()
    expect(await screen.findByLabelText('鱼小铺曝光推广')).toBeTruthy()
    expect(screen.queryByText('创建成功后进入同步队列')).toBeNull()
    expect(screen.queryByText('超级擦亮')).toBeNull()
    expect(screen.queryByRole('link', { name: '前往超级擦亮' })).toBeNull()
    expect(screen.queryByRole('button', { name: '触发超级擦亮' })).toBeNull()
  })

  it('keeps task completion actions in the today board without rendering the removed ledger', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [{
          deliveryId: 79493,
          sceneId: 496,
          idleTaskStatus: 'ACCEPTED',
          materialDTO: {
            title: '今日闲鱼币浏览',
            actionType: 'VISIT',
            desc: '80',
            subTitle: '闲鱼币+80',
            action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&adCode=coinTask01',
          },
          progressDTO: { times: 0, needTimes: 1 },
        }],
      },
    })
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        currentFlow: 90,
        incentiveTask: {
          learningTaskList: [{
            taskStatus: 'ACCEPTED',
            extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
            materialVO: { title: '今日鱼小铺浏览' },
            benefitVO: {
              activityId: 100769,
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }],
            },
            progressVO: { times: 0, needTimes: 1 },
          }],
        },
      },
    })
    apiMocks.getPromotionPage.mockResolvedValue({
      data: {
        items: [{
          id: 827559279,
          itemVO: { itemId: 1055797812835, title: '已购推广商品' },
          orderTime: '2026-05-28 08:03:28',
          promotionStatus: 4,
          aimedPromoteNum: 50,
          realPromoteNum: 13,
        }],
      },
    })
    apiMocks.getExposureHistory.mockResolvedValue({
      data: {
        exposureHistoryList: {
          result: [{ title: '普通曝光账本', count: 500, gmtCreate: '2025.04.10 10:01', instanceId: 6812709, bizCode: 'shop_task_detail_flow_card_v2' }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect(within(taskBoard).getByText('今日闲鱼币浏览')).toBeTruthy()
    expect(within(taskBoard).getByText('今日鱼小铺浏览')).toBeTruthy()
    expect(within(taskBoard).getByRole('button', { name: '完成任务 今日闲鱼币浏览' })).toBeTruthy()
    expect(within(taskBoard).getByRole('button', { name: '完成任务 今日鱼小铺浏览' })).toBeTruthy()

    expect(screen.queryByLabelText('已购与曝光')).toBeNull()
    expect(screen.queryByText('已购推广商品')).toBeNull()
    expect(screen.queryByText('普通曝光账本')).toBeNull()
  })

  it('renders fish-shop exposure creation in the promotion workspace without the removed ledger', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-exposure-1',
        uid: 'u1',
        title: '曝光候选商品',
        price: '66.00',
        status: 'ON_SALE',
        pic_url: 'https://img.example.test/item-exposure-1.jpg',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByLabelText('闲鱼币推广')).toBeTruthy()
    const exposureBuilder = await screen.findByLabelText('鱼小铺曝光推广')
    expect(within(exposureBuilder).getAllByText('曝光候选商品').length).toBeGreaterThan(0)
    await waitFor(() => {
      const action = within(exposureBuilder).getByRole('button', { name: '开启曝光 曝光候选商品' }) as HTMLButtonElement
      expect(action.disabled).toBe(false)
    })
    expect(screen.queryByLabelText('已购与曝光')).toBeNull()
    expect(apiMocks.getExposureItemRendering).not.toHaveBeenCalled()
    expect(apiMocks.createExposureOrder).not.toHaveBeenCalled()
    expect(apiMocks.triggerSuperExposure).not.toHaveBeenCalled()
  })

  it('creates fish-shop exposure from the listed item and selected amount directly', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-exposure-1',
        uid: 'u1',
        title: '曝光候选商品',
        price: '66.00',
        status: 'ON_SALE',
        pic_url: 'https://img.example.test/item-exposure-1.jpg',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const exposureBuilder = await screen.findByLabelText('鱼小铺曝光推广')
    const amountSelect = await within(exposureBuilder).findByRole('combobox', { name: '选择曝光量 曝光候选商品' })
    fireEvent.change(amountSelect, { target: { value: '100' } })
    fireEvent.click(within(exposureBuilder).getByRole('button', { name: '开启曝光 曝光候选商品' }))

    await waitFor(() => expect(apiMocks.getExposureItemRendering).toHaveBeenCalledWith(
      'u1',
      ['item-exposure-1'],
      expect.anything(),
    ))
    await waitFor(() => expect(apiMocks.createExposureOrder).toHaveBeenCalledWith({
      uid: 'u1',
      itemIds: ['item-exposure-1'],
      exposureNum: 100,
      expectedExposureNum: 100,
      picUrl: 'https://img.example.test/item-exposure-1.jpg',
      title: '曝光候选商品',
    }))
    await waitFor(() => expect(notifyMock).toHaveBeenCalledWith('商品「曝光候选商品」曝光推广已开启', 'success'))
    expect(notifyMock).not.toHaveBeenCalledWith('操作成功', 'success')
    expect(dialogMocks.showConfirm).not.toHaveBeenCalled()
  })

  it('selects a fish-shop exposure amount from the row dropdown', async () => {
    apiMocks.getExposureAmountOptions.mockResolvedValue({
      data: {
        hasQuantity: true,
        userAccount: 300,
        exposureAmountDOList: [
          { exposureNum: 50, title: '轻量曝光' },
          { exposureNum: 100, title: '集中投放' },
        ],
      },
    })
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-exposure-picker',
        uid: 'u1',
        title: '曝光下拉商品',
        price: '66.00',
        status: 'ON_SALE',
        pic_url: 'https://img.example.test/item-exposure-picker.jpg',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getExposureItemRendering.mockResolvedValue({
      data: {
        itemActivityInstanceVOList: [
          { itemId: 'item-exposure-picker', isPolishing: false },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const exposureBuilder = await screen.findByLabelText('鱼小铺曝光推广')
    const exposureSelect = await within(exposureBuilder).findByRole('combobox', { name: '选择曝光量 曝光下拉商品' })
    fireEvent.change(exposureSelect, { target: { value: '100' } })
    fireEvent.click(within(exposureBuilder).getByRole('button', { name: '开启曝光 曝光下拉商品' }))

    await waitFor(() => expect(apiMocks.createExposureOrder).toHaveBeenCalledWith({
      uid: 'u1',
      itemIds: ['item-exposure-picker'],
      exposureNum: 100,
      expectedExposureNum: 100,
      picUrl: 'https://img.example.test/item-exposure-picker.jpg',
      title: '曝光下拉商品',
    }))
  })

  it('keeps fish-shop exposure creation clickable when exposure metadata requests time out', async () => {
    apiMocks.getShopWorkbench.mockRejectedValue(new Error('workbench unavailable'))
    apiMocks.getExposureHome.mockRejectedValue(new Error('exposure home unavailable'))
    apiMocks.getExposureAmountOptions.mockRejectedValue(new Error('amount options unavailable'))
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-exposure-timeout',
        uid: 'u1',
        title: '接口超时商品',
        price: '66.00',
        status: 'ON_SALE',
        pic_url: 'https://img.example.test/item-exposure-timeout.jpg',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getExposureItemRendering.mockResolvedValue({
      data: {
        itemActivityInstanceVOList: [
          { itemId: 'item-exposure-timeout', isPolishing: false },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const exposureBuilder = await screen.findByLabelText('鱼小铺曝光推广')
    const amountSelect = await within(exposureBuilder).findByRole('combobox', { name: '选择曝光量 接口超时商品' }) as HTMLSelectElement
    expect(amountSelect.value).toBe('100')
    const action = within(exposureBuilder).getByRole('button', { name: '开启曝光 接口超时商品' }) as HTMLButtonElement
    expect(action.disabled).toBe(false)

    fireEvent.click(action)

    await waitFor(() => expect(apiMocks.createExposureOrder).toHaveBeenCalledWith({
      uid: 'u1',
      itemIds: ['item-exposure-timeout'],
      exposureNum: 100,
      expectedExposureNum: 100,
      picUrl: 'https://img.example.test/item-exposure-timeout.jpg',
      title: '接口超时商品',
    }))
  })

  it('keeps fish-shop exposure creation visible but disabled when upstream reports no daily quantity', async () => {
    apiMocks.getExposureAmountOptions.mockResolvedValue({
      data: {
        hasQuantity: false,
        userAccount: 40,
        exposureAmountDOList: [
          { exposureNum: 50, title: '推荐补量' },
        ],
      },
    })
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-exposure-low',
        uid: 'u1',
        title: '余额不足商品',
        price: '66.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByLabelText('闲鱼币推广')).toBeTruthy()
    const exposureBuilder = await screen.findByLabelText('鱼小铺曝光推广')
    expect(within(exposureBuilder).getAllByText('今日曝光已抢完').length).toBeGreaterThan(0)
    const action = within(exposureBuilder).getByRole('button', { name: '开启曝光 余额不足商品' }) as HTMLButtonElement
    expect(action.disabled).toBe(true)
    expect(apiMocks.createExposureOrder).not.toHaveBeenCalled()
  })

  it('drops stale growth data when the auth token changes before the response returns', async () => {
    const signin = deferred<unknown>()
    const coinActivity = deferred<unknown>()
    const coinChallenge = deferred<unknown>()
    const coinSubscribe = deferred<unknown>()
    const coinDetails = deferred<unknown>()
    const coinCards = deferred<unknown>()
    const tasks = deferred<unknown>()
    const promotions = deferred<unknown>()
    const exposure = deferred<unknown>()
    const workbench = deferred<unknown>()
    apiMocks.getCoinSigninHome.mockReturnValue(signin.promise)
    apiMocks.getCoinActivityUserInfo.mockReturnValue(coinActivity.promise)
    apiMocks.getCoinChallengeInfo.mockReturnValue(coinChallenge.promise)
    apiMocks.getCoinSubscribeStatus.mockReturnValue(coinSubscribe.promise)
    apiMocks.getCoinUserDetailList.mockReturnValue(coinDetails.promise)
    apiMocks.getCoinVirtualCardList.mockReturnValue(coinCards.promise)
    apiMocks.getCoinTasks.mockReturnValue(tasks.promise)
    apiMocks.getPromotionPage.mockReturnValue(promotions.promise)
    apiMocks.getExposureTasks.mockReturnValue(exposure.promise)
    apiMocks.getShopWorkbench.mockReturnValue(workbench.promise)
    apiMocks.getExposureHistory.mockResolvedValue({ data: [] })
    apiMocks.getExposureDetails.mockResolvedValue({ data: [] })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getCoinSigninHome).toHaveBeenCalledWith('u1', expect.anything()))
    clientMocks.getAuthToken.mockReturnValue('token-b')

    await act(async () => {
      signin.resolve({ data: { coinNum: 99 } })
      coinActivity.resolve({ data: { myIdleCoin: 99, todaySignIn: true } })
      coinChallenge.resolve({ data: { failedFlag: false } })
      coinSubscribe.resolve({ data: { subscribe: true } })
      coinDetails.resolve({ data: { userScoreDetailDOList: [{ describe: '旧流水', score: 1 }] } })
      coinCards.resolve({ data: { data: [{ title: '旧卡券' }] } })
      tasks.resolve({ data: { taskList: [{ taskId: 'old-task', taskName: '旧用户任务' }] } })
      promotions.resolve({ data: { promotionList: [{ promotionId: 'old-promo', title: '旧用户推广' }] } })
      exposure.resolve({ data: { taskList: [] } })
      workbench.resolve({ data: { availableExposureFlow: 66 } })
      await Promise.resolve()
    })

    expect(screen.queryByText('旧用户任务')).toBeNull()
    expect(screen.queryByText('旧用户推广')).toBeNull()
    expect(screen.queryByText('旧流水')).toBeNull()
    expect(screen.queryByText('旧卡券')).toBeNull()
    expect(screen.queryByText('99')).toBeNull()
  })

  it('does not render the mobile sticky top bar in desktop mode', async () => {
    const { container } = render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(container.querySelector('.mobile-topbar')).toBeNull()
    expect(await screen.findByText('推广')).toBeTruthy()
  })

  it('stays stable when opening the promotion page with no active accounts', async () => {
    accountStoreMock.activeAccounts = []
    accountStoreMock.loadAccounts.mockResolvedValue([])

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('暂无可用账号')).toBeTruthy()
    expect((await screen.findAllByText('请先选择账号')).length).toBeGreaterThan(0)
  })

  it('keeps PromotionPage as a thin device router like other top-level modules', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'PromotionPage.tsx'), 'utf8')

    expect(source).toContain("import DesktopPromotion from './DesktopPromotion'")
    expect(source).toContain("import MobilePromotion from './MobilePromotion'")
    expect(source).not.toContain("from '../../components/TopBar'")
    expect(source).not.toContain("from '../../hooks/useAccounts'")
    expect(source).not.toContain("from '../../api/shopGrowth'")
  })

  it('keeps navigation responsive when growth responses contain huge nested payloads', async () => {
    const huge = Array.from({ length: 1500 }, (_, index) => ({
      node: {
        nested: {
          list: [{ taskId: `noise-${index}`, taskName: `噪声任务 ${index}` }],
        },
      },
    }))
    apiMocks.getCoinTasks.mockResolvedValue({ data: { wrappers: huge, taskList: [{ taskId: 'real-task', taskName: '真实浏览任务' }] } })
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        wrappers: huge,
        incentiveTask: {
          learningTaskList: [
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_visit_my_data_new' }, materialVO: { title: '每天查看「小铺数据」' } },
          ],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('每天查看「小铺数据」')).toBeTruthy()
    expect(screen.queryByText('真实浏览任务')).toBeNull()
    expect(screen.queryByText('噪声任务 1499')).toBeNull()
  })

  it('renders fish-shop exposure tasks from grouped incentiveTask payloads even when workbench fails', async () => {
    apiMocks.getShopWorkbench.mockRejectedValue(new Error('Invalid token'))
    apiMocks.getExposureHome.mockRejectedValue(new Error('exposure home unavailable'))
    apiMocks.getExposureAmountOptions.mockRejectedValue(new Error('amount options unavailable'))
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        currentFlow: 20,
        accumulateFlow: 80,
        incentiveTask: {
          learningTaskList: [{
            taskInstanceId: 1001,
            taskStatus: 'ACCEPTED',
            extend: { taskType: 'shop_visit_xy_coin', flowNum: '10' },
            materialVO: { title: '访问闲鱼币频道', desc: '浏览 5 秒' },
            benefitVO: {
              activityId: 100769,
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }],
            },
            progressVO: { times: 0, needTimes: 1 },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('访问闲鱼币频道')).toBeTruthy()
    const taskBoard = await screen.findByLabelText('今日任务台')
    expect(within(taskBoard).queryByText('仅展示可直接完成或领奖的曝光任务')).toBeNull()
    expect(within(taskBoard).queryByText('展示平台返回的鱼小铺曝光明细')).toBeNull()
    expect(within(taskBoard).queryByText('可用曝光量')).toBeNull()
    expect(within(taskBoard).queryByText('今日可处理')).toBeNull()
  })

  it('shows only Chinese labels for exposure task internals', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        currentFlow: 20,
        incentiveTask: {
          learningTaskList: [{
            taskInstanceId: 'learning-1',
            taskStatus: 'ACCEPTED',
            extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
            materialVO: {},
            benefitVO: {
              activityId: 100769,
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }],
            },
            progressVO: { times: 0, needTimes: 1 },
          }],
          serviceTaskList: [{
            taskInstanceId: 'subscribe-1',
            taskStatus: 'ACCEPTED',
            extend: { taskType: 'shop_subscribe_msg', flowNum: '10' },
            materialVO: {},
            benefitVO: {
              activityId: 100769,
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }],
            },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('每天查看小铺数据')).toBeTruthy()
    expect(screen.getByText('待完成 · 0/1')).toBeTruthy()
    expect(screen.queryByText('shop_visit_my_data_new')).toBeNull()
    expect(screen.queryByText('learningTaskList')).toBeNull()
    expect(screen.queryByText('ACCEPTED')).toBeNull()
    expect(screen.queryByText('高级 H5 任务链路')).toBeNull()
    expect(screen.queryByText('关注鱼小铺服务号')).toBeNull()
    expect(screen.queryByText('shop_subscribe_msg')).toBeNull()
    expect(screen.queryByText('serviceTaskList')).toBeNull()
  })

  it('keeps growth focused on coin and fish-shop exposure without querying super-polish state', async () => {
    apiMocks.getShopWorkbench.mockResolvedValue({ data: {} })
    apiMocks.getExposureTasks.mockResolvedValue({ data: { taskList: [] } })
    apiMocks.getSuperPolishAccount.mockResolvedValue({
      data: { usableBalanceOfYuan: '12.50', status: '正常' },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('暂无鱼小铺曝光任务')).toBeTruthy()
    expect(screen.queryByText('超级擦亮')).toBeNull()
    expect(screen.queryByRole('link', { name: '前往超级擦亮' })).toBeNull()
    expect(screen.queryByRole('button', { name: '触发超级擦亮' })).toBeNull()
    expect(apiMocks.getSuperPolishAccount).not.toHaveBeenCalled()
    expect(apiMocks.triggerSuperExposure).not.toHaveBeenCalled()
  })

  it('surfaces verified coin browse tasks once the completion path is revalidated', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [{
          deliveryId: 79493,
          sceneId: 496,
          idleTaskStatus: 'ACCEPTED',
          materialDTO: {
            title: '去浏览全新好物',
            actionType: 'VISIT',
            desc: '80',
            subTitle: '闲鱼币+80',
            action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&orderCode=mmOrderView02&adCode=mmOrder_view01&isView=true&source=xybrworder',
          },
          progressDTO: { times: 0, needTimes: 1 },
        }],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('去浏览全新好物')).toBeTruthy()
    expect(screen.getByRole('button', { name: '一键领币' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '一键领取曝光' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: '完成已验证浏览任务' })).toBeNull()
    expect(screen.queryByRole('button', { name: '完成可直接完成任务' })).toBeNull()
    expect(screen.getByRole('button', { name: '一键领币' }).getAttribute('disabled')).toBeNull()
    expect(apiMocks.completeCoinViewTasks).not.toHaveBeenCalled()
    expect(apiMocks.claimCoinTask).not.toHaveBeenCalled()
  })

  it('surfaces new structurally safe coin browse tasks without adding delivery-id constants', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [
          {
            deliveryId: 74620,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '搜一搜推荐商品',
              actionType: 'VISIT',
              desc: '100',
              subTitle: '闲鱼币+100 小店经验+100',
              action: 'https://ku.m.taobao.com/xianyu/search.html?titleVisible=false&adPositionCode=xybAlimmss&implId=xyb001&channelSrp=xybrwNew',
              extra: {
                oliverActivityId: '168',
                oliverBenefitCode: 'benefit-74620',
                oliverType: 0,
              },
            },
            taskBenefitDTO: {
              oliverActivityId: 168,
              oliverBenefitCode: 'benefit-74620',
              oliverType: 0,
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
          {
            deliveryId: 79496,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '搜一搜推荐商品',
              actionType: 'VISIT',
              desc: '100',
              action: 'https://ku.m.taobao.com/xianyu/search.html?titleVisible=false&adPositionCode=mmOrder_buy01',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
          {
            deliveryId: 94398,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '去神奇鱼塘领能量',
              actionType: 'clickFinish',
              desc: '100',
              action: 'https://ssr.m.goofish.com/wow/moyu/moyu-project/fish-pool/pages/home?from=xianyubi',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('搜一搜推荐商品')).toBeTruthy()
    expect(screen.queryByText('搜一搜推荐商品下单')).toBeNull()
    expect(screen.queryByText('去神奇鱼塘领能量')).toBeNull()
    expect(screen.getByRole('button', { name: '一键领币' }).getAttribute('disabled')).toBeNull()
  })

  it('keeps completed coin tasks visible as completed instead of dropping them', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [
          {
            deliveryId: 90101,
            sceneId: 496,
            idleTaskStatus: 'FINISHED',
            materialDTO: {
              title: '已完成浏览任务',
              actionType: 'VISIT',
              desc: '80',
              subTitle: '闲鱼币+80',
              action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&adCode=doneTask01',
            },
            progressDTO: { times: 1, needTimes: 1 },
          },
          {
            deliveryId: 90102,
            sceneId: 496,
            idleTaskStatus: 'RECEIVED',
            materialDTO: {
              title: '已领取浏览任务',
              actionType: 'VISIT',
              desc: '50',
              subTitle: '闲鱼币+50',
              action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&adCode=receivedTask01',
            },
            progressDTO: { times: 1, needTimes: 1 },
          },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('已完成浏览任务')).toBeTruthy()
    expect(await screen.findByText('已领取浏览任务')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已完成 已完成浏览任务' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.getByRole('button', { name: '已领取 已领取浏览任务' }).getAttribute('disabled')).not.toBeNull()
  })

  it('uses latest coin detail rewards as claimed task evidence when the live task list still says accepted', async () => {
    const today = localDateKey()
    const yesterday = offsetDateKey(-1)
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [
          {
            deliveryId: 79496,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '搜一搜推荐商品下单',
              actionType: 'VISIT',
              desc: '5000',
              action: 'https://ku.m.taobao.com/xianyu/search.html?titleVisible=false&adPositionCode=mmOrder_search01',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
          {
            deliveryId: 79494,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '去浏览全新好物下单',
              actionType: 'VISIT',
              desc: '5000',
              action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&adCode=mmOrder_view01',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
          {
            deliveryId: 85989,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '浏览国补商品下单',
              actionType: 'VISIT',
              desc: '5000',
              action: 'https://ku.m.taobao.com/xianyu/like.html?titleVisible=false&adPositionCode=mmOrderGuobu02',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
        ],
      },
    })
    apiMocks.getCoinUserDetailList.mockResolvedValue({
      data: {
        userScoreDetailDOList: [
          { describe: '完成阿里妈妈国补任务奖励', score: 100, status: 1, time: `${today} 16:16:20` },
          { describe: '完成浏览全新好物奖励', score: 80, status: 1, time: `${today} 16:16:18` },
          { describe: '完成阿里妈妈搜索任务奖励', score: 100, status: 1, time: `${today} 16:16:16` },
          { describe: '完成阿里妈妈搜索任务奖励', score: 100, status: 1, time: `${yesterday} 19:39:14` },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('搜一搜推荐商品下单')).toBeTruthy()
    expect(await screen.findByText('去浏览全新好物下单')).toBeTruthy()
    expect(await screen.findByText('浏览国补商品下单')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已领取 搜一搜推荐商品下单' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.getByRole('button', { name: '已领取 去浏览全新好物下单' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.getByRole('button', { name: '已领取 浏览国补商品下单' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.queryByRole('button', { name: '一键领币' })?.getAttribute('disabled')).not.toBeNull()
  })

  it('does not use yesterday coin detail rewards as claimed evidence for today tasks', async () => {
    const yesterday = offsetDateKey(-1)
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [
          {
            deliveryId: 79496,
            sceneId: 496,
            idleTaskStatus: 'ACCEPTED',
            materialDTO: {
              title: '搜一搜推荐商品',
              actionType: 'VISIT',
              desc: '5000',
              action: 'https://ku.m.taobao.com/xianyu/search.html?titleVisible=false&adPositionCode=mmOrder_search01',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
        ],
      },
    })
    apiMocks.getCoinUserDetailList.mockResolvedValue({
      data: {
        userScoreDetailDOList: [
          { describe: '完成阿里妈妈搜索任务奖励', score: 100, status: 1, time: `${yesterday} 19:39:14` },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('搜一搜推荐商品')).toBeTruthy()
    expect(screen.queryByRole('button', { name: '已领取 搜一搜推荐商品' })).toBeNull()
    expect(screen.getByRole('button', { name: '一键领币' }).getAttribute('disabled')).toBeNull()
  })

  it('keeps upstream claimed coin tasks visible even when they are no longer auto-completable', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [{
          deliveryId: 94398,
          sceneId: 496,
          idleTaskStatus: 'ACCEPTED',
          awardBtn: '已领取',
          materialDTO: {
            title: '已领取鱼塘任务',
            actionType: 'clickFinish',
            desc: '100',
            subTitle: '闲鱼币+100',
            action: 'https://ssr.m.goofish.com/wow/moyu/moyu-project/fish-pool/pages/home?from=xianyubi',
          },
          progressDTO: { times: 1, needTimes: 1 },
        }],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('已领取鱼塘任务')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已领取 已领取鱼塘任务' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.queryByText('暂无可直接处理的闲鱼币任务')).toBeNull()
  })

  it('treats Chinese coin reward status as already claimed', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [{
          deliveryId: 94399,
          sceneId: 496,
          idleTaskStatus: 'ACCEPTED',
          awardStatus: '已领取',
          materialDTO: {
            title: '中文状态已领取闲鱼币任务',
            actionType: 'clickFinish',
            desc: '100',
            subTitle: '闲鱼币+100',
            action: 'https://ssr.m.goofish.com/wow/moyu/moyu-project/fish-pool/pages/home?from=xianyubi',
          },
          progressDTO: { times: 1, needTimes: 1 },
        }],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('中文状态已领取闲鱼币任务')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已领取 中文状态已领取闲鱼币任务' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.queryByRole('button', { name: '领奖 中文状态已领取闲鱼币任务' })).toBeNull()
  })

  it('refreshes account growth state from upstream after writes instead of reusing the cached pre-write state', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    apiMocks.getCoinActivityUserInfo
      .mockResolvedValueOnce({ data: { myIdleCoin: 88, todaySignIn: false, alreadySignInDays: 6 } })
      .mockResolvedValueOnce({ data: { myIdleCoin: 138, todaySignIn: true, alreadySignInDays: 7 } })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect((await within(taskBoard).findAllByText('未签到')).length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '签到领币' }))

    await waitFor(() => expect(apiMocks.signInCoin).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(apiMocks.getCoinActivityUserInfo).toHaveBeenCalledTimes(2))
    expect((await within(taskBoard).findAllByText('已签到')).length).toBeGreaterThan(0)
    expect((await screen.findAllByText('138')).length).toBeGreaterThan(0)
  })

  it('keeps the signin card mounted while refresh is pending, then follows the upstream signin state', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    const signinRefresh = deferred<unknown>()
    const coinActivityRefresh = deferred<unknown>()

    apiMocks.getCoinSigninHome
      .mockResolvedValueOnce({ data: { coinNum: 88, todaySignIn: false, alreadySignInDays: 6 } })
      .mockReturnValueOnce(signinRefresh.promise)
    apiMocks.getCoinActivityUserInfo
      .mockResolvedValueOnce({ data: { myIdleCoin: 88, todaySignIn: false, alreadySignInDays: 6 } })
      .mockReturnValueOnce(coinActivityRefresh.promise)

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect((await within(taskBoard).findAllByText('未签到')).length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '签到领币' }))

    await waitFor(() => expect(apiMocks.signInCoin).toHaveBeenCalledTimes(1))
    expect(await within(screen.getByLabelText('今日任务台')).findByText('签到领闲鱼币')).toBeTruthy()
    expect((await within(screen.getByLabelText('今日任务台')).findAllByText('已签到')).length).toBeGreaterThan(0)

    await act(async () => {
      signinRefresh.resolve({ data: { coinNum: 88, todaySignIn: false, alreadySignInDays: 6 } })
      coinActivityRefresh.resolve({ data: { myIdleCoin: 88, todaySignIn: false, alreadySignInDays: 6 } })
      await Promise.resolve()
    })

    expect((await within(screen.getByLabelText('今日任务台')).findAllByText('未签到')).length).toBeGreaterThan(0)
  })

  it('uses signin home as the source of truth when activity user-info has stale todaySignIn', async () => {
    apiMocks.getCoinSigninHome.mockResolvedValue({
      data: {
        signedIn: true,
        signInAwarded: true,
        signInDays: 1,
      },
    })
    apiMocks.getCoinActivityUserInfo.mockResolvedValue({
      data: {
        myIdleCoin: 65088,
        todaySignIn: false,
        alreadySignInDays: 0,
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect((await within(taskBoard).findAllByText('已签到')).length).toBeGreaterThan(0)
    expect(within(taskBoard).getByRole('button', { name: '已签到' }).getAttribute('disabled')).not.toBeNull()
    expect(within(taskBoard).getByText('1天')).toBeTruthy()
    expect(within(taskBoard).getByText('65088')).toBeTruthy()
  })

  it('uses the upstream signin state after the growth page remounts', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)

    const rendered = render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect((await within(await screen.findByLabelText('今日任务台')).findAllByText('未签到')).length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '签到领币' }))
    await waitFor(() => expect(apiMocks.signInCoin).toHaveBeenCalledTimes(1))

    rendered.unmount()

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect((await within(await screen.findByLabelText('今日任务台')).findAllByText('未签到')).length).toBeGreaterThan(0)
  })

  it('lays out coin tasks and coin promotion with auto package loading without the removed fish-shop builder', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-1001',
        uid: 'u1',
        title: '春日露营车',
        price: '128.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          10: { costCoinNum: 100, exposureNum: 500 },
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByLabelText('闲鱼币推广')).toBeTruthy()
    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-1001'))
    expect((await screen.findAllByText(/100币/)).length).toBeGreaterThan(0)
    const cardTitles = screen.getAllByRole('heading', { level: 2 }).map((heading) => heading.textContent)
    expect(cardTitles.slice(0, 2)).toEqual(['今日任务台', '闲鱼币推广'])
    expect(screen.queryByLabelText('已购与曝光')).toBeNull()
    expect(await screen.findByLabelText('鱼小铺曝光推广')).toBeTruthy()
    expect(screen.queryByText('创建成功后进入同步队列')).toBeNull()
    expect(screen.queryByText('高级 H5 任务链路')).toBeNull()
    expect(screen.queryByText('ON_SALE')).toBeNull()
    expect(screen.queryByText('待查询')).toBeNull()
    expect(screen.queryByRole('button', { name: '查询套餐 春日露营车' })).toBeNull()
    expect(screen.queryByLabelText('商品 ID')).toBeNull()
    expect(screen.getByRole('button', { name: '开启推广 春日露营车' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: '确认开启曝光' })).toBeNull()
    expect(screen.queryByText('可用曝光量')).toBeNull()
    expect(screen.queryByText('今日可处理')).toBeNull()
  })

  it('renders promotion items from the backend itemList response shape', async () => {
    marketMocks.getItems.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      _last_sync_ok: true,
      _last_synced_at: 1782640976,
      data: {
        itemList: [{
          itemId: 'item-backend-1',
          title: '后端包装商品',
          price: '66.00',
          status: 'ON_SALE',
        }],
        totalCount: 1,
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect((await screen.findAllByText('后端包装商品')).length).toBeGreaterThan(0)
    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-backend-1'))
    expect(screen.queryByRole('button', { name: '查询套餐 后端包装商品' })).toBeNull()
  })

  it('queues automatic coin promotion package queries instead of firing all listed items at once', async () => {
    const firstPackage = deferred<unknown>()
    marketMocks.getItems.mockResolvedValue({
      items: [
        {
          item_id: 'item-queue-1',
          uid: 'u1',
          title: '排队套餐商品一',
          price: '66.00',
          status: 'ON_SALE',
        },
        {
          item_id: 'item-queue-2',
          uid: 'u1',
          title: '排队套餐商品二',
          price: '88.00',
          status: 'ON_SALE',
        },
      ],
      pagination: { page: 1, page_size: 20, total: 2 },
    })
    apiMocks.getPromotionView
      .mockReturnValueOnce(firstPackage.promise)
      .mockResolvedValueOnce({
        data: {
          promotionPriceMap: {
            10: { costCoinNum: 100, exposureNum: 500 },
          },
        },
      })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect((await screen.findAllByText('排队套餐商品一')).length).toBeGreaterThan(0)
    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledTimes(1))
    expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-queue-1')
    expect(apiMocks.getPromotionView).not.toHaveBeenCalledWith('u1', 'item-queue-2')

    await act(async () => {
      firstPackage.resolve({
        data: {
          promotionPriceMap: {
            10: { costCoinNum: 100, exposureNum: 500 },
          },
        },
      })
      await Promise.resolve()
    })

    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-queue-2'))
  })

  it('force-syncs promotion items once when the local item cache has never synced', async () => {
    marketMocks.getItems
      .mockResolvedValueOnce({
        ret: ['SUCCESS::调用成功'],
        _last_sync_ok: false,
        _last_synced_at: 0,
        data: { itemList: [], totalCount: 0 },
      })
      .mockResolvedValueOnce({
        ret: ['SUCCESS::调用成功'],
        _last_sync_ok: true,
        _last_synced_at: 1782640976,
        data: {
          itemList: [{
            itemId: 'item-synced-1',
            title: '同步后商品',
            price: '99.00',
            status: 'ON_SALE',
          }],
          totalCount: 1,
        },
      })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect((await screen.findAllByText('同步后商品')).length).toBeGreaterThan(0)
    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-synced-1'))
    expect(marketMocks.getItems).toHaveBeenNthCalledWith(1, 'u1', 1, 20, 'ON_SALE', { force: false })
    expect(marketMocks.getItems).toHaveBeenNthCalledWith(2, 'u1', 1, 20, 'ON_SALE', { force: true })
  })

  it('creates a coin promotion from the listed item using the queried package', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-1001',
        uid: 'u1',
        title: '春日露营车',
        price: '128.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          10: { costCoinNum: 100, exposureNum: 500 },
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-1001'))
    expect((await screen.findAllByText(/100币/)).length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: '查询套餐 春日露营车' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: '开启推广 春日露营车' }))

    await waitFor(() => expect(apiMocks.createPromotion).toHaveBeenCalledWith({
      uid: 'u1',
      itemId: 'item-1001',
      promotionNum: '10',
      costCoinNum: 100,
      expectedCostCoinNum: 100,
    }))
    await waitFor(() => expect(notifyMock).toHaveBeenCalledWith('商品「春日露营车」闲鱼币推广已开启', 'success'))
    expect(notifyMock).not.toHaveBeenCalledWith('操作成功', 'success')
  })

  it('selects a coin promotion package from the row dropdown', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: 'item-package-picker',
        uid: 'u1',
        title: '套餐下拉商品',
        price: '128.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          10: { costCoinNum: 100, exposureNum: 500, name: '轻量推广' },
          20: { costCoinNum: 300, exposureNum: 1500, name: '集中推广' },
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const coinBuilder = await screen.findByLabelText('闲鱼币推广')
    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-package-picker'))
    const packageSelect = await within(coinBuilder).findByRole('combobox', { name: '选择套餐 套餐下拉商品' })
    fireEvent.change(packageSelect, { target: { value: '20' } })
    fireEvent.click(within(coinBuilder).getByRole('button', { name: '开启推广 套餐下拉商品' }))

    await waitFor(() => expect(apiMocks.createPromotion).toHaveBeenCalledWith({
      uid: 'u1',
      itemId: 'item-package-picker',
      promotionNum: '20',
      costCoinNum: 300,
      expectedCostCoinNum: 300,
    }))
  })

  it('marks listed items that already have a purchased coin promotion and keeps creation on row actions only', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [
        {
          item_id: 'item-opened-1',
          uid: 'u1',
          title: '已经推广商品',
          price: '88.00',
          status: 'ON_SALE',
        },
        {
          item_id: 'item-new-1',
          uid: 'u1',
          title: '未推广商品',
          price: '99.00',
          status: 'ON_SALE',
        },
      ],
      pagination: { page: 1, page_size: 20, total: 2 },
    })
    apiMocks.getPromotionPage.mockResolvedValue({
      data: {
        promotionList: [{
          promotionId: 'promo-opened-1',
          itemVO: { itemId: 'item-opened-1', title: '已经推广商品' },
          statusDesc: '投放中',
          aimedPromoteNum: 50,
          realPromoteNum: 13,
        }],
      },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          10: { costCoinNum: 100, exposureNum: 500 },
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const coinBuilder = await screen.findByLabelText('闲鱼币推广')
    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-opened-1'))
    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', 'item-new-1'))

    expect(within(coinBuilder).queryByRole('button', { name: '确认开启推广' })).toBeNull()
    expect(within(coinBuilder).queryByRole('button', { name: '开启推广 已经推广商品' })).toBeNull()
    expect(within(coinBuilder).getByText('投放中')).toBeTruthy()
    expect(within(coinBuilder).getByText('已开启')).toBeTruthy()
    expect(within(coinBuilder).getByRole('button', { name: '开启推广 未推广商品' })).toBeTruthy()
  })

  it('does not fetch or render the removed purchased-promotion and exposure-ledger section on page load', async () => {
    apiMocks.getPromotionPage.mockResolvedValue({
      data: {
        page: {
          records: [
            {
              promotionId: 'promo-nested-1',
              itemDTO: { itemTitle: '嵌套推广商品' },
              promotionDTO: {
                costCoinNum: 1500,
                promotionNum: 50,
                statusDesc: '投放中',
                gmtCreate: 1782100000000,
              },
            },
          ],
        },
      },
    })
    apiMocks.getExposureHistory.mockResolvedValue({
      data: {
        exposureHistoryList: {
          result: [
            {
              itemInfo: { itemTitle: '曝光历史商品', itemId: 'item-history-1' },
              exposureInfo: { exposureNum: 66, date: '20260622' },
              detail: { instanceId: 'inst-1', bizCode: 'biz-1' },
            },
          ],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getPromotionPage).toHaveBeenCalled())
    expect(apiMocks.getExposureHistory).not.toHaveBeenCalled()
    expect(screen.queryByLabelText('已购与曝光')).toBeNull()
    expect(screen.queryByRole('button', { name: '曝光历史' })).toBeNull()
    expect(screen.queryByRole('button', { name: /推广详情/ })).toBeNull()
    expect(screen.queryByText('嵌套推广商品')).toBeNull()
    expect(screen.queryByText('曝光历史商品')).toBeNull()
  })

  it('claims verified coin rewards directly after the browsing task is already ready', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [{
          deliveryId: 79493,
          sceneId: 496,
          idleTaskStatus: 'AWARD',
          groupId: 34344,
          implId: 'other_560_1_79493_0',
          fromToken: 'token-1',
          materialDTO: {
            title: '去浏览全新好物',
            actionType: 'VISIT',
            desc: '80',
            subTitle: '闲鱼币+80',
            extra: {
              oliverActivityId: '168',
              oliverBenefitCode: 'benefit-1',
              oliverType: 0,
            },
          },
          taskBenefitDTO: {
            oliverActivityId: 168,
            oliverBenefitCode: 'benefit-1',
            oliverType: 0,
          },
          progressDTO: { times: 1, needTimes: 1 },
        }],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('去浏览全新好物')).toBeTruthy()
    fireEvent.click(screen.getByLabelText('领奖 去浏览全新好物'))
    await waitFor(() => expect(apiMocks.claimCoinTask).toHaveBeenCalledTimes(1))
    expect(apiMocks.completeCoinViewTasks).not.toHaveBeenCalled()
  })

  it('shows only directly actionable tasks backed by live evidence', async () => {
    apiMocks.getCoinTasks.mockResolvedValue({
      data: {
        taskList: [
          { deliveryId: 94398, materialDTO: { title: '去神奇鱼塘领能量', actionType: 'clickFinish', desc: '80' }, progressDTO: { times: 0, needTimes: 1 } },
          {
            deliveryId: 79495,
            materialDTO: {
              title: '搜一搜推荐商品',
              actionType: 'VISIT',
              desc: '100',
              action: 'https://ku.m.taobao.com/xianyu/search.html?titleVisible=false&adPositionCode=mmOrder_search01&implId=mms0101',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
          { deliveryId: 75778, materialDTO: { title: '点击指定频道好物', actionType: 'PLAINVISIT', desc: '100' }, progressDTO: { times: 0, needTimes: 10 } },
          {
            deliveryId: 79493,
            materialDTO: {
              title: '去浏览全新好物',
              actionType: 'VISIT',
              desc: '80',
              action: 'https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&adCode=mmOrder_view01',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
          {
            deliveryId: 85981,
            materialDTO: {
              title: '浏览推荐的国补商品',
              actionType: 'VISIT',
              desc: '100',
              action: 'https://ku.m.taobao.com/xianyu/like.html?titleVisible=false&adPositionCode=mmOrderGuobu01&implId=mms0201',
            },
            progressDTO: { times: 0, needTimes: 1 },
          },
          { deliveryId: 88299, materialDTO: { title: '浏览鱼小铺工作台', actionType: 'clickFinish', desc: '50' }, progressDTO: { times: 0, needTimes: 1 } },
        ],
      },
    })
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        currentFlow: 90,
        incentiveTask: {
          learningTaskList: [
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_subscribe_msg' }, materialVO: { title: '关注鱼小铺服务号' } },
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_visit_my_data_new' }, materialVO: { title: '每天查看「小铺数据」' } },
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_visit_diagnosis' }, materialVO: { title: '查看小铺诊断建议' } },
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_visit_xy_coin' }, materialVO: { title: '访问闲鱼币频道' } },
          ],
          publishTaskList: [
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_main_pubilsh' }, materialVO: { title: '发布一个宝贝' } },
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_open_groupon' }, materialVO: { title: '开启1个“2人小刀”宝贝' } },
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_improvePack_visit' }, materialVO: { title: '查看“运营提效包”功能页' } },
          ],
          orderTaskList: [
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_order_one_step' }, materialVO: { title: '订单量' } },
          ],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('每天查看「小铺数据」')).toBeTruthy()
    expect(await screen.findByText('查看小铺诊断建议')).toBeTruthy()
    expect(await screen.findByText('访问闲鱼币频道')).toBeTruthy()
    expect(await screen.findByText('查看“运营提效包”功能页')).toBeTruthy()

    expect(await screen.findByText('搜一搜推荐商品')).toBeTruthy()
    expect(await screen.findByText('去浏览全新好物')).toBeTruthy()
    expect(await screen.findByText('浏览推荐的国补商品')).toBeTruthy()
    expect(screen.queryByText('去神奇鱼塘领能量')).toBeNull()
    expect(screen.queryByText('点击指定频道好物')).toBeNull()
    expect(screen.queryByText('浏览鱼小铺工作台')).toBeNull()
    expect(screen.queryByText('关注鱼小铺服务号')).toBeNull()
    expect(screen.queryByText('发布一个宝贝')).toBeNull()
    expect(screen.queryByText('开启1个“2人小刀”宝贝')).toBeNull()
    expect(screen.queryByText('订单量')).toBeNull()
    expect(screen.queryByText('仅展示可直接完成或领奖的曝光任务')).toBeNull()
    expect(screen.queryByText('展示平台返回的鱼小铺曝光明细')).toBeNull()
    expect(screen.queryByText('H5 已验证链路')).toBeNull()
    expect(screen.queryByText('高级 H5 任务链路')).toBeNull()
  })

  it('does not expose advanced verified exposure capabilities in the default task list', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        currentFlow: 90,
        incentiveTask: {
          learningTaskList: [
            { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_visit_my_data_new' }, materialVO: { title: '每天查看「小铺数据」' } },
            { taskStatus: 'ACCEPTED', taskId: 'subscribe-1', taskInstanceId: 'subscribe-1', benefitId: 'benefit-subscribe-1', benefitVO: { activityId: '100769' }, extend: { taskType: 'shop_subscribe_msg' }, materialVO: { title: '关注鱼小铺服务号' } },
          ],
          publishTaskList: [
            { taskStatus: 'ACCEPTED', taskId: 'publish-1', taskInstanceId: 'publish-1', benefitId: 'benefit-publish-1', benefitVO: { activityId: '100769' }, extend: { taskType: 'shop_main_pubilsh' }, materialVO: { title: '发布一个宝贝' } },
          ],
          orderTaskList: [
            { taskStatus: 'ACCEPTED', taskId: 'order-1', taskInstanceId: 'order-1', benefitId: 'benefit-order-1', benefitVO: { activityId: '100769' }, extend: { taskType: 'shop_order_one_step' }, materialVO: { title: '订单量' } },
          ],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('每天查看「小铺数据」')).toBeTruthy()
    expect(screen.queryByText('关注鱼小铺服务号')).toBeNull()
    expect(screen.queryByText('发布一个宝贝')).toBeNull()
    expect(screen.queryByText('订单量')).toBeNull()
    expect(screen.queryByText('高级 H5 任务链路')).toBeNull()
    expect(screen.queryByLabelText('执行 关注鱼小铺服务号')).toBeNull()
    expect(apiMocks.runExposureTask).not.toHaveBeenCalled()
  })

  it('loads exposure history only after opening the exposure detail tab and does not prefetch row details', async () => {
    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect(apiMocks.getExposureHistory).not.toHaveBeenCalled()
    const detailTabs = within(taskBoard).getAllByRole('button', { name: '明细' })
    fireEvent.click(detailTabs[detailTabs.length - 1])

    await waitFor(() => expect(apiMocks.getExposureHistory).toHaveBeenCalledWith('u1', 1, 20, expect.anything()))
    expect(apiMocks.getExposureDetails).not.toHaveBeenCalled()
  })

  it('keeps the promotion builder responsive while secondary growth queries are still pending', async () => {
    const exposureTasks = deferred<unknown>()

    apiMocks.getPromotionPage.mockResolvedValue({
      data: {
        promotionList: [{ promotionId: 'promo-1', itemTitle: '首屏推广商品', costCoinNum: 50, status: '投放中' }],
      },
    })
    apiMocks.getExposureTasks.mockReturnValue(exposureTasks.promise)

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('暂无在售商品')).toBeTruthy()
    expect(screen.queryByText('首屏推广商品')).toBeNull()
    expect(apiMocks.getCoinUserDetailList).not.toHaveBeenCalled()
    expect(apiMocks.getExposureHistory).not.toHaveBeenCalled()
    expect(apiMocks.getExposureTasks).toHaveBeenCalled()
  })

  it('shows task loading failures instead of empty task states when growth task requests time out', async () => {
    apiMocks.getCoinTasks.mockRejectedValueOnce(new Error('闲鱼币任务加载超时'))
    apiMocks.getExposureTasks.mockRejectedValueOnce(new Error('曝光任务加载超时'))

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect(await within(taskBoard).findByText('闲鱼币任务加载失败')).toBeTruthy()
    expect(await within(taskBoard).findByText('曝光任务加载失败')).toBeTruthy()
    expect(within(taskBoard).queryByText('暂无可直接处理的闲鱼币任务')).toBeNull()
    expect(within(taskBoard).queryByText('暂无可直接完成的鱼小铺任务')).toBeNull()
  })

  it('loads exposure history into the task detail tab without restoring old detail entry points', async () => {
    apiMocks.getExposureHistory.mockResolvedValue({
      data: {
        historyList: [
          { itemTitle: '没有明细标识', exposureCnt: 3, date: '2026-06-10' },
          { itemTitle: '可查曝光商品', exposureCnt: 8, date: '2026-06-11', instanceId: 'inst-1', bizCode: 'biz-1', itemId: 'item-1' },
        ],
      },
    })
    apiMocks.getExposureDetails.mockResolvedValue({
      data: {
        detailList: [
          { itemTitle: '可查曝光商品', exposureCnt: 8, date: '2026-06-11' },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect(apiMocks.getExposureHistory).not.toHaveBeenCalled()
    const detailTabs = within(taskBoard).getAllByRole('button', { name: '明细' })
    fireEvent.click(detailTabs[detailTabs.length - 1])

    await waitFor(() => expect(apiMocks.getExposureHistory).toHaveBeenCalledWith('u1', 1, 20, expect.anything()))
    expect(screen.queryByRole('button', { name: '曝光历史' })).toBeNull()
    expect(await within(taskBoard).findByText('没有明细标识')).toBeTruthy()
    expect(await within(taskBoard).findByText('可查曝光商品')).toBeTruthy()
    expect(screen.queryByLabelText('查看 可查曝光商品 的曝光明细')).toBeNull()
    expect(apiMocks.getExposureDetails).not.toHaveBeenCalled()
  })

  it('does not expose promotion detail entry points after the third section is removed', async () => {
    apiMocks.getPromotionPage.mockResolvedValue({
      data: {
        promotionList: [
          { itemId: 'item-only-1', itemTitle: '只有商品 ID 的推广记录', costCoinNum: 30, status: '投放中' },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getPromotionPage).toHaveBeenCalled())
    expect(screen.queryByText('只有商品 ID 的推广记录')).toBeNull()
    expect(screen.queryByRole('button', { name: /推广详情/ })).toBeNull()
    expect(apiMocks.getPromotionDetail).not.toHaveBeenCalled()
  })

  it('shows exposure flow from exposure task payload when workbench data is sparse', async () => {
    apiMocks.getShopWorkbench.mockResolvedValue({ data: { taskCenter: {} } })
    apiMocks.getExposureHome.mockRejectedValue(new Error('exposure home unavailable'))
    apiMocks.getExposureAmountOptions.mockRejectedValue(new Error('amount options unavailable'))
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        availableExposureFlow: 321,
        taskList: [
          { taskStatus: 'ACCEPTED', extend: { taskType: 'shop_visit_my_data_new' }, materialVO: { title: '每天查看「小铺数据」' } },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect((await screen.findAllByText('321')).length).toBeGreaterThan(0)
  })

  it('shows the queried package on the matching listed item', async () => {
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: '1234567890',
        uid: 'u1',
        title: '可推广商品',
        price: '88.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          '5': { costCoinNum: 50, exposureNum: 500 },
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', '1234567890'))

    expect((await screen.findAllByText(/50币/)).length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: '查询套餐 可推广商品' })).toBeNull()
    expect(screen.getByRole('button', { name: '开启推广 可推广商品' })).toBeTruthy()
  })

  it('creates a coin promotion plan from the listed item flow', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: '1234567890',
        uid: 'u1',
        title: '可推广商品',
        price: '88.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          '5': { costCoinNum: 50, exposureNum: 500 },
        },
      },
    })
    apiMocks.createPromotion.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { activityId: 'promo-created' } })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', '1234567890'))
    fireEvent.click(await screen.findByRole('button', { name: '开启推广 可推广商品' }))

    await waitFor(() => expect(apiMocks.createPromotion).toHaveBeenCalledWith({
      uid: 'u1',
      itemId: '1234567890',
      promotionNum: '5',
      costCoinNum: 50,
      expectedCostCoinNum: 50,
    }))
  })

  it('retries purchased promotion records after the creation refresh times out', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: '1234567890',
        uid: 'u1',
        title: '可推广商品',
        price: '88.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          '5': { costCoinNum: 50, exposureNum: 500 },
        },
      },
    })
    apiMocks.createPromotion.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { activityId: 'promo-created' } })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', '1234567890'))
    apiMocks.getPromotionPage.mockClear()
    apiMocks.getPromotionPage
      .mockRejectedValueOnce(new Error('推广记录加载超时'))
      .mockResolvedValueOnce({
        data: {
          promotionList: [{
            promotionId: 'promo-created',
            itemTitle: '创建后推广商品',
            costCoinNum: 50,
            promotionNum: '5',
            status: '投放中',
          }],
        },
      })

    fireEvent.click(await screen.findByRole('button', { name: '开启推广 可推广商品' }))

    await waitFor(() => expect(apiMocks.createPromotion).toHaveBeenCalledTimes(1))
    expect(apiMocks.getPromotionPage).toHaveBeenCalledTimes(2)
    expect(screen.queryByText('创建后推广商品')).toBeNull()
  })

  it('does not render local pending purchased records when the third section is hidden', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    marketMocks.getItems.mockResolvedValue({
      items: [{
        item_id: '1234567890',
        uid: 'u1',
        title: '可推广商品',
        price: '88.00',
        status: 'ON_SALE',
      }],
      pagination: { page: 1, page_size: 20, total: 1 },
    })
    apiMocks.getPromotionView.mockResolvedValue({
      data: {
        promotionPriceMap: {
          '5': { costCoinNum: 50, exposureNum: 500 },
        },
      },
    })
    apiMocks.createPromotion.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { activityId: 'promo-created' } })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(apiMocks.getPromotionView).toHaveBeenCalledWith('u1', '1234567890'))
    apiMocks.getPromotionPage.mockClear()
    apiMocks.getPromotionPage.mockResolvedValue({ data: { promotionList: [] } })

    fireEvent.click(await screen.findByRole('button', { name: '开启推广 可推广商品' }))

    await waitFor(() => expect(apiMocks.createPromotion).toHaveBeenCalledTimes(1))
    expect(screen.queryByText(/已创建，等待远端同步/)).toBeNull()
    expect(screen.queryByText('暂无已购推广')).toBeNull()
  })

  it('renders coin detail records without exposing the card tab', async () => {
    apiMocks.getCoinActivityUserInfo.mockResolvedValue({ data: { myIdleCoin: 15314, todaySignIn: false, alreadySignInDays: 7 } })
    apiMocks.getCoinUserDetailList.mockResolvedValue({
      data: {
        userScoreDetailDOList: [
          { describe: '签到奖励', score: 50, time: '2026-06-10 10:00:00' },
        ],
      },
    })
    apiMocks.getCoinVirtualCardList.mockResolvedValue({
      data: {
        data: [
          { title: '推广抵扣券', status: 'AVAILABLE', amount: 1 },
        ],
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect((await screen.findAllByText('15314')).length).toBeGreaterThan(0)
    const taskBoard = await screen.findByLabelText('今日任务台')
    expect(within(taskBoard).queryByRole('button', { name: /卡券/ })).toBeNull()
    fireEvent.click(within(taskBoard).getAllByRole('button', { name: '明细' })[0])
    expect(await screen.findByText('签到奖励')).toBeTruthy()
    expect(screen.queryByText('推广抵扣券')).toBeNull()
  })

  it('adds task and detail tabs for fish-shop exposure tasks', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        incentiveTask: {
          learningTaskList: [{
            taskInstanceId: 'learning-1',
            taskStatus: 'ACCEPTED',
            extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
            materialVO: { title: '每天查看小铺数据' },
            benefitVO: { activityId: 100769, rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }] },
            progressVO: { times: 0, needTimes: 1 },
          }],
        },
      },
    })
    apiMocks.getExposureHistory.mockResolvedValue({
      data: {
        exposureHistoryList: {
          result: [{
            itemInfo: { itemTitle: '曝光明细商品', itemId: 'item-history-1' },
            exposureInfo: { exposureNum: 66, date: '2026-06-22' },
            detail: { instanceId: 'inst-1', bizCode: 'biz-1' },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    const taskBoard = await screen.findByLabelText('今日任务台')
    expect(await within(taskBoard).findByText('每天查看小铺数据')).toBeTruthy()
    expect(within(taskBoard).getAllByRole('button', { name: '任务' }).length).toBeGreaterThanOrEqual(2)
    expect(within(taskBoard).getAllByRole('button', { name: '明细' }).length).toBeGreaterThanOrEqual(2)

    const detailTabs = within(taskBoard).getAllByRole('button', { name: '明细' })
    fireEvent.click(detailTabs[detailTabs.length - 1])

    expect(await within(taskBoard).findByText('曝光明细商品')).toBeTruthy()
    expect(within(taskBoard).getByText('66 曝光 · 2026-06-22')).toBeTruthy()
    expect(within(taskBoard).queryByText('promotionDetailVO')).toBeNull()
  })

  it('disables exposure reward actions when upstream marks the reward as already claimed', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        incentiveTask: {
          learningTaskList: [{
            taskInstanceId: 'claimed-exposure-1',
            taskStatus: 'COMPLETE',
            awardBtn: '已领取',
            extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
            materialVO: { title: '已领取小铺数据任务' },
            benefitVO: {
              activityId: 100769,
              status: 'RECEIVED',
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT', status: 'RECEIVED' }],
            },
            progressVO: { times: 1, needTimes: 1 },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('已领取小铺数据任务')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已领取 已领取小铺数据任务' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.queryByRole('button', { name: '领奖 已领取小铺数据任务' })).toBeNull()
  })

  it('treats Chinese exposure reward status as already claimed', async () => {
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        incentiveTask: {
          learningTaskList: [{
            taskInstanceId: 'claimed-exposure-cn',
            taskStatus: 'COMPLETE',
            awardStatus: '已领取',
            extend: { taskType: 'shop_visit_my_data_new', flowNum: '10' },
            materialVO: { title: '中文状态已领取曝光任务' },
            benefitVO: {
              activityId: 100769,
              rewardList: [{ denomination: 10, rewardType: 'FLOW_AMOUNT' }],
            },
            progressVO: { times: 1, needTimes: 1 },
          }],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    expect(await screen.findByText('中文状态已领取曝光任务')).toBeTruthy()
    expect(screen.getByRole('button', { name: '已领取 中文状态已领取曝光任务' }).getAttribute('disabled')).not.toBeNull()
    expect(screen.queryByRole('button', { name: '领奖 中文状态已领取曝光任务' })).toBeNull()
  })

  it('keeps composite write results as business summaries without exposing transport attempts', async () => {
    dialogMocks.showConfirm.mockResolvedValueOnce(true)
    apiMocks.receiveExposureReward.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      transportMeta: {
        preferred: 'app',
        chosen: 'h5',
        fallbackUsed: true,
        attempts: [
          { transport: 'app', success: false, error: 'APP_LINK_FAILED' },
          { transport: 'h5', success: true, ret: 'SUCCESS::调用成功' },
        ],
      },
      data: {
        receiveRewardResult: {
          receivedRewards: [{ taskId: 'task-1' }],
        },
      },
    })
    apiMocks.getExposureTasks.mockResolvedValue({
      data: {
        currentFlow: 90,
        incentiveTask: {
          learningTaskList: [
            {
              taskStatus: 'AWARD',
              taskId: 'data-1',
              taskInstanceId: 'data-1',
              benefitId: 'benefit-data-1',
              benefitVO: { activityId: '100769' },
              extend: { taskType: 'shop_visit_my_data_new' },
              materialVO: { title: '每天查看小铺数据' },
            },
          ],
        },
      },
    })

    render(
      <MemoryRouter>
        <PromotionPage />
      </MemoryRouter>,
    )

    fireEvent.click(await screen.findByRole('button', { name: '领奖 每天查看小铺数据' }))

    await waitFor(() => expect(apiMocks.receiveExposureReward).toHaveBeenCalledTimes(1))
    expect(screen.queryByText('最近一次执行结果')).toBeNull()
    expect(screen.queryByText('App')).toBeNull()
    expect(screen.queryByText('H5')).toBeNull()
    expect(screen.queryByText('已回退')).toBeNull()
    expect(screen.queryByText('APP_LINK_FAILED')).toBeNull()
    expect(screen.queryByText('领奖 1')).toBeNull()
  })
})
