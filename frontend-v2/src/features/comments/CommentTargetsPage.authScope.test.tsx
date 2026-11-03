// @ts-nocheck
import React from 'react'
import { act, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { apiMocks, clientMocks, notifyMock } = vi.hoisted(() => ({
  apiMocks: {
    getCommentTargets: vi.fn(),
    batchAddCommentTargets: vi.fn(),
  },
  clientMocks: {
    getAuthToken: vi.fn(() => 'token-a'),
  },
  notifyMock: vi.fn(),
}))

vi.mock('../../api/index', () => apiMocks)
vi.mock('../../api/client', () => clientMocks)
vi.mock('../../components/Notify', () => ({
  useSafeNotify: () => notifyMock,
}))
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false, isDesktop: true, device: 'desktop' }),
}))

import CommentTargetsPage from './CommentTargetsPage'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function renderPage() {
  return render(
    <MemoryRouter>
      <CommentTargetsPage />
    </MemoryRouter>,
  )
}

describe('CommentTargetsPage auth scope', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clientMocks.getAuthToken.mockReturnValue('token-a')
    apiMocks.getCommentTargets.mockResolvedValue({ items: [], pagination: { page: 1, page_size: 50, total: 0 } })
  })

  it('drops stale target rows when the auth token changes before load returns', async () => {
    const request = deferred<unknown>()
    apiMocks.getCommentTargets.mockReturnValue(request.promise)

    renderPage()

    await waitFor(() => expect(apiMocks.getCommentTargets).toHaveBeenCalled())
    clientMocks.getAuthToken.mockReturnValue('token-b')

    await act(async () => {
      request.resolve({
        items: [{ item_id: 'old-item', title: '旧用户商品', status: 'pending' }],
        pagination: { page: 1, page_size: 50, total: 1 },
      })
      await Promise.resolve()
    })

    expect(screen.queryByText('旧用户商品')).toBeNull()
  })

  it('clears mounted target rows immediately when auth changes', async () => {
    apiMocks.getCommentTargets.mockResolvedValue({
      items: [{ item_id: 'old-item', title: '旧用户商品', status: 'pending' }],
      pagination: { page: 1, page_size: 50, total: 1 },
    })

    renderPage()

    expect(await screen.findByText('旧用户商品')).toBeTruthy()

    clientMocks.getAuthToken.mockReturnValue('token-b')
    act(() => {
      window.dispatchEvent(new Event('xy:authChanged'))
    })

    expect(screen.queryByText('旧用户商品')).toBeNull()
  })
})
