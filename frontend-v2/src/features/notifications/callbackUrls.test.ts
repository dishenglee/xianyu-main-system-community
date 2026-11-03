// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { buildCallbackUrls } from './callbackUrls'

it('buildCallbackUrls returns wechat verification and callback urls', () => {
  const urls = buildCallbackUrls('https://console.example.com', { id: 8, type: 'wechat' })

  assert.deepEqual(urls, [
    {
      label: '校验地址',
      method: 'GET',
      url: 'https://console.example.com/api/notification-hooks/wechat/8',
    },
    {
      label: '消息回调',
      method: 'POST',
      url: 'https://console.example.com/api/notification-hooks/wechat/8',
    },
  ])
})

it('buildCallbackUrls returns generic callback url for dingtalk', () => {
  const urls = buildCallbackUrls('https://console.example.com/', { id: 9, type: 'dingtalk' })

  assert.deepEqual(urls, [
    {
      label: '消息回调',
      method: 'POST',
      url: 'https://console.example.com/api/notification-hooks/dingtalk/9',
    },
  ])
})

it('buildCallbackUrls maps lark channels to the feishu callback route', () => {
  const urls = buildCallbackUrls('https://console.example.com/', { id: 12, type: 'lark' })

  assert.deepEqual(urls, [
    {
      label: '消息回调',
      method: 'POST',
      url: 'https://console.example.com/api/notification-hooks/feishu/12',
    },
  ])
})

it('buildCallbackUrls returns empty list for non-interactive channels', () => {
  assert.deepEqual(
    buildCallbackUrls('https://console.example.com', { id: 10, type: 'openclaw_weixin' }),
    [],
  )
  assert.deepEqual(
    buildCallbackUrls('https://console.example.com', { id: 11, type: 'weixin_personal' }),
    [],
  )
})
