// @ts-nocheck
import React from 'react'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { apiMocks } = vi.hoisted(() => ({
  apiMocks: {
    startFeishuQrRegistration: vi.fn(),
    pollFeishuQrRegistration: vi.fn(),
  },
}))

vi.mock('../../api/notifications', () => apiMocks)
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false, isDesktop: true, device: 'desktop' }),
}))

import { FeishuQrRegistrationModal } from './FeishuQrRegistrationModal'

async function flushPromises() {
  await Promise.resolve()
  await Promise.resolve()
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('FeishuQrRegistrationModal', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
    apiMocks.startFeishuQrRegistration.mockResolvedValue({
      device_code: 'device-1',
      domain: 'feishu',
      qr_image: 'data:image/png;base64,qr',
      qr_url: 'https://example.test/feishu-qr',
      poll_interval_ms: 1200,
      required_scopes: ['im:message.group_msg'],
      publish_required: true,
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('keeps polling across repeated pending responses until confirmed', async () => {
    const channel = { id: 42, name: '飞书扫码' }
    const onChannelCreated = vi.fn()
    apiMocks.pollFeishuQrRegistration
      .mockResolvedValueOnce({ status: 'pending', device_code: 'device-1', domain: 'feishu' })
      .mockResolvedValueOnce({ status: 'pending', device_code: 'device-1', domain: 'feishu' })
      .mockResolvedValueOnce({ status: 'confirmed', device_code: 'device-1', domain: 'feishu', channel })

    render(
      <FeishuQrRegistrationModal
        open
        onClose={vi.fn()}
        onChannelCreated={onChannelCreated}
      />,
    )

    await act(async () => {
      await flushPromises()
    })
    expect(screen.getByText('等待扫码绑定')).toBeTruthy()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200)
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200)
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200)
    })

    expect(apiMocks.pollFeishuQrRegistration).toHaveBeenCalledTimes(3)
    expect(onChannelCreated).toHaveBeenCalledWith(channel)
  })

  it('renders required scope guidance for non-mention group replies', async () => {
    render(
      <FeishuQrRegistrationModal
        open
        onClose={vi.fn()}
      />,
    )

    await act(async () => {
      await flushPromises()
    })

    expect(screen.getByText('第一步')).toBeTruthy()
    expect(screen.getByText('扫码绑定')).toBeTruthy()
    expect(screen.getByText('第二步')).toBeTruthy()
    expect(screen.getByText('扫码授权')).toBeTruthy()
  })

  it('switches to scope setup qr when registration confirms but app is missing scope', async () => {
    apiMocks.pollFeishuQrRegistration.mockResolvedValueOnce({
      status: 'scope_setup_required',
      device_code: 'device-1',
      domain: 'feishu',
      qr_image: 'data:image/png;base64,scope-qr',
      qr_url: 'https://open.feishu.cn/page/launcher?clientID=cli_x&addons=abc',
      missing_scopes: ['im:message.group_msg'],
      scope_hint: '请再扫一次这个补权二维码',
      publish_required: false,
    })

    render(
      <FeishuQrRegistrationModal
        open
        onClose={vi.fn()}
      />,
    )

    await act(async () => {
      await flushPromises()
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200)
      await flushPromises()
    })

    expect(screen.getByText('等待扫码授权')).toBeTruthy()
    expect(screen.getByText(/当前还缺这些权限/i)).toBeTruthy()
    expect(screen.getByText(/请再扫一次这个补权二维码/i)).toBeTruthy()
    expect(screen.getByText(/第二步授权说明/i)).toBeTruthy()
  })

  it('ignores a stale poll response after refreshing the QR registration', async () => {
    const stalePoll = deferred<Record<string, unknown>>()
    const onChannelCreated = vi.fn()
    apiMocks.startFeishuQrRegistration
      .mockResolvedValueOnce({
        device_code: 'device-1',
        domain: 'feishu',
        qr_image: 'data:image/png;base64,qr-1',
        qr_url: 'https://example.test/feishu-qr-1',
        poll_interval_ms: 1200,
      })
      .mockResolvedValueOnce({
        device_code: 'device-2',
        domain: 'feishu',
        qr_image: 'data:image/png;base64,qr-2',
        qr_url: 'https://example.test/feishu-qr-2',
        poll_interval_ms: 1200,
      })
    apiMocks.pollFeishuQrRegistration.mockReturnValueOnce(stalePoll.promise)

    render(
      <FeishuQrRegistrationModal
        open
        onClose={vi.fn()}
        onChannelCreated={onChannelCreated}
      />,
    )

    await act(async () => {
      await flushPromises()
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1200)
    })

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /刷新二维码/i }))
      await flushPromises()
    })

    await act(async () => {
      stalePoll.resolve({
        status: 'confirmed',
        device_code: 'device-1',
        domain: 'feishu',
        channel: { id: 7, name: '旧飞书' },
      })
      await flushPromises()
    })

    expect(onChannelCreated).not.toHaveBeenCalled()
    expect(screen.getByText('等待扫码绑定')).toBeTruthy()
  })
})
