import type { RequestOptions } from './types'
import { isTauriRuntime } from '../tauriRuntime'

let cachedBaseURL: Promise<string> | null = null
let clientLogDelivery: Promise<void> = Promise.resolve()

type ErrorPayload = {
  detail?: string
  reason?: string
}

export type ClientLogLevel = 'info' | 'warn' | 'error'

export type ClientLogEvent = {
  level?: ClientLogLevel
  logger?: 'frontend.api' | 'frontend.console' | 'frontend.runtime' | 'frontend.action' | string
  module?: string
  action?: string
  route?: string
  errorCode?: string
  message: string
  detail?: string
  httpStatus?: number
  retryable?: boolean
  promoteToIncident?: boolean
  context?: Record<string, unknown>
}

function isBackendStartingError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error || '')
  return /backend not ready within \d+s/i.test(message)
}

async function resolveTauriBackendPort(): Promise<number> {
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    return await invoke<number>('get_backend_port')
  } catch (error) {
    if (!isBackendStartingError(error)) throw error
    return await invoke<number>('get_backend_port')
  }
}

function resolveBaseURL(): Promise<string> {
  return (async () => {
    if (isTauriRuntime()) {
      const port = await resolveTauriBackendPort()
      return `http://127.0.0.1:${port}`
    }
    return ''
  })()
}

export function getBaseURL(): Promise<string> {
  if (cachedBaseURL) return cachedBaseURL
  cachedBaseURL = resolveBaseURL().catch((error) => {
    cachedBaseURL = null
    throw error
  })
  return cachedBaseURL
}

// Invalidate the cached port. Called when a fetch fails with a transport-level
// error (e.g. Tauri dev restarted and allocated a new ephemeral backend port).
// The next request will re-invoke `get_backend_port`.
function invalidateBaseURL() {
  cachedBaseURL = null
}

export function getAuthToken(): string {
  return 'local-app'
}

function buildHeaders(options: RequestOptions): Record<string, string> {
  return {
    'Content-Type': 'application/json',
    ...(options.headers ?? {}),
  }
}

function buildInit(method: string, body: unknown, options: RequestOptions): RequestInit {
  const init: RequestInit = {
    method,
    headers: buildHeaders(options),
    signal: options.signal,
  }
  if (body !== undefined) {
    init.body = JSON.stringify(body)
  }
  return init
}

function createRequestSignal(options: RequestOptions) {
  if (!options.signal && !(options.timeoutMs && options.timeoutMs > 0)) {
    return {
      signal: options.signal,
      didTimeout: () => false,
      cleanup: () => {},
    }
  }

  const controller = new AbortController()
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  let timedOut = false
  let onAbort: (() => void) | null = null

  if (options.signal) {
    onAbort = () => controller.abort(options.signal?.reason)
    if (options.signal.aborted) {
      onAbort()
    } else {
      options.signal.addEventListener('abort', onAbort, { once: true })
    }
  }

  if (options.timeoutMs && options.timeoutMs > 0) {
    timeoutId = setTimeout(() => {
      timedOut = true
      controller.abort(new DOMException('Request timed out', 'AbortError'))
    }, options.timeoutMs)
  }

  return {
    signal: controller.signal,
    didTimeout: () => timedOut,
    cleanup: () => {
      if (timeoutId) clearTimeout(timeoutId)
      if (options.signal && onAbort) {
        options.signal.removeEventListener('abort', onAbort)
      }
    },
  }
}

async function sendWithBaseURLRetry(url: string, init: RequestInit): Promise<Response> {
  const baseURL = await getBaseURL()
  try {
    return await fetch(baseURL + url, init)
  } catch (err) {
    if (init.signal?.aborted) throw err
    // Transport-level failure — most common cause in dev is Tauri restarted
    // and the cached ephemeral port is now dead. Invalidate and retry once
    // with a fresh port.
    invalidateBaseURL()
    const freshBaseURL = await getBaseURL()
    if (freshBaseURL !== baseURL) {
      return await fetch(freshBaseURL + url, init)
    }
    throw err
  }
}

function isClientTelemetryEndpoint(url: string): boolean {
  return url.startsWith('/api/logs/client-events')
}

function shouldSkipClientTelemetry(url: string): boolean {
  return isClientTelemetryEndpoint(url)
}

function isMutationMethod(method: string): boolean {
  return ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method.trim().toUpperCase())
}

