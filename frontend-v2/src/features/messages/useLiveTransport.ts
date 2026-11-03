// @ts-nocheck
/**
 * useLiveTransport — 前端 WebSocket 推送通道
 *
 * 使用 WebSocket（/api/live/ws）实现前后端实时双向通信。
 * 对上层消费者暴露统一的 onEvent 回调。
 *
 * 支持上行指令：subscribe / focus / blur / ping / send / send_image
 * 支持指数退避重连：1s → 2s → 4s → ... → 30s，±30% 抖动
 */

import { buildBackendWsUrl } from '../../runtimeProfile'
import { getAuthToken } from '../../api/client'
import { emitActionLog } from '../../bootstrap/actionLogging'

const MAX_RECONNECT_DELAY = 30000
const MAX_RECONNECT_ATTEMPTS = 50
const PING_INTERVAL = 25000
const JITTER_FACTOR = 0.3
const COMMAND_TIMEOUT = 15000
const canReadLocalStorage =
  typeof localStorage !== 'undefined'
  && typeof localStorage.getItem === 'function'
const LIVE_DEBUG =
  import.meta.env.DEV
  && canReadLocalStorage
  && localStorage.getItem('xianyu_live_debug') === '1'

function devLog(...args) {
  if (LIVE_DEBUG) console.log(...args)
}

function devWarn(...args) {
  if (LIVE_DEBUG) console.warn(...args)
}

function devError(...args) {
  if (LIVE_DEBUG) console.error(...args)
}

/**
 * 计算退避延迟（ms），含抖动
 */
function backoffDelay(attempt) {
  const base = Math.min(MAX_RECONNECT_DELAY, 1000 * Math.pow(2, Math.min(attempt, 5)))
  const jitter = base * JITTER_FACTOR * (Math.random() * 2 - 1)
  return Math.max(500, Math.round(base + jitter))
}

/**
 * 解析 WebSocket 基地址
 */
async function resolveWsUrl() {
  const baseUrl = await buildBackendWsUrl('/api/live/ws')
  if (!baseUrl) return null
  return baseUrl
}

/**
 * 创建一个实时传输层实例
 *
 * @param {Object} options
 * @param {Function} options.onEvent   - 收到事件时的回调 (event: Object) => void
 * @param {Function} [options.onStateChange] - 连接状态变化 (state: string) => void
 * @returns {Object} transport 控制器
 */
