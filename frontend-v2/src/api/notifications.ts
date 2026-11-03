import { request } from './client'

// ==================== Notification channels ====================

export const getNotificationChannels = () =>
  request<unknown[]>('GET', '/api/notification-channels')

export const createNotificationChannel = (
  payload: { name: string; type: string; enabled: boolean; config: Record<string, unknown> },
) => request<{ status: string; channel: Record<string, unknown> }>(
  'POST', '/api/notification-channels', payload,
)

export const updateNotificationChannel = (
  id: number,
  payload: { name?: string; type?: string; enabled?: boolean; config?: Record<string, unknown> },
) => request<{ status: string; channel: Record<string, unknown> }>(
  'PUT', `/api/notification-channels/${id}`, payload,
)

export const deleteNotificationChannel = (id: number) =>
  request<{ status: string }>('DELETE', `/api/notification-channels/${id}`)

export const testNotificationChannel = (id: number, message = '') =>
  request<unknown>('POST', `/api/notification-channels/${id}/test`, { message })

export const patchNotificationChannelRuntime = (
  id: number, runtime: Record<string, unknown>,
) => request<unknown>('POST', `/api/notification-channels/${id}/runtime`, { runtime })

export const controlNotificationChannel = (
  id: number, senderId: string, text: string,
) => request<{ response?: string; delivery_error?: string }>(
  'POST', `/api/notification-channels/${id}/control`, { sender_id: senderId, text },
)

export const getMessageNotifications = () =>
  request<Record<string, Array<{ channel_id: number; enabled: boolean; binding_id?: number }>>>(
    'GET', '/api/message-notifications',
  )

export type MessageNotificationSettings = {
  platformMessagesEnabled: boolean
}

export type FeishuChannelMember = {
  id: number
  sender_id: string
  sender_name: string
  role: string
  joined_via: string
  invited_by_sender_id?: string
  invited_by_sender_name?: string
}

export type FeishuCollaboratorInvite = {
  id: number
  code: string
  status: string
  expires_at: number
  created_by_sender_id?: string
  created_by_sender_name?: string
}

export const getMessageNotificationSettings = () =>
  request<MessageNotificationSettings>('GET', '/api/message-notifications/settings')

export const updateMessageNotificationSettings = (platformMessagesEnabled: boolean) =>
  request<unknown>('PUT', '/api/message-notifications/settings', {
    platformMessagesEnabled,
  })

export const setMessageNotification = (uid: string, channelId: number, enabled: boolean) =>
  request<unknown>('POST', `/api/message-notifications/${uid}`, {
    channel_id: channelId, enabled,
  })

export const getFeishuChannelMembers = (channelId: number) =>
  request<{
    status: string
    members: FeishuChannelMember[]
    active_invite: FeishuCollaboratorInvite | null
  }>('GET', `/api/notification-channels/${channelId}/feishu/members`)

export const createFeishuChannelInvite = (channelId: number) =>
  request<{
    status: string
    invite: FeishuCollaboratorInvite
  }>('POST', `/api/notification-channels/${channelId}/feishu/invites`, {})

export const deleteFeishuChannelMember = (channelId: number, senderId: string) =>
  request<{ status: string }>(
    'DELETE',
    `/api/notification-channels/${channelId}/feishu/members/${encodeURIComponent(senderId)}`,
  )

// ==================== iLink (WeChat) QR-code login ====================

export type WechatQrStartResp = {
  qrcode: string
  qrcode_img_content: string
  host: string
}

export type WechatQrPollResp = {
  status: string
  bot_token?: string
  ilink_bot_id?: string
  ilink_user_id?: string
  baseurl?: string
  redirect_host?: string
  // Server-side auto-provisioned channel — present only on status==="confirmed".
  channel?: Record<string, unknown>
}

export const startWechatQrLogin = () =>
  request<WechatQrStartResp>(
    'POST', '/api/notification-channels/wechat/qr-start', {},
  )

export const pollWechatQrLogin = (payload: {
  qrcode: string
  host: string
  verify_code?: string
}) => request<WechatQrPollResp>(
  'POST', '/api/notification-channels/wechat/qr-poll', payload,
)

// ==================== Feishu / Lark QR-code registration ====================

export type FeishuQrStartResp = {
  device_code: string
  qr_url: string
  qr_image: string
  expires_at_ms: number
  poll_interval_ms: number
  domain: string
  required_scopes?: string[]
  publish_required?: boolean
}

export type FeishuQrPollResp = {
  status: string
  device_code: string
  domain: string
  qr_url?: string
  qr_image?: string
  required_scopes?: string[]
  missing_scopes?: string[]
  publish_required?: boolean
  scope_hint?: string
  channel?: Record<string, unknown>
}

export const startFeishuQrRegistration = () =>
  request<FeishuQrStartResp>(
    'POST', '/api/notification-channels/feishu/qr-start', {},
  )

export const pollFeishuQrRegistration = (payload: {
  device_code: string
  domain?: string
}) => request<FeishuQrPollResp>(
  'POST', '/api/notification-channels/feishu/qr-poll', payload,
)