function normalizeClientLogString(value: string | undefined, fallback = ''): string {
  const source = String(value ?? '').trim() || fallback
  let text = source
  for (const marker of ['Bearer ', 'bearer ', 'authorization: ', 'cookie=', 'Cookie: ', 'token=']) {
    const index = text.indexOf(marker)
    if (index < 0) continue
    const prefixEnd = index + marker.length
    const tail = text.slice(prefixEnd)
    const endOffset = tail.search(/[\s;",]/)
    const replacementEnd = endOffset >= 0 ? prefixEnd + endOffset : text.length
    text = `${text.slice(0, prefixEnd)}[redacted]${text.slice(replacementEnd)}`
  }
  if (text.length > 512) {
    return `${text.slice(0, 512)}...`
  }
  return text
}

function isSensitiveContextKey(key: string): boolean {
  return [
    'token',
    'authorization',
    'cookie',
    'password',
    'secret',
    'content',
    'text',
    'payload',
    'rawcontent',
    'bodyhtml',
    'itemtitle',
  ].some((needle) => key.toLowerCase().includes(needle))
}

function sanitizeClientContext(value: unknown, depth = 0): unknown {
  if (depth > 4) return '[truncated]'
  if (value === null || value === undefined) return null
  if (typeof value === 'string') return normalizeClientLogString(value)
  if (typeof value === 'number' || typeof value === 'boolean') return value
  if (Array.isArray(value)) return value.slice(0, 20).map((item) => sanitizeClientContext(item, depth + 1))
  if (typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>).slice(0, 40)
    return Object.fromEntries(
      entries.map(([key, next]) => [
        key,
        isSensitiveContextKey(key) ? '[redacted]' : sanitizeClientContext(next, depth + 1),
      ]),
    )
  }
  return normalizeClientLogString(String(value))
}

function summarizeRequestBody(body: unknown): Record<string, unknown> {
  if (body === undefined) {
    return { hasBody: false }
  }
  if (body === null) {
    return { hasBody: true, bodyType: 'null' }
  }
  if (Array.isArray(body)) {
    return { hasBody: true, bodyType: 'array', length: body.length }
  }
  if (typeof body === 'object') {
    return {
      hasBody: true,
      bodyType: 'object',
      keys: Object.keys(body as Record<string, unknown>).slice(0, 20),
    }
  }
  return {
    hasBody: true,
    bodyType: typeof body,
  }
}

function buildRequestContext(
  method: string,
  url: string,
  body: unknown,
  options: RequestOptions,
  extra: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    method: method.trim().toUpperCase(),
    url,
    timeoutMs: options.timeoutMs ?? null,
    pathname: typeof window !== 'undefined' ? window.location.pathname : '',
    body: summarizeRequestBody(body),
    ...extra,
  }
}

function isUrlLikeString(value: string): boolean {
  const trimmed = value.trim()
  return /^https?:\/\//i.test(trimmed) || /^\/\//.test(trimmed)
}

function keyLooksLikeAsset(key: string): boolean {
  return /(avatar|logo|pic|image|cover|thumb)/i.test(key)
}

function normalizeAssetUrl(value: string): string {
  const trimmed = value.trim()
  if (!trimmed) return ''
  if (trimmed.startsWith('//')) {
    return `https:${trimmed}`
  }
  try {
    const parsed = new URL(trimmed)
    if (parsed.protocol === 'http:' && !['localhost', '127.0.0.1', '::1'].includes(parsed.hostname)) {
      parsed.protocol = 'https:'
      return parsed.toString()
    }
  } catch {
    return trimmed
  }
  return trimmed
}

function normalizeResponsePayload<T = unknown>(value: T, assetPath = false): T {
  if (typeof value === 'string') {
    if (!assetPath || !isUrlLikeString(value)) return value
    return normalizeAssetUrl(value) as T
  }
  if (Array.isArray(value)) {
    let next: unknown[] | null = null
    for (let i = 0; i < value.length; i += 1) {
      const normalized = normalizeResponsePayload(value[i], assetPath)
      if (next) {
        next.push(normalized)
      } else if (normalized !== value[i]) {
        next = value.slice(0, i)
        next.push(normalized)
      }
    }
    return (next ?? value) as T
  }
  if (value && typeof value === 'object') {
    const source = value as Record<string, unknown>
    let nextObject: Record<string, unknown> | null = null
    for (const [key, nextValue] of Object.entries(source)) {
      const normalized = normalizeResponsePayload(nextValue, assetPath || keyLooksLikeAsset(key))
      if (nextObject) {
        nextObject[key] = normalized
      } else if (normalized !== nextValue) {
        nextObject = { ...source, [key]: normalized }
      }
    }
    return (nextObject ?? value) as T
  }
  return value
}

function queueClientLogDelivery(task: () => Promise<void>) {
  clientLogDelivery = clientLogDelivery
    .catch(() => {})
    .then(task)
    .catch(() => {})
}

