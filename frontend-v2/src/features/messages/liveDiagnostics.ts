import { getAuthToken } from '../../api/client'
import {
  getRuntimeDiagnosticPolicy,
  normalizeRuntimeDiagnosticPolicy,
  type RuntimeDiagnosticPolicy,
} from '../../api/tasks'
import { useAuthStore } from '../../store/authStore'

export type LiveDiagnosticScope = {
  module: string
  username?: string
  deviceId?: string
  accountUid?: string
  sessionId?: string
  nowMs?: number
}

type BuildDiagnosticContextArgs = {
  scope: LiveDiagnosticScope
  event?: Record<string, unknown> | null
  extra?: Record<string, unknown>
  policy?: RuntimeDiagnosticPolicy | null
}

const LIVE_DIAGNOSTIC_POLICY_TTL_MS = 60_000

let cachedPolicy: RuntimeDiagnosticPolicy = normalizeRuntimeDiagnosticPolicy(null)
let cachedAuthToken = ''
let cachedUsername = ''
let lastPolicyRefreshAt = 0
let inFlightPolicyRefresh: Promise<RuntimeDiagnosticPolicy> | null = null

function currentAuthToken(): string {
  return String(getAuthToken() || useAuthStore.getState().token || '').trim()
}

function currentUsername(): string {
  return String(useAuthStore.getState().user?.username || '').trim()
}

function currentDeviceId(): string {
  return ''
}

function normalizedNeedles(values: string[]): string[] {
  return values
    .map((value) => value.trim().toLowerCase())
    .filter(Boolean)
}

function listMatches(values: string[], candidate: string): boolean {
  if (!values.length) return true
  const normalized = candidate.trim().toLowerCase()
  return normalized ? normalizedNeedles(values).includes(normalized) : false
}

function samplingKey(scope: LiveDiagnosticScope): string {
  return [
    scope.sessionId,
    scope.accountUid,
    scope.username,
    scope.deviceId,
    scope.module,
  ]
    .map((value) => String(value ?? '').trim())
    .find(Boolean) || '__runtime_diagnostic_default__'
}

