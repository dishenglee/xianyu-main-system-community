import React from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

const toggleSelectMock = vi.fn()
const setShowBatchListMock = vi.fn()

vi.mock('./hooks/useSearchPageState', () => ({
  SOURCE_MODES: [{ label: '自动', value: 'auto' }],
  useSearchPageState: () => ({
    activeAccounts: [{ uid: 'u1' }],
    accountsLoading: false,
    account: { uid: 'u1' },
    keyword: '键盘',
    setKeyword: vi.fn(),
    doSearch: vi.fn(),
    loading: false,
    loadingMore: false,
    error: null,
    searched: true,
    results: [{
      itemId: 'ITEM1',
      title: '测试搜索商品',
      price: '88.00',
      picUrl: 'https://example.com/item.jpg',
      sellerNick: '卖家A',
      area: '长沙',
      userAvatar: '',
      wantNum: 2,
      wantText: '2人想要',
      freeShip: true,
      sellerBadgeUrl: '',
      userExtraTags: [],
      infoTags: [],
      statTags: [],
      serviceTags: [],
    }],
    page: 1,
    hasMore: false,
    sourceMode: 'auto',
    changeSourceMode: vi.fn(),
    sortField: '',
    sortValue: '',
    priceDir: 'asc',
    handleSort: vi.fn(),
    dynFilters: [],
    activeFilters: new Map(),
    toggleFilter: vi.fn(),
    clearFilters: vi.fn(),
    selectedIds: new Set(),
    selectedItems: [],
    toggleSelect: toggleSelectMock,
    clearSelection: vi.fn(),
    showBatchList: false,
    setShowBatchList: setShowBatchListMock,
    nearby: null,
    setNearby: vi.fn(),
  }),
}))

vi.mock('./BatchListModal', () => ({
  BatchListModal: () => <div>batch modal</div>,
}))

vi.mock('./RegionFilterControl', () => ({
  RegionFilterControl: () => <div>region control</div>,
}))

import MobileSearch from './MobileSearch'

beforeAll(() => {
  class MockIntersectionObserver {
    observe() {}
    disconnect() {}
  }
  vi.stubGlobal('IntersectionObserver', MockIntersectionObserver)
})

describe('MobileSearch', () => {
  beforeEach(() => {
    toggleSelectMock.mockReset()
    setShowBatchListMock.mockReset()
  })

  it('shows visible mobile actions for filtering and batch publish', () => {
    render(<MobileSearch />)

    expect(screen.getAllByText('筛选').length).toBeGreaterThan(0)
    expect(screen.getByText('点商品卡片或右上角勾选，可批量一键上架')).toBeTruthy()
    expect(screen.queryByRole('button', { name: '一键上架' })).toBeNull()
  })

  it('selects the item when tapping the card body on mobile', () => {
    render(<MobileSearch />)

    fireEvent.click(screen.getByText('测试搜索商品'))

    expect(toggleSelectMock).toHaveBeenCalledWith('ITEM1')
  })
})
