import { emitClientLog, type ClientLogEvent, type ClientLogLevel } from '../api/client'

export type ActionLogOutcome = 'started' | 'success' | 'failed' | 'warn' | 'cancelled'

export type ActionLogInput = {
  module: string
  action: string
  outcome?: ActionLogOutcome
  message: string
  detail?: string
  error?: unknown
  context?: Record<string, unknown>
  level?: ClientLogLevel
  errorCode?: string
  retryable?: boolean
  promoteToIncident?: boolean
}

function currentRoute(): string {
  return typeof window === 'undefined' ? '' : window.location.pathname
}

function normalizePart(value: string, fallback: string): string {
  const trimmed = String(value || '').trim()
  return trimmed || fallback
}

export function errorDetail(error: unknown): string {
  if (!error) return ''
  if (error instanceof Error) return error.message || error.name || 'Error'
  return String(error)
}

function levelForOutcome(outcome: ActionLogOutcome, level?: ClientLogLevel): ClientLogLevel {
  if (level) return level
  if (outcome === 'failed') return 'error'
  if (outcome === 'warn' || outcome === 'cancelled') return 'warn'
  return 'info'
}

export function buildActionLogEvent(input: ActionLogInput): ClientLogEvent {
  const outcome = input.outcome || 'success'
  const action = `${normalizePart(input.action, 'action')}_${outcome}`
  const level = levelForOutcome(outcome, input.level)
  const detail = input.detail || errorDetail(input.error)
  return {
    level,
    logger: 'frontend.action',
    module: normalizePart(input.module, 'frontend-action'),
    action,
    route: currentRoute(),
    errorCode: input.errorCode || action,
    message: input.message,
    detail,
    retryable: input.retryable ?? outcome === 'failed',
    promoteToIncident: input.promoteToIncident ?? outcome === 'failed',
    context: {
      outcome,
      ...(input.context || {}),
    },
  }
}

export function emitActionLog(input: ActionLogInput): void {
  emitClientLog(buildActionLogEvent(input))
}

export function emitActionStarted(input: Omit<ActionLogInput, 'outcome'>): void {
  emitActionLog({ ...input, outcome: 'started', retryable: false, promoteToIncident: false })
}

export function emitActionSuccess(input: Omit<ActionLogInput, 'outcome'>): void {
  emitActionLog({ ...input, outcome: 'success', retryable: false, promoteToIncident: false })
}

export function emitActionFailure(input: Omit<ActionLogInput, 'outcome'>): void {
  emitActionLog({ ...input, outcome: 'failed' })
}

export async function withActionLog<T>(
  input: Omit<ActionLogInput, 'outcome' | 'detail' | 'error'> & {
    startedMessage?: string
    successMessage?: string
    successContext?: (result: T) => Record<string, unknown>
  },
  run: () => Promise<T>,
): Promise<T> {
  emitActionStarted({
    ...input,
    message: input.startedMessage || input.message,
  })
  try {
    const result = await run()
    emitActionSuccess({
      ...input,
      message: input.successMessage || input.message,
      context: {
        ...(input.context || {}),
        ...(input.successContext ? input.successContext(result) : {}),
      },
    })
    return result
  } catch (error) {
    emitActionFailure({
      ...input,
      error,
    })
    throw error
  }
}