function stableSamplingBucket(key: string): number {
  let hash = 0x811c9dc5
  const bytes = new TextEncoder().encode(key)
  for (const byte of bytes) {
    hash ^= byte
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return hash / 0x1_0000_0000
}

function sampleRateMatches(sampleRate: number, key: string): boolean {
  if (sampleRate >= 1) return true
  if (sampleRate <= 0) return false
  return stableSamplingBucket(key) < sampleRate
}

function coerceText(value: unknown): string {
  return String(value ?? '').trim()
}

function truncateText(value: string, limit = 48): string {
  return value.length > limit ? `${value.slice(0, limit)}...` : value
}

function redactSensitiveText(value: string): string {
  return truncateText(
    value
      .replace(/https?:\/\/\S+/gi, '[url]')
      .replace(/\b\d{5,}\b/g, '[id]')
      .replace(/\b[a-z0-9_-]{20,}\b/gi, '[token]'),
  )
}

function summarizeText(value: unknown): string {
  const text = coerceText(value)
  return text ? redactSensitiveText(text) : ''
}

function summarizeArray(value: unknown[], depth: number): unknown[] {
  if (depth > 1) return [`[items:${value.length}]`]
  return value.slice(0, 5).map((entry) => summarizePayload(entry, depth + 1))
}

function summarizeObject(value: Record<string, unknown>, depth: number): Record<string, unknown> {
  const summary: Record<string, unknown> = {}
  const entries = Object.entries(value).slice(0, 12)
  for (const [key, next] of entries) {
    const normalizedKey = key.toLowerCase()
    if (['token', 'authorization', 'cookie', 'password', 'secret', 'bodyhtml'].some((needle) => normalizedKey.includes(needle))) {
      summary[key] = '[redacted]'
      continue
    }
    if (['text', 'content', 'detail', 'reason'].includes(normalizedKey)) {
      summary[key] = summarizeText(next)
      continue
    }
    summary[key] = summarizePayload(next, depth + 1)
  }
  return summary
}

function summarizePayload(value: unknown, depth = 0): unknown {
  if (value === null || value === undefined) return null
  if (typeof value === 'string') return summarizeText(value)
  if (typeof value === 'number' || typeof value === 'boolean') return value
  if (Array.isArray(value)) return summarizeArray(value, depth)
  if (typeof value === 'object') return summarizeObject(value as Record<string, unknown>, depth)
  return summarizeText(value)
}

function shouldRefreshPolicy(authToken: string, username: string, force = false): boolean {
  if (!authToken) return false
  if (force) return true
  if (!lastPolicyRefreshAt) return true
  if (Date.now() - lastPolicyRefreshAt > LIVE_DIAGNOSTIC_POLICY_TTL_MS) return true
  return authToken !== cachedAuthToken || username !== cachedUsername
}

export function clearLiveDiagnosticPolicyCache() {
  cachedPolicy = normalizeRuntimeDiagnosticPolicy(null)
  cachedAuthToken = ''
  cachedUsername = ''
  lastPolicyRefreshAt = 0
  inFlightPolicyRefresh = null
}

export function getLiveDiagnosticPolicySnapshot(): RuntimeDiagnosticPolicy {
  return cachedPolicy
}

export async function refreshLiveDiagnosticPolicy(
  {
    authToken = currentAuthToken(),
    force = false,
  }: {
    authToken?: string
    force?: boolean
  } = {},
): Promise<RuntimeDiagnosticPolicy> {
  const token = String(authToken ?? '').trim()
  const username = currentUsername()
  if (!token) {
    clearLiveDiagnosticPolicyCache()
    return cachedPolicy
  }
  if (!shouldRefreshPolicy(token, username, force)) {
    return cachedPolicy
  }
  if (inFlightPolicyRefresh) {
    return inFlightPolicyRefresh
  }
  inFlightPolicyRefresh = getRuntimeDiagnosticPolicy()
    .then((policy) => {
      cachedPolicy = policy
      cachedAuthToken = token
      cachedUsername = username
      lastPolicyRefreshAt = Date.now()
      return cachedPolicy
    })
    .catch(() => {
      lastPolicyRefreshAt = Date.now()
      return cachedPolicy
    })
    .finally(() => {
      inFlightPolicyRefresh = null
    })
  return inFlightPolicyRefresh
}

export function maybeRefreshLiveDiagnosticPolicy(
  options?: {
    authToken?: string
    force?: boolean
  },
) {
  void refreshLiveDiagnosticPolicy(options)
}

export function matchesLiveDiagnosticPolicy(
  policy: RuntimeDiagnosticPolicy | null | undefined,
  scope: LiveDiagnosticScope,
): boolean {
  if (!policy?.enabled) return false
  const now = scope.nowMs ?? Date.now()
  if (policy.startsAt !== null && now < policy.startsAt) return false
  if (policy.endsAt !== null && now > policy.endsAt) return false
  if (!listMatches(policy.modules, scope.module)) return false
  if (!listMatches(policy.usernames, scope.username || currentUsername())) return false
  if (!listMatches(policy.deviceIds, scope.deviceId || currentDeviceId())) return false
  if (!listMatches(policy.accountUids, scope.accountUid || '')) return false
  if (!listMatches(policy.sessionIds, scope.sessionId || '')) return false
  if (!sampleRateMatches(policy.sampleRate, samplingKey(scope))) return false
  return true
}

export function shouldLogLiveDiagnostics(scope: LiveDiagnosticScope): boolean {
  const authToken = currentAuthToken()
  const username = scope.username || currentUsername()
  const deviceId = scope.deviceId || currentDeviceId()
  if (shouldRefreshPolicy(authToken, username)) {
    maybeRefreshLiveDiagnosticPolicy({ authToken })
  }
  return matchesLiveDiagnosticPolicy(cachedPolicy, {
    ...scope,
    username,
    deviceId,
  })
}

export function buildLiveDiagnosticContext({
  scope,
  event,
  extra,
  policy = cachedPolicy,
}: BuildDiagnosticContextArgs): Record<string, unknown> {
  const summary: Record<string, unknown> = {
    module: scope.module,
    username: scope.username || currentUsername(),
    deviceId: scope.deviceId || currentDeviceId(),
    accountUid: scope.accountUid || '',
    sessionId: scope.sessionId || '',
    policyLevel: policy?.level || 'DEBUG',
    sampleRate: Number(policy?.sampleRate ?? 1),
    includePayloads: Boolean(policy?.includePayloads),
  }
  if (event?.type) {
    summary.eventType = coerceText(event.type)
  }
  if (event?.reason) {
    summary.reason = summarizeText(event.reason)
  }
  if (event?.snapshot !== undefined) {
    summary.snapshot = Boolean(event.snapshot)
  }
  if (event?.requestId) {
    summary.requestId = coerceText(event.requestId)
  }
  if (extra) {
    Object.assign(summary, summarizePayload(extra))
  }
  if (policy?.includePayloads && event) {
    summary.payloadPreview = summarizePayload(event)
  }
  return summary
}
