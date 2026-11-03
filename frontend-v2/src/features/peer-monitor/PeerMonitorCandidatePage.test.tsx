import React from 'react'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { peerMonitorApi } = vi.hoisted(() => ({
  peerMonitorApi: {
    createPeerMonitorTarget: vi.fn(),
    getPeerMonitorCandidate: vi.fn(),
    getPeerMonitorCandidateFromLink: vi.fn(),
    getPeerMonitorCandidateReviews: vi.fn(),
  },
}))

vi.mock('../../api/peerMonitor', () => peerMonitorApi)
vi.mock('./PeerMonitorTabs', () => ({
  PeerMonitorTabs: () => <div data-testid="peer-monitor-tabs" />,
}))
import PeerMonitorCandidatePage from './PeerMonitorCandidatePage'

const candidate = {
  sellerId: 'seller-1',
  sellerNick: '同行卖家甲',
  sellerAvatar: 'https://img.example.test/avatar.jpg',
  profileBackgroundUrl: 'https://img.example.test/profile-background.jpg',
  sellerLevel: '闲小铺 L3',
  shopLevel: 'L3',
  area: '杭州',
  ipRegion: '浙江',
  residence: '西湖区',
  followersCount: 1234,
  followingCount: 86,
  visitTime: '2小时前来过',
  itemCount: 4,
  rateCount: 12,
  dynamicCount: 9,
  postCount: 3,
  shopScore: '4.9',
  praiseRatio: '99%',
  businessQuality: '优秀',
  replyRatio24h: '98%',
  replyInterval: '4 分钟',
  registeredDays: 860,
  soldCount: 126,
  creditLevel: '优秀',
  profileTags: ['实名认证', '闲置数码', '支持当面验货与同城自提。'],
  verificationTags: ['实名认证', '芝麻信用已授权'],
  sellerTags: ['发货快', '描述相符'],
  sellerSummary: '专注二手数码，支持验货。',
  shopDescription: '支持当面验货与同城自提。',
  timelineItems: [{ id: 'dynamic-1', title: '真实动态商品', price: '88', date: '20260730' }],
}

function renderSellerHome() {
  return render(
    <MemoryRouter initialEntries={['/peer-monitor/items/item-1/home?uid=search-account-1']}>
      <Routes>
        <Route path="/peer-monitor/items/:itemId/home" element={<PeerMonitorCandidatePage sellerHome />} />
      </Routes>
    </MemoryRouter>,
  )
}

function renderLinkImport(link: string) {
  return render(
    <MemoryRouter initialEntries={[`/peer-monitor/import?link=${encodeURIComponent(link)}`]}>
      <Routes>
        <Route path="/peer-monitor/import" element={<PeerMonitorCandidatePage />} />
      </Routes>
    </MemoryRouter>,
  )
}