export function createLiveTransport(options = {}) {
  const { onEvent, onStateChange, authToken = getAuthToken() } = options

  let ws = null
  let mode = 'disconnected' // 'ws' | 'disconnected'
  let reconnectAttempt = 0
  let reconnectTimer = null
  let pingTimer = null
  let closed = false
  let nextRequestId = 0
  let lastTransportLogKey = ''
  let lastTransportLogAt = 0
  const pendingCommands = new Map()

  function setState(newState) {
    mode = newState
    if (typeof onStateChange === 'function') {
      onStateChange(newState)
    }
  }

  function emitTransportLog(action, outcome, message, context = {}, error = null) {
    const now = Date.now()
    const key = `${action}:${outcome}:${context?.code || ''}:${context?.reason || ''}`
    if (key === lastTransportLogKey && now - lastTransportLogAt < 5000) return
    lastTransportLogKey = key
    lastTransportLogAt = now
    emitActionLog({
      module: 'messages-live-transport',
      action,
      outcome,
      message,
      error,
      context: {
        reconnectAttempt,
        pendingCommandCount: pendingCommands.size,
        ...context,
      },
      promoteToIncident: outcome === 'failed',
    })
  }

  function handleEvent(data) {
    if (typeof onEvent === 'function' && data && data.type) {
      onEvent(data)
    }
  }

  function rejectPendingCommands(error) {
    for (const pending of pendingCommands.values()) {
      clearTimeout(pending.timer)
      pending.reject(error)
    }
    pendingCommands.clear()
  }

  function settlePendingCommand(data) {
    const requestId = String(data?.requestId || '').trim()
    if (!requestId || !pendingCommands.has(requestId)) return false

    const pending = pendingCommands.get(requestId)
    pendingCommands.delete(requestId)
    clearTimeout(pending.timer)

    if (data.ok) {
      pending.resolve(data.response)
      return true
    }

    const detail = data?.response?.detail || data?.response?.ret?.[0] || data?.reason || 'WebSocket command failed'
    pending.reject(new Error(String(detail)))
    return true
  }

  // ── WebSocket ─────────────────────────────────────────────

  async function connectWs() {
    if (!authToken || getAuthToken() !== authToken) {
      setState('disconnected')
      return
    }
    const url = await resolveWsUrl().catch((error) => {
      devError('[live-ws] resolve URL failed', error)
      emitTransportLog('resolve_ws_url', 'failed', '消息 WebSocket 地址解析失败', {}, error)
      return null
    })
    if (!url) {
      devWarn('[live-ws] no URL, scheduling reconnect')
      emitTransportLog('resolve_ws_url', 'warn', '消息 WebSocket 地址为空，准备重连')
      scheduleReconnect()
      return
    }

    devLog('[live-ws] connecting to', url.replace(/token=[^&]+/, 'token=***'))
    try {
      ws = new WebSocket(url)
    } catch (e) {
      devError('[live-ws] WebSocket constructor error', e)
      emitTransportLog('connect_ws', 'failed', '消息 WebSocket 创建失败', {}, e)
      scheduleReconnect()
      return
    }

    ws.onopen = () => {
      if (getAuthToken() !== authToken) {
        ws?.close()
        return
      }
      reconnectAttempt = 0
      devLog('[live-ws] connected')
      setState('ws')
      emitTransportLog('connect_ws', 'success', '消息 WebSocket 已连接')
      startPing()
    }

    ws.onmessage = (event) => {
      if (getAuthToken() !== authToken) return
      try {
        const data = JSON.parse(event.data)
        if (data.type === 'system.disconnect') {
          devWarn('[live-ws] system.disconnect', data.reason)
          emitTransportLog('system_disconnect', 'failed', '消息 WebSocket 被服务端断开', {
            reason: data.reason || '',
          })
          rejectPendingCommands(new Error(data.reason || 'WebSocket disconnected'))
          scheduleReconnect()
          return
        }
        if (data.type === 'send.result' || data.type === 'send_image.result') {
          if (settlePendingCommand(data)) {
            return
          }
        }
        devLog('[live-ws] event', data.type, data.uid || '', data.text?.slice(0, 20) || '')
        handleEvent(data)
      } catch (e) {
        emitTransportLog('parse_ws_event', 'warn', '消息 WebSocket 事件解析失败', {}, e)
      }
    }

    ws.onerror = (e) => {
      devError('[live-ws] error', e)
      emitTransportLog('ws_error', 'warn', '消息 WebSocket 发生错误')
    }

    ws.onclose = (e) => {
      devWarn('[live-ws] closed code=' + e.code, e.reason)
      stopPing()
      ws = null
      emitTransportLog('ws_closed', 'warn', '消息 WebSocket 已断开', {
        code: e.code,
        reason: e.reason || '',
      })
      rejectPendingCommands(new Error('WebSocket disconnected'))
      if (!closed) {
        scheduleReconnect()
      }
    }
  }

  function startPing() {
    stopPing()
    pingTimer = setInterval(() => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ cmd: 'ping' }))
      }
    }, PING_INTERVAL)
  }

  function stopPing() {
    if (pingTimer) {
      clearInterval(pingTimer)
      pingTimer = null
    }
  }

  // ── 重连 ──────────────────────────────────────────────────

  function scheduleReconnect() {
    if (closed || reconnectAttempt >= MAX_RECONNECT_ATTEMPTS) {
      setState('disconnected')
      if (!closed) {
        emitTransportLog('reconnect_exhausted', 'failed', '消息 WebSocket 重连次数已耗尽', {
          maxReconnectAttempts: MAX_RECONNECT_ATTEMPTS,
        })
      }
      return
    }

    const delay = backoffDelay(reconnectAttempt)
    reconnectAttempt++
    setState('reconnecting')

    clearTimeout(reconnectTimer)
    reconnectTimer = setTimeout(() => {
      if (!closed) {
        connectWs()
      }
    }, delay)
  }

  // ── 上行指令 ──────────────────────────────────────────────

  function sendCommand(cmd) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(cmd))
      return true
    }
    return false
  }

  function sendCommandAwait(cmd, resultType) {
    if (!(ws && ws.readyState === WebSocket.OPEN)) {
      return Promise.reject(new Error('实时连接不可用'))
    }

    nextRequestId += 1
    const requestId = `${resultType}:${Date.now()}:${nextRequestId}`

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pendingCommands.delete(requestId)
        reject(new Error('实时请求超时'))
      }, COMMAND_TIMEOUT)

      pendingCommands.set(requestId, { resolve, reject, timer })
      ws.send(JSON.stringify({
        ...cmd,
        requestId,
      }))
    })
  }

  function subscribe(uids) {
    devLog('[live-ws] subscribe uids=', uids?.length, 'connected=', !!(ws && ws.readyState === WebSocket.OPEN))
    return sendCommand({ cmd: 'subscribe', uids })
  }

  function focus(uid, sessionId, sessionType = 1) {
    return sendCommand({ cmd: 'focus', uid, sessionId, sessionType })
  }

  function blur(uid = '', sessionId = '') {
    return sendCommand({ cmd: 'blur', uid, sessionId })
  }

  function sendText(uid, sessionId, toUid, content) {
    return sendCommandAwait({ cmd: 'send', uid, sessionId, toUid, content }, 'send')
  }

  function sendImage(uid, sessionId, toUid, imageUrl, width, height) {
    return sendCommandAwait({
      cmd: 'send_image', uid, sessionId, toUid, imageUrl,
      width: width || 0, height: height || 0,
    }, 'send_image')
  }

  // ── 生命周期 ──────────────────────────────────────────────

  function connect() {
    closed = false
    reconnectAttempt = 0
    connectWs()
  }

  function close() {
    closed = true
    clearTimeout(reconnectTimer)
    stopPing()
    if (ws) {
      ws.close()
      ws = null
    }
    rejectPendingCommands(new Error('实时连接已关闭'))
    setState('disconnected')
  }

  return {
    connect,
    close,
    subscribe,
    focus,
    blur,
    sendText,
    sendImage,
    sendCommand,
    get mode() { return mode },
    get isConnected() { return mode === 'ws' },
  }
}
