// @ts-nocheck
import React from 'react'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { apiMocks, accountStoreMock, notifyMock } = vi.hoisted(() => ({
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
}))

vi.mock('../../hooks/useAccounts', () => ({
  useAccounts: () => accountStoreMock,
}))

vi.mock('../../api/index', () => apiMocks)
vi.mock('../../components/Notify', () => ({
  useNotify: () => notifyMock,
}))
vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ isMobile: true, isTablet: false, isDesktop: false, device: 'mobile' }),
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

import MobileNotifications from './MobileNotifications'
import { AppDialogProvider } from '../../components/AppDialog'

function renderPage() {
  return render(
    <MemoryRouter>
      <AppDialogProvider>
        <MobileNotifications />
      </AppDialogProvider>
    </MemoryRouter>,
  )
}

describe('MobileNotifications', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    apiMocks.getNotificationChannels.mockResolvedValue([
      {
        id: 11,
        name: '企业微信回调',
        type: 'wechat',
        enabled: true,
        config: { corp_id: 'corp-1' },
        runtime: {
          status: 'error',
          last_error: 'callback verification failed',
          last_attempt_at: 1710000300,
          last_success_at: 1710000200,
          runtime_json: {
            last_callback_source: 'wechat.callback',
            last_callback_method: 'GET',
            last_callback_at: 1710000400,
            last_callback_echostr: 'echo-123',
            worker_restart_status: 'error',
            worker_restart_source: 'wechat.manager',
            worker_restart_reason: 'missing bot_token',
            worker_channel_type: 'wechat',
          },
        },
      },
    ])
    apiMocks.getMessageNotifications.mockResolvedValue({
      'u-1': [{ channel_id: 11, enabled: true }],
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

  it('renders and saves the mobile global platform-message toggle', async () => {
    renderPage()

    expect(await screen.findByText('通知消息 / 互动消息')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /关闭通知消息 \/ 互动消息/i }))

    await waitFor(() => {
      expect(apiMocks.updateMessageNotificationSettings).toHaveBeenCalledWith(false)
    })
    expect(notifyMock.success).toHaveBeenCalledWith('已关闭通知消息和互动消息')
  })

  it('shows runtime diagnostics on cards and inside the mobile editor sheet', async () => {
    renderPage()

    expect(await screen.findByText('企业微信回调')).toBeTruthy()
    expect(screen.getByText(/最近回调/)).toBeTruthy()

    fireEvent.click(screen.getByText('企业微信回调'))

    expect(await screen.findByTestId('mobile-runtime-panel')).toBeTruthy()
    expect(screen.getByText('运行时诊断')).toBeTruthy()
    expect(screen.getAllByText('wechat.callback', { exact: false }).length).toBeGreaterThanOrEqual(2)
    expect(screen.getByText('missing bot_token')).toBeTruthy()
  })

  it('shows callback URL and bound-account context inside the mobile editor sheet', async () => {
    window.history.replaceState({}, '', '/notifications')

    renderPage()

    expect(await screen.findByText('企业微信回调')).toBeTruthy()
    fireEvent.click(screen.getByText('企业微信回调'))

    expect(await screen.findByText('Callback 地址（配置到渠道后台）')).toBeTruthy()
    expect(screen.getByText('账号绑定')).toBeTruthy()
    expect(screen.getByText('账号一')).toBeTruthy()
    expect(screen.getAllByText(/\/api\/notification-hooks\/wechat\/11/).length).toBeGreaterThanOrEqual(2)
  })

  it('keeps mobile test-send action wired to the backend API', async () => {
    renderPage()

    await screen.findByText('企业微信回调')
    fireEvent.click(screen.getByRole('button', { name: /发送测试通知/i }))

    await waitFor(() => {
      expect(apiMocks.testNotificationChannel).toHaveBeenCalledWith(11, '')
    })
    expect(notifyMock.success).toHaveBeenCalledWith('测试已发送')
  })

  it('opens Feishu QR registration from the unified add sheet', async () => {
    renderPage()

    await screen.findByText('企业微信回调')
    expect(apiMocks.getNotificationChannels).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('heading', { name: '扫码添加' })).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /新建通知渠道/i }))
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

  it('opens personal WeChat QR from the unified add sheet', async () => {
    renderPage()

    await screen.findByText('企业微信回调')
    fireEvent.click(screen.getByRole('button', { name: /新建通知渠道/i }))
    fireEvent.click(await screen.findByRole('button', { name: /个人微信/i }))

    expect(await screen.findByTestId('mock-wechat-qr-modal')).toBeTruthy()
    expect(screen.queryByText('新建 个人微信')).toBeNull()
  })

  it('opens the created Feishu channel so the test action is available', async () => {
    apiMocks.getNotificationChannels
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        { id: 99, name: '飞书扫码', type: 'feishu', enabled: true, config: {}, runtime: {} },
      ])
    apiMocks.getMessageNotifications.mockResolvedValue({})

    renderPage()

    await waitFor(() => expect(apiMocks.getNotificationChannels).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: /新建通知渠道/i }))
    fireEvent.click(screen.getByRole('button', { name: /^飞书$/i }))
    fireEvent.click(await screen.findByRole('button', { name: /完成飞书扫码/i }))

    expect(await screen.findByText('编辑通知渠道')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /^测试$/i }))

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

    expect(await screen.findByText('编辑通知渠道')).toBeTruthy()
    expect(screen.queryByText('App ID')).toBeNull()
    expect(screen.queryByText('App Secret')).toBeNull()
    expect(screen.queryByRole('button', { name: /^保存$/i })).toBeNull()
    expect(screen.getByRole('button', { name: /^测试$/i })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /重新扫码绑定/i }))

    expect(await screen.findByTestId('mock-feishu-qr-modal')).toBeTruthy()
  })

  it('shows Feishu collaborators and invite actions inside the mobile compact panel', async () => {
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

    fireEvent.click(screen.getByRole('button', { name: /生成邀请码/i }))

    await waitFor(() => {
      expect(apiMocks.createFeishuChannelInvite).toHaveBeenCalledWith(99)
    })
    expect(await screen.findByText('INVITE-001')).toBeTruthy()
  })

  it('keeps account binding edits local until the mobile sheet is saved', async () => {
    accountStoreMock.accounts = [
      { uid: 'u-1', nick: '账号一', status: 'active' },
      { uid: 'u-2', nick: '账号二', status: 'active' },
    ]
    accountStoreMock.loadAccounts.mockResolvedValue(accountStoreMock.accounts)
    apiMocks.getMessageNotifications.mockResolvedValue({
      'u-1': [{ channel_id: 11, enabled: true }],
      'u-2': [],
    })

    renderPage()

    expect(await screen.findByText('企业微信回调')).toBeTruthy()
    fireEvent.click(screen.getByText('企业微信回调'))
    fireEvent.click(screen.getByRole('button', { name: /全选/i }))
    fireEvent.click(screen.getByRole('button', { name: /^取消$/i }))

    expect(apiMocks.setMessageNotification).not.toHaveBeenCalled()
    expect(screen.queryByText('编辑通知渠道')).toBeNull()

    fireEvent.click(screen.getByText('企业微信回调'))
    fireEvent.click(screen.getByRole('button', { name: /全选/i }))
    fireEvent.click(screen.getByRole('button', { name: /^保存$/i }))

    await waitFor(() => {
      expect(apiMocks.setMessageNotification).toHaveBeenCalledTimes(1)
      expect(apiMocks.setMessageNotification).toHaveBeenCalledWith('u-2', 11, true)
    })
  })
})
