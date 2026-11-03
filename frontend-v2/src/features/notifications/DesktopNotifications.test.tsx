// @ts-nocheck
import React from 'react'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { apiMocks, accountStoreMock, notifyMock, clientMocks } = vi.hoisted(() => ({
  apiMocks: {
    getNotificationChannels: vi.fn(),
    createNotificationChannel: vi.fn(),
    updateNotificationChannel: vi.fn(),
    deleteNotificationChannel: vi.fn(),
    testNotificationChannel: vi.fn(),
    getFeishuChannelMembers: vi.fn(),
    createFeishuChannelInvite: vi.fn(),
    deleteFeishuChannelMember: vi.fn(),
    getMessageNotifications: vi.fn(),
    setMessageNotification: vi.fn(),
    getMessageNotificationSettings: vi.fn(),
    updateMessageNotificationSettings: vi.fn(),
  },
  accountStoreMock: {
    accounts: [{ uid: 'u-1', nick: '账号一', status: 'active' }],
    loadAccounts: vi.fn().mockResolvedValue(undefined),
  },
  notifyMock: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    show: vi.fn(),
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
vi.mock('../../components/Notify', () => ({
  useNotify: () => notifyMock,
}))
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: false, isTablet: false, isDesktop: true, device: 'desktop' }),
}))
vi.mock('../../utils/copy', () => ({
  copyText: vi.fn((_value, onSuccess) => onSuccess?.()),
}))
vi.mock('./WechatQrLoginModal', () => ({
  WechatQrLoginModal: ({ open, onChannelCreated }) => open ? (
    <div data-testid="mock-wechat-qr-modal">
      <button
        type="button"
        onClick={() => onChannelCreated?.({ id: 88, name: '个人微信', type: 'openclaw_weixin' })}
      >
        完成微信扫码
      </button>
    </div>
  ) : null,
}))
vi.mock('./FeishuQrRegistrationModal', () => ({
  FeishuQrRegistrationModal: ({ open, onChannelCreated }) => open ? (
    <div data-testid="mock-feishu-qr-modal">
      <button
        type="button"
        onClick={() => onChannelCreated?.({ id: 99, name: '飞书扫码', type: 'feishu' })}
      >
        完成飞书扫码
      </button>
    </div>
  ) : null,
}))

import DesktopNotifications from './DesktopNotifications'
import { AppDialogProvider } from '../../components/AppDialog'

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
      <AppDialogProvider>
        <DesktopNotifications />
      </AppDialogProvider>
    </MemoryRouter>,
  )
}

