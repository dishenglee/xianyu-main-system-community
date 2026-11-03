// @ts-nocheck
const CALLBACK_TYPES = new Set(['wechat', 'dingtalk', 'feishu', 'lark'])

export function buildCallbackUrls(origin, channel) {
  const id = Number(channel?.id || 0)
  const type = String(channel?.type || '').trim()
  if (!id || !CALLBACK_TYPES.has(type)) {
    return []
  }

  const baseOrigin = String(origin || '').replace(/\/+$/, '')
  const normalizedType = type === 'lark' ? 'feishu' : type
  const callbackUrl = `${baseOrigin}/api/notification-hooks/${normalizedType}/${id}`
  if (normalizedType === 'wechat') {
    return [
      { label: '校验地址', method: 'GET', url: callbackUrl },
      { label: '消息回调', method: 'POST', url: callbackUrl },
    ]
  }
  return [{ label: '消息回调', method: 'POST', url: callbackUrl }]
}
