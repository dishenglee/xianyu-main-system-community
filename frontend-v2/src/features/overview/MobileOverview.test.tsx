// @ts-nocheck
import React from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import MobileOverview from './MobileOverview'
import { useUserUnreadStore } from '../../store/userUnreadStore'

const { accountStoreMock, apiMocks, clientMocks } = vi.hoisted(() => ({
  accountStoreMock: {
    accounts: [{ uid: 'u1', nick: '账号甲', status: 'active' }],
    loadAccounts: vi.fn().mockResolvedValue(undefined),
  },
  apiMocks: {
    getDashboardStats: vi.fn(),
  },
  clientMocks: {
    getAuthToken: vi.fn(() => 'token-a'),
  },
}))

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../api/index', () => apiMocks)

vi.mock('../../api/client', () => clientMocks)

function renderPage() {
  return render(
    <MemoryRouter>
      <MobileOverview />
    </MemoryRouter>,
  )
}

describe('MobileOverview unread metric', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useUserUnreadStore.getState().reset()
    accountStoreMock.accounts = [{ uid: 'u1', nick: '账号甲', status: 'active' }]
    accountStoreMock.loadAccounts.mockResolvedValue(undefined)
    apiMocks.getDashboardStats.mockResolvedValue({
      activeAccounts: 1,
      restrictedAccounts: 0,
      inactiveAccounts: 0,
      sessionErrorAccounts: 0,
      unreadMessages: 18,
      unreadSessions: 3,
      overdueReplySessions: 2,
      todayRevenue: 0,
      todayOrders: 0,
    })
    clientMocks.getAuthToken.mockReturnValue('token-a')
  })

  it('uses the live message unread store instead of stale dashboard unread counts', async () => {
    renderPage()

    expect(await screen.findByText('未读消息')).toBeTruthy()
    await waitFor(() => expect(apiMocks.getDashboardStats).toHaveBeenCalled())
    expect(screen.queryByText('待处理未读')).toBeNull()
    expect(screen.getByText('未读消息').previousElementSibling?.textContent).toBe('0')
  })

  it('matches the unread badge count from the live store', async () => {
    useUserUnreadStore.getState().setUnread('u1', 's1', 1, 2)
    useUserUnreadStore.getState().setUnread('u1', 's2', 1, 1)

    renderPage()

    expect(await screen.findByText('未读消息')).toBeTruthy()
    expect(screen.getByText('未读消息').previousElementSibling?.textContent).toBe('3')
  })

  it('exposes account import and export from the mobile workbench', async () => {
    renderPage()

    const bundleEntry = await screen.findByRole('link', { name: /账号导入导出/ })
    expect(bundleEntry.getAttribute('href')).toBe('/bundle')
  })
})