describe('DesktopNotifications', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clientMocks.getAuthToken.mockReturnValue('token-a')
    accountStoreMock.accounts = [{ uid: 'u-1', nick: '账号一', status: 'active' }]
    accountStoreMock.loadAccounts.mockResolvedValue(accountStoreMock.accounts)
    apiMocks.getNotificationChannels.mockResolvedValue([
      {
        id: 7,
        name: '企业微信回调',
        type: 'wechat',
        enabled: true,
        config: {
          bot_token: 'token',
          corp_id: 'corp-id',
          secret: 'secret',
          agent_id: 'agent-1',
          token: 'verify-token',
          encoding_aes_key: 'x'.repeat(43),
        },
        runtime: {
          status: 'error',
          last_error: 'driver failed',
          last_attempt_at: 1710000000,
          last_success_at: 1710000100,
          runtime_json: {
            listener_status: 'listening',
            last_callback_source: 'wechat.callback',
            last_callback_method: 'POST',
            last_callback_at: 1710000200,
            worker_restart_status: 'spawned',
            worker_restart_source: 'wechat.manager',
            worker_restart_reason: 'manual restart',
            worker_channel_type: 'wechat',
          },
        },
      },
    ])
    apiMocks.getMessageNotifications.mockResolvedValue({
      'u-1': [{ channel_id: 7, enabled: true }],
    })
    apiMocks.getMessageNotificationSettings.mockResolvedValue({
      platformMessagesEnabled: true,
    })
    apiMocks.updateMessageNotificationSettings.mockResolvedValue({
      platformMessagesEnabled: false,
    })
    apiMocks.getFeishuChannelMembers.mockResolvedValue({
      status: 'ok',
      members: [],
      active_invite: null,
    })
    apiMocks.createFeishuChannelInvite.mockResolvedValue({
      status: 'ok',
      invite: {
        code: 'INVITE-001',
        status: 'active',
        expires_at: 1797000000,
        created_by_sender_name: '主用户',
      },
    })
    apiMocks.deleteFeishuChannelMember.mockResolvedValue({ status: 'ok' })
    apiMocks.testNotificationChannel.mockResolvedValue({ ok: true })
  })

  it('renders and saves the global platform-message toggle', async () => {
    renderPage()

    expect(await screen.findByText('通知消息 / 互动消息')).toBeTruthy()

    const toggle = screen.getByRole('checkbox', { name: /通知消息 \/ 互动消息/i })
    expect((toggle as HTMLInputElement).checked).toBe(true)

    fireEvent.click(toggle)

    await waitFor(() => {
      expect(apiMocks.updateMessageNotificationSettings).toHaveBeenCalledWith(false)
    })
    expect(notifyMock.success).toHaveBeenCalledWith('已关闭通知消息和互动消息')
  })

  it('renders shared runtime diagnostics on the list row and editor panel', async () => {
    renderPage()

    expect(await screen.findByText('企业微信回调')).toBeTruthy()
    expect(screen.getByText('监听中')).toBeTruthy()
    expect(screen.getByText(/最近错误：/)).toBeTruthy()

    fireEvent.click(screen.getByText('企业微信回调'))

    expect(await screen.findByTestId('desktop-runtime-panel')).toBeTruthy()
    expect(screen.getByText('运行时诊断')).toBeTruthy()
    expect(screen.getAllByText('wechat.callback', { exact: false }).length).toBeGreaterThanOrEqual(2)
    expect(screen.getByText('manual restart')).toBeTruthy()
  })

  it('renders callback URL and bound-account controls on the real desktop page', async () => {
    window.history.replaceState({}, '', '/notifications')

    renderPage()

    expect(await screen.findByText('企业微信回调')).toBeTruthy()
    fireEvent.click(screen.getByText('企业微信回调'))

    expect(await screen.findByText('Callback 地址（配置到渠道后台）')).toBeTruthy()
    expect(screen.getAllByRole('button', { name: /复制/i }).length).toBeGreaterThanOrEqual(2)
    expect(screen.getAllByText(/\/api\/notification-hooks\/wechat\/7/).length).toBeGreaterThanOrEqual(2)
    expect(screen.getByRole('button', { name: /绑定账号/i })).toBeTruthy()
  })

  it('keeps test send wiring intact from the card action', async () => {
    renderPage()

    await screen.findByText('企业微信回调')
    fireEvent.click(screen.getByRole('button', { name: /测试/i }))

    await waitFor(() => {
      expect(apiMocks.testNotificationChannel).toHaveBeenCalledWith(7, '')
    })
    expect(notifyMock.success).toHaveBeenCalledWith('测试已发送')
  })

  it('opens QR-based channels from the unified add button instead of standalone toolbar buttons', async () => {
    renderPage()

    await screen.findByText('企业微信回调')
    expect(apiMocks.getNotificationChannels).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('button', { name: /扫码添加微信/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /扫码添加飞书/i })).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /^新增$/i }))

    expect(await screen.findByText('选择渠道类型')).toBeTruthy()
    expect(screen.getByRole('button', { name: /个人微信/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /^飞书$/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /企业微信/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /钉钉/i })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /^飞书$/i }))
    expect(await screen.findByTestId('mock-feishu-qr-modal')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /完成飞书扫码/i }))

    await waitFor(() => {
      expect(notifyMock.success).toHaveBeenCalledWith('飞书渠道已创建')
      expect(apiMocks.getNotificationChannels).toHaveBeenCalledTimes(2)
    })
  })

  it('opens personal WeChat QR from the unified add button', async () => {
    renderPage()

    await screen.findByText('企业微信回调')
    fireEvent.click(screen.getByRole('button', { name: /^新增$/i }))
    fireEvent.click(await screen.findByRole('button', { name: /个人微信/i }))

    expect(await screen.findByTestId('mock-wechat-qr-modal')).toBeTruthy()
    expect(screen.queryByText('新增渠道')).toBeNull()
  })

  it('opens the created Feishu channel so it can be tested immediately', async () => {
    apiMocks.getNotificationChannels
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        { id: 99, name: '飞书扫码', type: 'feishu', enabled: true, config: {}, runtime: {} },
      ])
    apiMocks.getMessageNotifications.mockResolvedValue({})

    renderPage()

    await waitFor(() => expect(apiMocks.getNotificationChannels).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: /^新增$/i }))
    fireEvent.click(await screen.findByRole('button', { name: /^飞书$/i }))
    fireEvent.click(await screen.findByRole('button', { name: /完成飞书扫码/i }))

    expect(await screen.findByText('编辑渠道')).toBeTruthy()
    const testButtons = screen.getAllByRole('button', { name: /^测试$/i })
    fireEvent.click(testButtons[testButtons.length - 1])

    await waitFor(() => {
      expect(apiMocks.testNotificationChannel).toHaveBeenCalledWith(99, '')
    })
  })

  it('shows only Feishu test and rebind actions instead of advanced app config', async () => {
    apiMocks.getNotificationChannels.mockResolvedValue([
      {
        id: 99,
        name: '飞书扫码',
        type: 'feishu',
        enabled: true,
        config: {
          app_id: 'cli-secret',
          app_secret: 'sec-secret',
          allowed_operator_ids: ['ou-owner'],
        },
        runtime: { status: 'healthy', runtime_json: {} },
      },
    ])
    apiMocks.getMessageNotifications.mockResolvedValue({})

    renderPage()

    expect(await screen.findByText('飞书扫码')).toBeTruthy()
    fireEvent.click(screen.getByText('飞书扫码'))

    expect(await screen.findByText('编辑渠道')).toBeTruthy()
    expect(screen.queryByText('App ID')).toBeNull()
    expect(screen.queryByText('App Secret')).toBeNull()
    expect(screen.queryByRole('button', { name: /^保存$/i })).toBeNull()
    expect(screen.getAllByRole('button', { name: /^测试$/i }).length).toBeGreaterThanOrEqual(2)
    fireEvent.click(screen.getByRole('button', { name: /重新扫码绑定/i }))

    expect(await screen.findByTestId('mock-feishu-qr-modal')).toBeTruthy()
  })

  it('loads Feishu collaborators and allows creating an invite from the compact panel', async () => {
    apiMocks.getNotificationChannels.mockResolvedValue([
      {
        id: 99,
        name: '飞书扫码',
        type: 'feishu',
        enabled: true,
        config: {},
        runtime: { status: 'healthy', runtime_json: {} },
      },
    ])
    apiMocks.getMessageNotifications.mockResolvedValue({})
    apiMocks.getFeishuChannelMembers.mockResolvedValue({
      status: 'ok',
      members: [
        {
          id: 1,
          sender_id: 'ou_primary',
          sender_name: '主用户',
          role: 'primary',
          joined_via: 'qr_owner',
        },
        {
          id: 2,
          sender_id: 'ou_collab',
          sender_name: '客服小王',
          role: 'collaborator',
          joined_via: 'invite_code',
          invited_by_sender_name: '主用户',
        },
      ],
      active_invite: null,
    })

    renderPage()

    expect(await screen.findByText('飞书扫码')).toBeTruthy()
    fireEvent.click(screen.getByText('飞书扫码'))

    expect(await screen.findByText('协作者管理')).toBeTruthy()
    expect(apiMocks.getFeishuChannelMembers).toHaveBeenCalledWith(99)
    expect(screen.getAllByText('主用户').length).toBeGreaterThanOrEqual(1)
    expect(screen.getByText('客服小王')).toBeTruthy()
    expect(screen.getByText('主用户 · 扫码创建')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /生成邀请码/i }))

    await waitFor(() => {
      expect(apiMocks.createFeishuChannelInvite).toHaveBeenCalledWith(99)
    })
    expect(await screen.findByText('INVITE-001')).toBeTruthy()
  })

  it('saves account bindings as a draft with select-all and cancel controls', async () => {
    accountStoreMock.accounts = [
      { uid: 'u-1', nick: '账号一', status: 'active' },
      { uid: 'u-2', nick: '账号二', status: 'active' },
    ]
    accountStoreMock.loadAccounts.mockResolvedValue(accountStoreMock.accounts)
    apiMocks.getMessageNotifications.mockResolvedValue({
      'u-1': [{ channel_id: 7, enabled: true }],
      'u-2': [],
    })

    renderPage()

    expect(await screen.findByText('企业微信回调')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /绑定账号/i }))
    fireEvent.click(screen.getByRole('button', { name: /全选/i }))
    fireEvent.click(screen.getByRole('button', { name: /^取消$/i }))

    expect(apiMocks.setMessageNotification).not.toHaveBeenCalled()
    expect(screen.queryByText(/绑定账号 — 企业微信回调/)).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /绑定账号/i }))
    fireEvent.click(screen.getByRole('button', { name: /全选/i }))
    fireEvent.click(screen.getByRole('button', { name: /^保存$/i }))

    await waitFor(() => {
      expect(apiMocks.setMessageNotification).toHaveBeenCalledTimes(1)
      expect(apiMocks.setMessageNotification).toHaveBeenCalledWith('u-2', 7, true)
    })
  })

  it('does not render stale channels when the auth token changes before reload returns', async () => {
    const channels = deferred<unknown[]>()
    const bindings = deferred<Record<string, unknown[]>>()
    apiMocks.getNotificationChannels.mockReturnValue(channels.promise)
    apiMocks.getMessageNotifications.mockReturnValue(bindings.promise)

    renderPage()

    await waitFor(() => expect(apiMocks.getNotificationChannels).toHaveBeenCalled())
    clientMocks.getAuthToken.mockReturnValue('token-b')

    await act(async () => {
      channels.resolve([
        { id: 31, name: '旧用户渠道', type: 'wechat', enabled: true, config: {}, runtime: {} },
      ])
      bindings.resolve({ 'u-1': [{ channel_id: 31, enabled: true }] })
      await Promise.resolve()
    })

    expect(screen.queryByText('旧用户渠道')).toBeNull()
  })
})
