// @ts-nocheck
import React from 'react'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { accountStoreMock, apiMocks, notifyMock } = vi.hoisted(() => ({
  accountStoreMock: {
    accounts: [],
    loading: false,
    loadAccounts: vi.fn().mockResolvedValue([]),
  },
  apiMocks: {
    getItems: vi.fn(),
    getOfflineReply: vi.fn(),
    syncOfflineReply: vi.fn(),
  },
  notifyMock: vi.fn(),
}))

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../api', () => apiMocks)
vi.mock('../../components/TopBar', () => ({
  TopBar: ({ title }) => <div>{title}</div>,
}))
vi.mock('../../components/HubTabs', () => ({
  HubTabs: () => <div>automation-tabs</div>,
  HUB_PRESETS: { automation: [] },
}))
vi.mock('../../components/Notify', () => ({
  useSafeNotify: () => notifyMock,
}))
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({
    isMobile: false,
    isTablet: false,
    isDesktop: true,
    device: 'desktop',
  }),
}))
vi.mock('../../hooks/useAuthScope', () => ({
  useAuthScope: () => ({
    authToken: 'token-a',
    captureScope: (key = '') => ({
      token: 'token-a',
      key,
    }),
    isScopeCurrent: (scope, key = scope.key) => scope.key === key,
  }),
}))

import OfflineReplyPage from './OfflineReplyPage'

function renderPage() {
  return render(
    <MemoryRouter>
      <OfflineReplyPage />
    </MemoryRouter>,
  )
}

describe('OfflineReplyPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    accountStoreMock.accounts = []
    accountStoreMock.loading = false
    accountStoreMock.loadAccounts.mockResolvedValue([])
    apiMocks.getItems.mockResolvedValue({ data: { itemList: [], hasMore: false } })
    apiMocks.getOfflineReply.mockResolvedValue({ status: 'ok', name: '不在线回复' })
    apiMocks.syncOfflineReply.mockResolvedValue({ status: 'ok', name: '不在线回复' })
  })

  it('loads active accounts directly and selects accounts from the shared account picker', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-1', nick: '账号一', status: 'active' },
      { uid: 'uid-2', nick: '账号二', status: 'active' },
    ]

    renderPage()

    expect(await screen.findByText('账号一')).toBeTruthy()
    expect(accountStoreMock.loadAccounts).toHaveBeenCalledWith({ force: true })
    expect(apiMocks.getItems).not.toHaveBeenCalled()
    expect(apiMocks.getOfflineReply).not.toHaveBeenCalled()

    await waitFor(() => {
      expect(screen.getByLabelText('账号一').checked).toBe(true)
      expect(screen.getByLabelText('账号二').checked).toBe(true)
    })

    fireEvent.click(screen.getByLabelText('账号二'))

    await waitFor(() => {
      expect(screen.getByLabelText('账号二').checked).toBe(false)
    })
  })

  it('saves global offline text without an item id', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-1', nick: '账号一', status: 'active' },
    ]
    renderPage()

    expect(await screen.findByText('账号一')).toBeTruthy()
    fireEvent.change(screen.getByPlaceholderText('账号级不在线回复话术'), {
      target: { value: '亲，我现在不在，稍后回复' },
    })
    fireEvent.click(screen.getByRole('button', { name: '保存所选账号话术' }))

    await waitFor(() => {
      expect(apiMocks.syncOfflineReply).toHaveBeenCalledWith('uid-1', {
        enableOfflineReply: true,
        offlineReplyText: '亲，我现在不在，稍后回复',
      })
    })
  })

  it('applies account-level offline reply to every active account', async () => {
    accountStoreMock.accounts = [
      { uid: 'uid-1', nick: '账号一', status: 'active' },
      { uid: 'uid-2', nick: '账号二', status: 'active' },
      { uid: 'uid-3', nick: '停用账号', status: 'disabled' },
    ]
    renderPage()

    expect(await screen.findByText('账号一')).toBeTruthy()
    fireEvent.change(screen.getByPlaceholderText('账号级不在线回复话术'), {
      target: { value: '统一不在线话术' },
    })
    fireEvent.click(screen.getAllByRole('button', { name: '应用到全部账号' })[0])

    await waitFor(() => {
      expect(apiMocks.syncOfflineReply).toHaveBeenCalledWith('uid-1', {
        enableOfflineReply: true,
        offlineReplyText: '统一不在线话术',
      })
      expect(apiMocks.syncOfflineReply).toHaveBeenCalledWith('uid-2', {
        enableOfflineReply: true,
        offlineReplyText: '统一不在线话术',
      })
      expect(apiMocks.syncOfflineReply).not.toHaveBeenCalledWith('uid-3', expect.anything())
      expect(apiMocks.syncOfflineReply).not.toHaveBeenCalledWith('uid-1', expect.objectContaining({ itemId: expect.any(String) }))
      expect(apiMocks.syncOfflineReply).not.toHaveBeenCalledWith('uid-2', expect.objectContaining({ itemId: expect.any(String) }))
    })
    await waitFor(() => {
      expect(screen.getAllByText('全局不在线回复').length).toBeGreaterThan(1)
      expect(screen.getByText('账号：2/2')).toBeTruthy()
      expect(screen.getByText('成功：2')).toBeTruthy()
    })
  })
})
