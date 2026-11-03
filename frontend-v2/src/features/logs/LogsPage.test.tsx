import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import LogsPage from './LogsPage'

const { apiMocks } = vi.hoisted(() => ({
  apiMocks: {
    getLogs: vi.fn(),
    getIncidents: vi.fn(),
    clearLogs: vi.fn(),
    getOperationLogs: vi.fn(),
    getOperationJobs: vi.fn(),
    getOperationJob: vi.fn(),
    getMessageDailyStats: vi.fn(),
  },
}))

vi.mock('../../api/index', () => apiMocks)

vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false, isDesktop: true }),
}))

vi.mock('../../hooks/useAuthScope', () => ({
  useAuthScope: () => ({
    authToken: 'token-a',
    captureScope: (key = '') => ({ token: 'token-a', key }),
    isScopeCurrent: () => true,
  }),
}))

vi.mock('../../store/authStore', () => ({
  useAuthStore: (selector: any) => selector({
    user: { username: '90000000003', role: 'admin' },
    _hydrated: true,
  }),
}))

describe('LogsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    apiMocks.getLogs.mockResolvedValue({ data: [] })
    apiMocks.getIncidents.mockResolvedValue({ data: [] })
    apiMocks.clearLogs.mockResolvedValue(undefined)
    apiMocks.getOperationLogs.mockResolvedValue({
      items: [
        {
          id: 1,
          status: 'success',
          action: 'publish',
          account_nick: '主账号',
          uid: 'u1',
          message: '商品发布成功',
          target_title: '测试商品',
          target_id: 'item-1',
          created_at: 1710000000,
        },
      ],
    })
    apiMocks.getOperationJobs.mockResolvedValue({ items: [] })
    apiMocks.getOperationJob.mockResolvedValue({ job: null, items: [] })
    apiMocks.getMessageDailyStats.mockResolvedValue({ items: [] })
  })

  it('loads and renders operation logs on desktop', async () => {
    render(
      <MemoryRouter>
        <LogsPage />
      </MemoryRouter>,
    )

    fireEvent.click(screen.getByRole('button', { name: '操作审计' }))

    expect(await screen.findByText('商品发布成功')).toBeTruthy()
    expect(screen.getByText('测试商品')).toBeTruthy()
    expect(apiMocks.getOperationLogs).toHaveBeenCalledWith({
      module: '',
      action: '',
      uid: '',
      status: '',
      keyword: '',
      limit: 200,
    })
  })
})
