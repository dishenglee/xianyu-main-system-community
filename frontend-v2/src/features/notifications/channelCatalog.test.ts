// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import {
  CHANNEL_DEFINITIONS,
  buildChannelConfigDraft,
  getChannelDefinition,
  serializeChannelConfigDraft,
} from './channelCatalog'

it('channel catalog exposes all supported notification channel types', () => {
  assert.deepEqual(
    CHANNEL_DEFINITIONS.map((definition) => definition.type),
    [
      'wechat',
      'dingtalk',
      'feishu',
      'openclaw_weixin',
    ],
  )
})

it('buildChannelConfigDraft populates known fields and keeps runtime override values', () => {
  const draft = buildChannelConfigDraft(
    'wechat',
    {
      corp_id: 'corp-001',
      secret: 'secret-001',
      touser: '@all',
    },
    {
      runtime_json: {
        receive_userid: 'user-001',
      },
    },
  )

  assert.equal(draft.corp_id, 'corp-001')
  assert.equal(draft.secret, 'secret-001')
  assert.equal(draft.touser, '@all')
  assert.equal(draft.receive_userid, 'user-001')
})

it('serializeChannelConfigDraft keeps retained channel keys', () => {
  const config = serializeChannelConfigDraft('dingtalk', {
    robot_url: 'https://oapi.dingtalk.example/robot/send',
    secret: 'signing-secret',
    app_key: 'app-key',
    app_secret: 'app-secret',
    target_staff_id: 'staff-001',
    target_open_conversation_id: '',
  })

  assert.deepEqual(config, {
    robot_url: 'https://oapi.dingtalk.example/robot/send',
    secret: 'signing-secret',
    app_key: 'app-key',
    app_secret: 'app-secret',
    target_staff_id: 'staff-001',
  })
})

it('serializeChannelConfigDraft omits empty values', () => {
  const config = serializeChannelConfigDraft('feishu', {
    app_id: 'cli_001',
    app_secret: '',
    receive_id: 'chat-001',
    robot_url: '',
  })

  assert.deepEqual(config, {
    app_id: 'cli_001',
    receive_id: 'chat-001',
  })
})

it('getChannelDefinition falls back to feishu for unknown types', () => {
  const definition = getChannelDefinition('unknown-type')

  assert.equal(definition.type, 'feishu')
  assert.match(definition.label, /飞书/)
})

it('channel catalog only includes supported notification channel types', () => {
  const supportedTypes = new Set(CHANNEL_DEFINITIONS.map((definition) => definition.type))

  for (const type of ['wechat', 'dingtalk', 'feishu', 'openclaw_weixin']) {
    assert.equal(supportedTypes.has(type), true, `${type} should remain in the catalog`)
  }

  for (const type of ['webhook', 'generic_webhook', 'email', 'bark', 'telegram', 'tg', 'weixin_personal']) {
    assert.equal(supportedTypes.has(type), false, `${type} should not exist in the catalog`)
  }
})

it('getChannelDefinition maps lark to feishu definition', () => {
  const definition = getChannelDefinition('lark')

  assert.equal(definition.type, 'feishu')
  assert.match(definition.label, /飞书/)
})
