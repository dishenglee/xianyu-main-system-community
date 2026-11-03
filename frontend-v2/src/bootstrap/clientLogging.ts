import { emitClientLog, type ClientLogEvent } from '../api/client'

const DUPLICATE_WINDOW_MS = 5000
const recentFingerprints = new Map<string, number>()
let installed = false
let forwardingConsoleError = false
let forwardingConsoleWarn = false

type ClientLogEmitter = (event: ClientLogEvent) => void

export function clearClientLogDedup() {
  recentFingerprints.clear()
}

export function fingerprintClientLog(event: ClientLogEvent): string {
  return [
    event.level || '',
    event.logger || '',
    event.module || '',
    event.action || '',
    event.route || '',
    event.errorCode || '',
    event.message || '',
    event.detail || '',
  ].join('\n')
}

export function shouldDropDuplicateClientLog(
  event: ClientLogEvent,
  nowMs: number = Date.now(),
): boolean {
  const fingerprint = fingerprintClientLog(event)
  const previous = recentFingerprints.get(fingerprint)
  recentFingerprints.set(fingerprint, nowMs)
  return previous !== undefined && nowMs - previous < DUPLICATE_WINDOW_MS
}

export function normalizeConsoleArgs(args: unknown[]): { message: string; detail: string } {
  const parts = args.map((value) => {
    if (value instanceof Error) {
      return value.message || value.name || 'Error'
    }
    if (typeof value === 'string') {
      return value
    }
    try {
      return JSON.stringify(value)
    } catch {
      return String(value)
    }
  })
  const message = String(parts[0] || 'console.log')
  const detail = parts.slice(1).join(' | ')
  return { message, detail }
}

function currentRoute(): string {
  return typeof window === 'undefined' ? '' : window.location.pathname
}

function emitIfNeeded(emitter: ClientLogEmitter, event: ClientLogEvent) {
  if (!shouldDropDuplicateClientLog(event)) {
    emitter(event)
  }
}

export function installGlobalClientLogging(emitter: ClientLogEmitter = emitClientLog) {
  if (installed || typeof window === 'undefined') return
  installed = true

  window.addEventListener('error', (event) => {
    const detail = [
      event.filename || '',
      event.lineno ? `line=${event.lineno}` : '',
      event.colno ? `col=${event.colno}` : '',
    ]
      .filter(Boolean)
      .join(' ')
    emitIfNeeded(emitter, {
      level: 'error',
      logger: 'frontend.runtime',
      module: 'frontend-runtime',
      action: 'window_error',
      route: currentRoute(),
      errorCode: 'window_error',
      message: event.message || 'Unhandled window error',
      detail: detail || event.error?.message || '',
      promoteToIncident: true,
      retryable: false,
      context: {
        stack: event.error?.stack || '',
        filename: event.filename || '',
      },
    })
  })

  window.addEventListener('unhandledrejection', (event) => {
    const reason = event.reason
    emitIfNeeded(emitter, {
      level: 'error',
      logger: 'frontend.runtime',
      module: 'frontend-runtime',
      action: 'unhandled_rejection',
      route: currentRoute(),
      errorCode: 'unhandled_rejection',
      message:
        reason instanceof Error
          ? reason.message || 'Unhandled promise rejection'
          : String(reason || 'Unhandled promise rejection'),
      detail: reason instanceof Error ? reason.stack || '' : '',
      promoteToIncident: true,
      retryable: false,
      context: {
        reasonType: typeof reason,
      },
    })
  })

  const originalConsoleError = console.error.bind(console)
  console.error = (...args: unknown[]) => {
    originalConsoleError(...args)
    if (forwardingConsoleError) return
    forwardingConsoleError = true
    try {
      const { message, detail } = normalizeConsoleArgs(args)
      emitIfNeeded(emitter, {
        level: 'error',
        logger: 'frontend.console',
        module: 'frontend-console',
        action: 'console_error',
        route: currentRoute(),
        errorCode: 'console_error',
        message,
        detail,
        retryable: false,
        context: {
          argumentCount: args.length,
        },
      })
    } finally {
      forwardingConsoleError = false
    }
  }

  const originalConsoleWarn = console.warn.bind(console)
  console.warn = (...args: unknown[]) => {
    originalConsoleWarn(...args)
    if (forwardingConsoleWarn) return
    forwardingConsoleWarn = true
    try {
      const { message, detail } = normalizeConsoleArgs(args)
      emitIfNeeded(emitter, {
        level: 'warn',
        logger: 'frontend.console',
        module: 'frontend-console',
        action: 'console_warn',
        route: currentRoute(),
        errorCode: 'console_warn',
        message,
        detail,
        retryable: false,
        context: {
          argumentCount: args.length,
        },
      })
    } finally {
      forwardingConsoleWarn = false
    }
  }
}
