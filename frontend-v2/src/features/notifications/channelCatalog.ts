// @ts-nocheck
const BOOLEAN_FIELDS = new Set([
  'enabled',
])

const JSON_FIELDS = new Set([])

function textField(key, label, placeholder = '', options = {}) {
  return {
    key,
    label,
    input: 'text',
    placeholder,
    ...options,
  }
}

function passwordField(key, label, placeholder = '', options = {}) {
  return {
    key,
    label,
    input: 'password',
    placeholder,
    ...options,
  }
}

export const CHANNEL_DEFINITIONS = [
  {
    type: 'wechat',
    label: '企业微信',
    accent: '#07C160',
    description: '支持企业微信应用消息和群机器人地址，两者至少配置一种。',
    fields: [
      textField('corp_id', 'Corp ID'),
      passwordField('secret', 'Secret'),
      textField('agent_id', 'Agent ID'),
      textField('token', 'Callback Token', '企业微信回调校验 token'),
      passwordField('encoding_aes_key', 'EncodingAESKey', '43 位回调解密密钥'),
      textField('receive_userid', 'Receive User ID', '回调可自动写入'),
      textField('touser', 'ToUser'),
      textField('toparty', 'ToParty'),
      textField('totag', 'ToTag'),
      textField('api_base_url', 'API Base URL', '默认 https://qyapi.weixin.qq.com'),
      textField('robot_url', '群机器人地址', '没有应用配置时可只用群机器人地址'),
    ],
  },
  {
    type: 'dingtalk',
    label: '钉钉',
    accent: '#1677FF',
    description: '支持钉钉机器人地址，callback/runtime 已接入。',
    fields: [
      textField('robot_url', '机器人地址'),
      passwordField('secret', '签名 Secret', '机器人加签 secret'),
      textField('app_key', 'App Key', '后续 Stream / 应用模式预留'),
      passwordField('app_secret', 'App Secret'),
      textField('target_staff_id', 'Target Staff ID', '可留空，等待 callback 自动绑定'),
      textField('target_open_conversation_id', 'Target Conversation ID', '群聊可自动绑定'),
    ],
  },
  {
    type: 'feishu',
    label: '飞书',
    accent: '#3370FF',
    description: '扫码授权后自动创建飞书机器人并接入消息通知。',
    fields: [
      textField('app_id', 'App ID'),
      passwordField('app_secret', 'App Secret'),
      textField('receive_id', 'Receive ID', '可留空，等待 callback 自动绑定'),
      textField('receive_id_type', 'Receive ID Type', '默认 chat_id'),
      textField('allowed_operator_ids', '允许绑定的飞书用户 ID', '扫码接入会自动写入；多个 ID 用逗号分隔'),
      textField('verification_token', 'Verification Token', '回调校验可选'),
      passwordField('encrypt_key', 'Encrypt Key', '加密事件可选'),
      textField('open_api_base_url', 'Open API Base URL', '默认 https://open.feishu.cn'),
    ],
  },
  {
    type: 'openclaw_weixin',
    label: '个人微信',
    accent: '#22C55E',
    description: '扫码登录后自动创建个人微信通知渠道。',
    fields: [
      textField('base_url', 'Base URL', '扫码登录后自动填入'),
      passwordField('bot_token', 'Bot Token', '扫码登录后自动填入'),
      textField('ilink_user_id', 'iLink User ID', '扫码用户 ID，扫码登录后自动填入'),
      textField('to_user_id', 'Target User ID (可选)', '默认与 ilink_user_id 相同（自发自）'),
      textField('bot_agent', 'Bot Agent (可选)', '默认 xianyu-proxy/1.0'),
      textField('context_token', 'Context Token (可选)', '可留空 — 由 callback 自动写入'),
    ],
  },
]

const CHANNEL_MAP = new Map(CHANNEL_DEFINITIONS.map((definition) => [definition.type, definition]))
const CHANNEL_ALIASES = {
  lark: 'feishu',
}

const ADDABLE_CHANNEL_TYPES = ['feishu', 'dingtalk', 'wechat', 'openclaw_weixin']
const CONFIG_CREATE_TYPES = new Set(['wechat', 'dingtalk'])

export const ADDABLE_CHANNEL_DEFINITIONS = ADDABLE_CHANNEL_TYPES
  .map((type) => CHANNEL_MAP.get(type))
  .filter(Boolean)

export const CREATABLE_CHANNEL_DEFINITIONS = CHANNEL_DEFINITIONS.filter(
  (definition) => CONFIG_CREATE_TYPES.has(definition.type),
)

export function getChannelDefinition(type) {
  const normalized = String(type || '').trim()
  return CHANNEL_MAP.get(CHANNEL_ALIASES[normalized] || normalized) || CHANNEL_MAP.get('feishu')
}

export function buildChannelConfigDraft(type, config = {}, runtime = {}) {
  const definition = getChannelDefinition(type)
  const runtimeJson = runtime && typeof runtime === 'object' && runtime.runtime_json && typeof runtime.runtime_json === 'object'
    ? runtime.runtime_json
    : {}
  const merged = { ...(config || {}), ...runtimeJson }
  const draft = {}

  for (const field of definition.fields) {
    const value = merged[field.key]
    if (BOOLEAN_FIELDS.has(field.key)) {
      draft[field.key] = Boolean(value)
      continue
    }
    if (JSON_FIELDS.has(field.key)) {
      draft[field.key] = value && typeof value === 'object'
        ? JSON.stringify(value, null, 2)
        : String(value || '')
      continue
    }
    draft[field.key] = value === undefined || value === null ? '' : String(value)
  }

  return draft
}

export function serializeChannelConfigDraft(type, draft = {}) {
  const definition = getChannelDefinition(type)
  const config = {}

  for (const field of definition.fields) {
    const rawValue = draft[field.key]
    if (BOOLEAN_FIELDS.has(field.key)) {
      if (rawValue === undefined || rawValue === null || rawValue === '') continue
      config[field.key] = Boolean(rawValue)
      continue
    }

    if (JSON_FIELDS.has(field.key)) {
      const text = String(rawValue || '').trim()
      if (!text) continue
      config[field.key] = JSON.parse(text)
      continue
    }

    const text = String(rawValue || '').trim()
    if (!text) continue
    config[field.key] = text
  }

  return config
}