export function emitClientLog(event: ClientLogEvent): void {
  queueClientLogDelivery(async () => {
    const payload = {
      level: normalizeClientLogString(event.level || 'error', 'error'),
      logger: normalizeClientLogString(event.logger || 'frontend.runtime', 'frontend.runtime'),
      module: normalizeClientLogString(event.module || 'frontend', 'frontend'),
      action: normalizeClientLogString(event.action || 'client_event', 'client_event'),
      route: normalizeClientLogString(event.route || '', ''),
      errorCode: normalizeClientLogString(event.errorCode || 'client_event', 'client_event'),
      message: normalizeClientLogString(event.message, 'client_event'),
      detail: normalizeClientLogString(event.detail || '', ''),
      httpStatus: event.httpStatus ?? null,
      retryable: Boolean(event.retryable),
      promoteToIncident: Boolean(event.promoteToIncident),
      context: sanitizeClientContext(event.context || {}),
    }
    const baseURL = await getBaseURL()
    await fetch(baseURL + '/api/logs/client-events', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    }).catch(() => undefined)
  })
}

export async function request<T = unknown>(
  method: string,
  url: string,
  body?: unknown,
  options: RequestOptions = {},
): Promise<T> {
  const requestSignal = createRequestSignal(options)
  try {
    const requestOptions = { ...options, signal: requestSignal.signal }
    const resp = await sendWithBaseURLRetry(url, buildInit(method, body, requestOptions))

    if (!resp.ok) {
      const err = await resp.json().catch(() => ({ detail: resp.statusText }))
      if (!shouldSkipClientTelemetry(url)) {
        emitClientLog({
          level: resp.status >= 500 ? 'error' : 'warn',
          logger: 'frontend.api',
          module: 'frontend-api',
          action: 'request_failed',
          route: url,
          errorCode: resp.status >= 500 ? 'http_server_error' : 'http_request_failed',
          message: `${method.toUpperCase()} ${url} failed`,
          detail: (err as { detail?: string }).detail ?? resp.statusText,
          httpStatus: resp.status,
          retryable: resp.status >= 500,
          context: buildRequestContext(method, url, body, options, {
            responseReason: (err as ErrorPayload | null)?.reason ?? '',
          }),
        })
      }
      throw new Error((err as { detail?: string }).detail ?? `HTTP ${resp.status}`)
    }
    if (isMutationMethod(method) && !shouldSkipClientTelemetry(url)) {
      emitClientLog({
        level: 'info',
        logger: 'frontend.action',
        module: 'frontend-api',
        action: 'request_ok',
        route: url,
        errorCode: 'request_ok',
        message: `${method.toUpperCase()} ${url} ok`,
        context: buildRequestContext(method, url, body, options),
      })
    }
    const payload = await resp.json()
    return normalizeResponsePayload(payload) as T
  } catch (error) {
    if (requestSignal.didTimeout()) {
      if (!shouldSkipClientTelemetry(url)) {
        emitClientLog({
          level: 'error',
          logger: 'frontend.api',
          module: 'frontend-api',
          action: 'request_timeout',
          route: url,
          errorCode: 'request_timeout',
          message: `${method.toUpperCase()} ${url} timed out`,
          detail: options.timeoutMessage ?? `请求超时（${options.timeoutMs}ms）`,
          retryable: true,
          context: buildRequestContext(method, url, body, options),
        })
      }
      throw new Error(options.timeoutMessage ?? `请求超时（${options.timeoutMs}ms）`)
    }
    if (!shouldSkipClientTelemetry(url)) {
      emitClientLog({
        level: 'error',
        logger: 'frontend.api',
        module: 'frontend-api',
        action: 'request_exception',
        route: url,
        errorCode: 'request_exception',
        message: `${method.toUpperCase()} ${url} exception`,
        detail: error instanceof Error ? error.message : String(error),
        retryable: true,
        context: buildRequestContext(method, url, body, options),
      })
    }
    throw error
  } finally {
    requestSignal.cleanup()
  }
}

// ==================== MTOP helpers ====================

export function retStr(r: unknown): string {
  const ret = (r as { ret?: unknown })?.ret
  return Array.isArray(ret) ? String(ret[0]) : String(ret ?? '')
}

export function retMsg(r: unknown, fallback = '操作失败'): string {
  const s = retStr(r)
  if (!s) return fallback
  const idx = s.indexOf('::')
  let msg = idx >= 0 ? s.slice(idx + 2) : s
  msg = msg.replace(/\|\|.*$/, '').trim()
  return msg || fallback
}

export function isSuccess(r: unknown): boolean {
  return retStr(r).toUpperCase().includes('SUCCESS')
}
