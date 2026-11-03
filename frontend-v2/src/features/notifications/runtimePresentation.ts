// @ts-nocheck

function asObject(value) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {}
}

function readText(value) {
  if (value === undefined || value === null) return ''
  const text = String(value).trim()
  return text
}

function readNumber(value) {
  const num = Number(value)
  return Number.isFinite(num) && num > 0 ? num : 0
}

function toIsoOrEmpty(value) {
  const timestamp = readNumber(value)
  if (!timestamp) return ''
  const millis = timestamp > 1e12 ? timestamp : timestamp * 1000
  const date = new Date(millis)
  if (Number.isNaN(date.getTime())) return ''
  return date.toISOString()
}

function formatTimestamp(value) {
  const iso = toIsoOrEmpty(value)
  if (!iso) return ''
  return iso.replace('T', ' ').replace('.000Z', 'Z')
}

function statusMeta(status) {
  if (status === 'healthy' || status === 'listening') {
    return { label: status === 'listening' ? '监听中' : '正常', dot: 'var(--success)', tone: 'success' }
  }
  if (status === 'error' || status === 'failed') {
    return { label: status === 'failed' ? '失败' : '异常', dot: 'var(--danger)', tone: 'danger' }
  }
  if (status === 'sending') {
    return { label: '发送中', dot: 'var(--primary)', tone: 'info' }
  }
  if (status === 'starting') {
    return { label: '启动中', dot: 'var(--warning)', tone: 'warning' }
  }
  if (status === 'idle' || !status) {
    return { label: '待激活', dot: 'var(--text-muted)', tone: 'neutral' }
  }
  return { label: status, dot: 'var(--text-muted)', tone: 'neutral' }
}

function buildFacts(runtime, runtimeJson) {
  const facts = []

  const lastError = readText(runtime.last_error || runtimeJson.last_error)
  const lastAttemptAt = formatTimestamp(runtime.last_attempt_at || runtimeJson.last_attempt_at)
  const lastSuccessAt = formatTimestamp(runtime.last_success_at || runtimeJson.last_success_at)
  const listenerStatus = readText(runtimeJson.listener_status)
  const callbackSource = readText(runtimeJson.last_callback_source)
  const callbackMethod = readText(runtimeJson.last_callback_method)
  const callbackAt = formatTimestamp(runtimeJson.last_callback_at || runtimeJson.updated_at)
  const workerRestartStatus = readText(runtimeJson.worker_restart_status)
  const workerRestartReason = readText(runtimeJson.worker_restart_reason)
  const workerRestartSource = readText(runtimeJson.worker_restart_source)
  const workerChannelType = readText(runtimeJson.worker_channel_type)

  if (listenerStatus) facts.push({ key: 'listener', label: '监听器', value: listenerStatus })
  if (lastAttemptAt) facts.push({ key: 'attempt', label: '最近发送', value: lastAttemptAt })
  if (lastSuccessAt) facts.push({ key: 'success', label: '最近成功', value: lastSuccessAt })
  if (callbackAt || callbackSource || callbackMethod) {
    facts.push({
      key: 'callback',
      label: '最近回调',
      value: [callbackMethod, callbackSource, callbackAt].filter(Boolean).join(' · '),
    })
  }
  if (workerRestartStatus || workerRestartReason || workerRestartSource || workerChannelType) {
    facts.push({
      key: 'worker_restart',
      label: 'Worker 重启',
      value: [workerRestartStatus, workerRestartSource, workerChannelType].filter(Boolean).join(' · '),
      detail: workerRestartReason,
    })
  }
  if (lastError) facts.push({ key: 'error', label: '最近错误', value: lastError, tone: 'danger' })

  return facts
}

export function presentNotificationRuntime(channel) {
  const runtime = asObject(channel?.runtime)
  const runtimeJson = asObject(runtime.runtime_json)
  const status = readText(runtimeJson.listener_status) || readText(runtime.status) || 'idle'
  const meta = statusMeta(status)

  return {
    status,
    label: meta.label,
    dot: meta.dot,
    tone: meta.tone,
    lastError: readText(runtime.last_error || runtimeJson.last_error),
    lastAttemptAt: formatTimestamp(runtime.last_attempt_at || runtimeJson.last_attempt_at),
    lastSuccessAt: formatTimestamp(runtime.last_success_at || runtimeJson.last_success_at),
    callback: {
      source: readText(runtimeJson.last_callback_source),
      method: readText(runtimeJson.last_callback_method),
      payload: asObject(runtimeJson.last_callback_payload),
      echoStr: readText(runtimeJson.last_callback_echostr),
      at: formatTimestamp(runtimeJson.last_callback_at || runtimeJson.updated_at),
    },
    workerRestart: {
      status: readText(runtimeJson.worker_restart_status),
      reason: readText(runtimeJson.worker_restart_reason),
      source: readText(runtimeJson.worker_restart_source),
      channelType: readText(runtimeJson.worker_channel_type),
      updatedAt: formatTimestamp(runtimeJson.updated_at || runtime.updated_at),
    },
    listenerStatus: readText(runtimeJson.listener_status),
    facts: buildFacts(runtime, runtimeJson),
  }
}