function renderSearchItem() {
  return render(
    <MemoryRouter initialEntries={['/peer-monitor/items/item-search-1?uid=search-account-1']}>
      <Routes>
        <Route path="/peer-monitor/items/:itemId" element={<PeerMonitorCandidatePage />} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('PeerMonitorCandidatePage reviews', () => {
  afterEach(() => cleanup())

  beforeEach(() => {
    vi.clearAllMocks()
    peerMonitorApi.getPeerMonitorCandidate.mockResolvedValue({ candidate })
    peerMonitorApi.getPeerMonitorCandidateReviews.mockResolvedValue({
      source: 'wx_seller_rate',
      reviews: [],
      hasNext: false,
    })
  })

  it('renders review content directly after appending item-rate reviews from a later page', async () => {
    peerMonitorApi.getPeerMonitorCandidateReviews.mockResolvedValueOnce({
      source: 'wx_seller_rate',
      reviews: [{
        id: 'rate-1',
        feedback: '成色很好，卖家回复及时。',
        raterNick: '买家甲',
        createdAt: '2026-07-30',
        tags: ['描述相符'],
        pictures: ['https://img.example.test/review.jpg'],
        appendReview: { feedback: '用了一周依然很好', pictures: ['https://img.example.test/append.jpg'] },
        reply: { feedback: '感谢认可' },
      }],
      hasNext: true,
    }).mockResolvedValueOnce({
      source: 'item_rate',
      reviews: [{
        id: 'rate-2',
        feedback: '商品级评价内容',
        raterNick: '买家乙',
        createdAt: '2026-07-31',
        itemTitle: '平台返回的商品标题',
        itemPic: 'https://img.example.test/item.jpg',
      }],
      hasNext: false,
    })

    renderSellerHome()

    await screen.findByText('同行卖家甲')
    fireEvent.click(screen.getByRole('button', { name: '评价' }))

    await screen.findByText('账号评价')
    expect(peerMonitorApi.getPeerMonitorCandidateReviews).toHaveBeenCalledWith('seller-1', 1, 'search-account-1')
    expect(screen.queryByText('平台未公开可归属商品时，不展示评论商品')).toBeNull()
    expect(screen.getByText('成色很好，卖家回复及时。')).toBeTruthy()
    expect(screen.getByText('追评：用了一周依然很好')).toBeTruthy()
    expect(screen.getByText('卖家回复：感谢认可')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: '下一页' }))
    await screen.findByText('商品级评价内容')
    expect(peerMonitorApi.getPeerMonitorCandidateReviews).toHaveBeenLastCalledWith('seller-1', 2, 'search-account-1')
    expect(screen.getByRole('heading', { name: '评价记录' })).toBeTruthy()
    expect(screen.queryByText('本页包含商品评价与账号评价，商品信息仅在平台实际返回时展示')).toBeNull()
    expect(screen.getByText('评论商品')).toBeTruthy()
    expect(screen.getByText(/平台返回的商品标题/)).toBeTruthy()
    expect(screen.getByText('评价人：买家乙')).toBeTruthy()
    expect(screen.getByText('时间：2026-07-31')).toBeTruthy()
    expect(screen.getAllByText('评论正文')).toHaveLength(2)
    expect(screen.getByText('商品级评价内容')).toBeTruthy()
    expect(screen.queryByRole('button', { name: '查看评价详情' })).toBeNull()
  })

  it('keeps H5 fallback reviews account-scoped without inventing an item or detail action', async () => {
    peerMonitorApi.getPeerMonitorCandidateReviews.mockResolvedValue({
      source: 'seller_rate_fallback',
      reviews: [{
        id: 'rate-2',
        feedback: '平台公开的账号评价',
        raterNick: '买家乙',
        title: '不能作为商品标题展示',
        picUrl: 'https://img.example.test/not-an-item.jpg',
        itemPrice: '0.0',
      }],
      hasNext: false,
    })

    renderSellerHome()

    await screen.findByText('同行卖家甲')
    fireEvent.click(screen.getByRole('button', { name: '评价' }))

    await screen.findByText('账号评价')
    await waitFor(() => expect(peerMonitorApi.getPeerMonitorCandidateReviews).toHaveBeenCalledWith('seller-1', 1, 'search-account-1'))
    expect(screen.queryByText('平台未公开可归属商品时，不展示评论商品')).toBeNull()
    expect(screen.getByText('平台公开的账号评价')).toBeTruthy()
    expect(screen.queryByText('不能作为商品标题展示')).toBeNull()
    expect(screen.queryByText('评论商品')).toBeNull()
    expect(screen.queryByText('商品 · ¥0.0')).toBeNull()
    expect(screen.queryByRole('button', { name: '查看评价详情' })).toBeNull()
  })

  it('shows only provided public seller profile fields without repurposing existing values', async () => {
    renderSellerHome()

    await screen.findByText('同行卖家甲')
    expect(screen.queryByAltText('同行主页背景')).toBeNull()
    expect(screen.getByAltText('同行卖家甲头像')).toBeTruthy()
    expect(screen.getByText('闲小铺 L3')).toBeTruthy()
    expect(screen.getByText('杭州 · 浙江 · 西湖区')).toBeTruthy()
    expect(screen.getByText('个人简介')).toBeTruthy()
    expect(screen.getByText('最近到访 2小时前来过')).toBeTruthy()
    expect(screen.getByText('1234')).toBeTruthy()
    expect(screen.getByText('86')).toBeTruthy()
    expect(screen.getByText('24小时回复率')).toBeTruthy()
    expect(screen.getByText('平均回复间隔')).toBeTruthy()
    expect(screen.getByText('注册天数')).toBeTruthy()
    expect(screen.getByText('\u7d2f\u8ba1\u552e\u51fa')).toBeTruthy()
    expect(screen.getByText(/信用等级/)).toBeTruthy()
    expect(screen.getByText('实名认证')).toBeTruthy()
    expect(screen.getAllByText('实名认证')).toHaveLength(1)
    expect(screen.getByText('发货快')).toBeTruthy()
    expect(screen.getByText('专注二手数码，支持验货。')).toBeTruthy()
    expect(screen.getAllByText('支持当面验货与同城自提。')).toHaveLength(1)
    expect(screen.queryByText('店铺服务 / 信用')).toBeNull()
    expect(screen.queryByText('同行公开主页')).toBeNull()
  })

  it('keeps the seller profile as a light card without rendering a visual backdrop', async () => {
    peerMonitorApi.getPeerMonitorCandidate.mockResolvedValue({ candidate: { ...candidate, profileBackgroundUrl: undefined } })

    renderSellerHome()

    await screen.findByText('同行卖家甲')
    expect(screen.queryByAltText('同行主页背景')).toBeNull()
    expect(screen.getByAltText('同行卖家甲头像')).toBeTruthy()
  })

  it('uses the three ordered summary cards to switch same-page content without a review dialog', async () => {
    renderSellerHome()

    await screen.findByText('同行卖家甲')
    const cards = within(screen.getByTestId('peer-summary-cards')).getAllByRole('button')
    expect(cards.map(card => card.getAttribute('aria-label'))).toEqual(['宝贝数量', '动态', '评价'])
    expect(within(cards[0]).getByText('4')).toBeTruthy()
    expect(within(cards[1]).getByText('9')).toBeTruthy()
    expect(within(cards[2]).getByText('12')).toBeTruthy()
    expect(screen.getByText('当前公开宝贝')).toBeTruthy()

    fireEvent.click(cards[1])
    expect(screen.getByText('主页动态')).toBeTruthy()
    expect(screen.getByText('真实动态商品')).toBeTruthy()

    fireEvent.click(cards[2])
    await screen.findByText('账号评价')
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.queryByRole('button', { name: '关闭' })).toBeNull()
  })

  it.each([
    ['个人主页链接', 'seller'],
    ['商品链接', 'item'],
  ])('opens the seller homepage directly for an imported %s using inputKind', async (_label, inputKind) => {
    const link = `https://example.test/${inputKind}/seller-1`
    peerMonitorApi.getPeerMonitorCandidateFromLink.mockResolvedValue({ inputKind, candidate })
    const { container } = renderLinkImport(link)

    await screen.findByRole('heading', { name: '同行卖家甲' })
    expect(peerMonitorApi.getPeerMonitorCandidateFromLink).toHaveBeenCalledWith(link)
    expect(screen.queryByText('同行公开主页')).toBeNull()
    expect(screen.queryByText('搜索商品详情')).toBeNull()
    expect(screen.queryByRole('button', { name: '查看主页' })).toBeNull()
    expect(container.querySelector(`[data-import-kind="${inputKind}"]`)).toBeTruthy()
  })

  it('keeps keyword-search items on the existing product-detail then seller-home path', async () => {
    peerMonitorApi.getPeerMonitorCandidate.mockResolvedValue({ candidate: { ...candidate, title: '关键词搜索到的商品' } })
    renderSearchItem()

    await screen.findByText('搜索商品详情')
    expect(screen.getByText('关键词搜索到的商品')).toBeTruthy()
    expect(screen.getByRole('button', { name: '查看主页' })).toBeTruthy()
  })
})
