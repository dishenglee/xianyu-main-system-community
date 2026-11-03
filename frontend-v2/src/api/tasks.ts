import type { Task, TaskProgress, TaskDeleteJob, TaskAttempt, TaskLog, Pagination } from './types'
import { getBaseURL, request } from './client'
import { search } from './market'

// ==================== 任务系统 - normalizers ====================

function normalizeTaskProgress(progress: unknown): TaskProgress | null {
  if (!progress || typeof progress !== 'object') return null
  const p = progress as Record<string, unknown>
  return {
    status: String(p.status ?? 'draft'),
    total_targets: Number(p.total_targets ?? 0),
    completed_targets: Number(p.completed_targets ?? 0),
    successful_targets: Number(p.successful_targets ?? 0),
    failed_targets: Number(p.failed_targets ?? 0),
    skipped_targets: Number(p.skipped_targets ?? 0),
    pending_targets: Number(p.pending_targets ?? 0),
    percent: Number(p.percent ?? 0),
    current_target_id: String(p.current_target_id ?? ''),
    current_account_uid: String(p.current_account_uid ?? ''),
    last_message: String(p.last_message ?? ''),
    updated_at: Number(p.updated_at ?? 0),
  }
}

function normalizeTaskTarget(target: unknown) {
  const t = (target ?? {}) as Record<string, unknown>
  return {
    item_id: String(t.item_id ?? ''),
    position: Number(t.position ?? 0),
    title: String(t.title ?? ''),
    price: String(t.price ?? ''),
    seller_id: String(t.seller_id ?? ''),
    seller_nick: String(t.seller_nick ?? ''),
    status: String(t.status ?? 'pending'),
    created_at: Number(t.created_at ?? 0),
    updated_at: Number(t.updated_at ?? 0),
  }
}

function normalizeTaskAttemptSummary(summary: unknown) {
  if (!summary || typeof summary !== 'object') {
    return { total: 0, success: 0, failed: 0, skipped: 0, latest_created_at: 0 }
  }
  const s = summary as Record<string, unknown>
  return {
    total: Number(s.total ?? 0),
    success: Number(s.success ?? 0),
    failed: Number(s.failed ?? 0),
    skipped: Number(s.skipped ?? 0),
    latest_created_at: Number(s.latest_created_at ?? 0),
  }
}

function normalizeTaskDeleteJobSummary(summary: unknown) {
  if (!summary || typeof summary !== 'object') {
    return {
      total: 0, pending: 0, running: 0, retryable: 0,
      succeeded: 0, failed: 0, cancelled: 0,
      already_removed: 0, item_gone: 0, latest_created_at: 0,
    }
  }
  const s = summary as Record<string, unknown>
  return {
    total: Number(s.total ?? 0),
    pending: Number(s.pending ?? 0),
    running: Number(s.running ?? 0),
    retryable: Number(s.retryable ?? 0),
    succeeded: Number(s.succeeded ?? 0),
    failed: Number(s.failed ?? 0),
    cancelled: Number(s.cancelled ?? 0),
    already_removed: Number(s.already_removed ?? 0),
    item_gone: Number(s.item_gone ?? 0),
    latest_created_at: Number(s.latest_created_at ?? 0),
  }
}

export function normalizeTaskDeleteJob(job: unknown): TaskDeleteJob {
  const j = (job ?? {}) as Record<string, unknown>
  return {
    id: Number(j.id ?? 0),
    task_id: String(j.task_id ?? ''),
    attempt_id: Number(j.attempt_id ?? 0),
    account_uid: String(j.account_uid ?? ''),
    item_id: String(j.item_id ?? ''),
    comment_id: String(j.comment_id ?? ''),
    status: String(j.status ?? 'pending'),
    payload: (j.payload && typeof j.payload === 'object' ? j.payload : {}) as Record<string, unknown>,
    scheduled_at: Number(j.scheduled_at ?? 0),
    retry_count: Number(j.retry_count ?? 0),
    started_at: Number(j.started_at ?? 0),
    finished_at: Number(j.finished_at ?? 0),
    cancelled_at: Number(j.cancelled_at ?? 0),
    last_error_at: Number(j.last_error_at ?? 0),
    last_error_code: String(j.last_error_code ?? ''),
    last_error_message: String(j.last_error_message ?? ''),
    claimed_by: String(j.claimed_by ?? ''),
    claim_token: String(j.claim_token ?? ''),
    created_at: Number(j.created_at ?? 0),
    updated_at: Number(j.updated_at ?? 0),
  }
}

export function normalizeTaskAttempt(attempt: unknown): TaskAttempt {
  const a = (attempt ?? {}) as Record<string, unknown>
  return {
    id: Number(a.id ?? 0),
    task_id: String(a.task_id ?? ''),
    account_uid: String(a.account_uid ?? ''),
    item_id: String(a.item_id ?? ''),
    target_status: String(a.target_status ?? 'pending'),
    success: Boolean(a.success),
    comment_content: String(a.comment_content ?? ''),
    comment_content_hash: String(a.comment_content_hash ?? ''),
    source_comment: (a.source_comment && typeof a.source_comment === 'object' ? a.source_comment : {}) as Record<string, unknown>,
    published_comment_id: String(a.published_comment_id ?? ''),
    published_at: Number(a.published_at ?? 0),
    failure_reason_code: String(a.failure_reason_code ?? ''),
    failure_reason_message: String(a.failure_reason_message ?? ''),
    failure_scope: String(a.failure_scope ?? ''),
    request_payload: (a.request_payload && typeof a.request_payload === 'object' ? a.request_payload : {}) as Record<string, unknown>,
    response_payload: (a.response_payload && typeof a.response_payload === 'object' ? a.response_payload : {}) as Record<string, unknown>,
    delete_job_payload: (a.delete_job_payload && typeof a.delete_job_payload === 'object' ? a.delete_job_payload : {}) as Record<string, unknown>,
    created_at: Number(a.created_at ?? 0),
    updated_at: Number(a.updated_at ?? 0),
  }
}

export function normalizeTask(task: unknown): Task {
  const t = (task ?? {}) as Record<string, unknown>
  const progress = normalizeTaskProgress(t.progress)
  return {
    task_id: String(t.task_id ?? ''),
    title: String(t.title ?? '未命名任务'),
    status: String(t.status ?? 'draft'),
    source: String(t.source ?? 'comment_targets'),
    target_total: Number(t.target_total ?? progress?.total_targets ?? 0),
    account_total: Number(t.account_total ?? (Array.isArray(t.account_uids) ? t.account_uids.length : 0)),
    created_at: Number(t.created_at ?? 0),
    updated_at: Number(t.updated_at ?? 0),
    started_at: Number(t.started_at ?? 0),
    paused_at: Number(t.paused_at ?? 0),
    finished_at: Number(t.finished_at ?? 0),
    last_error: String(t.last_error ?? ''),
    last_transition_at: Number(t.last_transition_at ?? 0),
    last_log_at: Number(t.last_log_at ?? 0),
    comment_templates: Array.isArray(t.comment_templates) ? t.comment_templates as string[] : [],
    publish_interval_min_seconds: Number(t.publish_interval_min_seconds ?? 0),
    publish_interval_max_seconds: Number(t.publish_interval_max_seconds ?? 0),
    delete_interval_min_seconds: Number(t.delete_interval_min_seconds ?? 0),
    delete_interval_max_seconds: Number(t.delete_interval_max_seconds ?? 0),
    delete_after_seconds: Number(t.delete_after_seconds ?? 3600),
    enable_delete_comments: Boolean(t.enable_delete_comments ?? true),
    account_uids: Array.isArray(t.account_uids) ? t.account_uids as string[] : [],
    targets: Array.isArray(t.targets) ? t.targets.map(normalizeTaskTarget) : [],
    progress,
    attempt_summary: normalizeTaskAttemptSummary(t.attempt_summary),
    delete_job_summary: normalizeTaskDeleteJobSummary(t.delete_job_summary),
  }
}

export function normalizeTaskLog(log: unknown): TaskLog {
  const l = (log ?? {}) as Record<string, unknown>
  return {
    id: Number(l.id ?? 0),
    task_id: String(l.task_id ?? ''),
    level: String(l.level ?? 'info'),
    event: String(l.event ?? 'info'),
    message: String(l.message ?? ''),
    context: (l.context && typeof l.context === 'object' ? l.context : {}) as Record<string, unknown>,
    created_at: Number(l.created_at ?? 0),
  }
}

export type RuntimeDiagnosticPolicy = {
  enabled: boolean
  level: string
  modules: string[]
  usernames: string[]
  deviceIds: string[]
  accountUids: string[]
  sessionIds: string[]
  startsAt: number | null
  endsAt: number | null
  includePayloads: boolean
  sampleRate: number
  refreshedAt: number
  source: string
}

const RUNTIME_DIAGNOSTIC_POLICY_ENDPOINTS = [
  '/api/logs/diagnostic-policy',
  '/api/logs/diagnostics/policy',
]

function normalizeDiagnosticStringList(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  return value
    .map((entry) => String(entry ?? '').trim())
    .filter(Boolean)
}

function normalizeDiagnosticTimestamp(value: unknown): number | null {
  const normalizeNumeric = (numeric: number): number | null => {
    if (!Number.isFinite(numeric) || numeric <= 0) return null
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric
  }
  if (typeof value === 'number' && Number.isFinite(value)) {
    return normalizeNumeric(value)
  }
  if (typeof value === 'string') {
    const trimmed = value.trim()
    if (!trimmed) return null
    const numeric = Number(trimmed)
    if (Number.isFinite(numeric)) return normalizeNumeric(numeric)
    const parsed = Date.parse(trimmed)
    return normalizeNumeric(parsed)
  }
  return null
}

function unwrapRuntimeDiagnosticPayload(payload: unknown): Record<string, unknown> {
  const current = (payload && typeof payload === 'object' ? payload : {}) as Record<string, unknown>
  for (const key of ['data', 'policy', 'result']) {
    const nested = current[key]
    if (nested && typeof nested === 'object') {
      return nested as Record<string, unknown>
    }
  }
  return current
}

function normalizeDiagnosticLevel(value: unknown): string {
  const normalized = String(value ?? '').trim().toUpperCase()
  if (['TRACE', 'DEBUG', 'INFO', 'WARN', 'WARNING', 'ERROR'].includes(normalized)) {
    return normalized === 'WARNING' ? 'WARN' : normalized
  }
  return 'DEBUG'
}

function normalizeDiagnosticSampleRate(value: unknown): number {
  const numeric = Number(value)
  if (!Number.isFinite(numeric)) return 1
  return Math.max(0, Math.min(1, numeric))
}

export function normalizeRuntimeDiagnosticPolicy(
  payload: unknown,
  source = '',
): RuntimeDiagnosticPolicy {
  const policy = unwrapRuntimeDiagnosticPayload(payload)
  return {
    enabled: Boolean(policy.enabled),
    level: normalizeDiagnosticLevel(policy.level),
    modules: normalizeDiagnosticStringList(policy.modules),
    usernames: normalizeDiagnosticStringList(policy.usernames),
    deviceIds: normalizeDiagnosticStringList(policy.deviceIds ?? policy.device_ids),
    accountUids: normalizeDiagnosticStringList(policy.accountUids ?? policy.account_uids),
    sessionIds: normalizeDiagnosticStringList(policy.sessionIds ?? policy.session_ids),
    startsAt: normalizeDiagnosticTimestamp(policy.startsAt ?? policy.starts_at),
    endsAt: normalizeDiagnosticTimestamp(policy.endsAt ?? policy.ends_at),
    includePayloads: Boolean(policy.includePayloads ?? policy.include_payloads),
    sampleRate: normalizeDiagnosticSampleRate(policy.sampleRate ?? policy.sample_rate),
    refreshedAt: Date.now(),
    source,
  }
}

export const getRuntimeDiagnosticPolicy = async (
  {
    signal,
  }: {
    signal?: AbortSignal
  } = {},
): Promise<RuntimeDiagnosticPolicy> => {
  const baseURL = await getBaseURL()
  const headers: Record<string, string> = {
    Accept: 'application/json',
  }

  for (const endpoint of RUNTIME_DIAGNOSTIC_POLICY_ENDPOINTS) {
    try {
      const response = await fetch(baseURL + endpoint, {
        method: 'GET',
        headers,
        signal,
      })
      if (response.status === 401 || response.status === 403 || response.status === 404) {
        continue
      }
      if (!response.ok) {
        return normalizeRuntimeDiagnosticPolicy(null)
      }
      const payload = await response.json().catch(() => null)
      return normalizeRuntimeDiagnosticPolicy(payload, endpoint)
    } catch (error) {
      if (signal?.aborted) {
        break
      }
      return normalizeRuntimeDiagnosticPolicy(null)
    }
  }

  return normalizeRuntimeDiagnosticPolicy(null)
}

// ==================== 任务 API ====================

export const getTasks = async ({ status = '', page = 1, pageSize = 20 } = {}) => {
  const q = new URLSearchParams()
  if (status) q.set('status', status)
  q.set('page', String(page))
  q.set('page_size', String(pageSize))
  const response = await request<{ items: unknown[]; pagination: Pagination }>('GET', `/api/tasks?${q.toString()}`)
  return {
    items: Array.isArray(response?.items) ? response.items.map(normalizeTask) : [],
    pagination: response?.pagination ?? { page, page_size: pageSize, total: 0 },
  }
}

export const getTask = async (id: string) => normalizeTask(await request('GET', `/api/tasks/${id}`))
export const getTaskProgress = async (id: string) =>
  normalizeTaskProgress(await request('GET', `/api/tasks/${id}/progress`))

export const getTaskDeleteJobs = async (
  {
    taskId = '', status = '', accountUid = '', itemId = '',
    page = 1, pageSize = 20,
  }: {
    taskId?: string; status?: string; accountUid?: string; itemId?: string
    page?: number; pageSize?: number
  } = {},
) => {
  const q = new URLSearchParams({ page: String(page), page_size: String(pageSize) })
  if (taskId) q.set('task_id', taskId)
  if (status) q.set('status', status)
  if (accountUid) q.set('account_uid', accountUid)
  if (itemId) q.set('item_id', itemId)
  const response = await request<{ items: unknown[]; pagination: Pagination }>('GET', `/api/tasks/delete-jobs?${q.toString()}`)
  return {
    items: Array.isArray(response?.items) ? response.items.map(normalizeTaskDeleteJob) : [],
    pagination: response?.pagination ?? { page, page_size: pageSize, total: 0 },
  }
}

export const retryTaskDeleteJob = async (id: number) => {
  const response = await request<{ job?: unknown }>('POST', `/api/tasks/delete-jobs/${id}/retry`)
  return response?.job ? normalizeTaskDeleteJob(response.job) : null
}
export const cancelTaskDeleteJob = async (id: number) => {
  const response = await request<{ job?: unknown }>('POST', `/api/tasks/delete-jobs/${id}/cancel`)
  return response?.job ? normalizeTaskDeleteJob(response.job) : null
}

export const getTaskLogs = async (
  id: string,
  { limit = 50, offset = 0, level = '', accountUid = '', itemId = '' } = {},
) => {
  const q = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  if (level) q.set('level', level)
  if (accountUid) q.set('account_uid', accountUid)
  if (itemId) q.set('item_id', itemId)
  const response = await request<{ items: unknown[]; pagination: { limit: number; offset: number; total: number } }>(
    'GET', `/api/tasks/${id}/logs?${q.toString()}`,
  )
  return {
    items: Array.isArray(response?.items) ? response.items.map(normalizeTaskLog) : [],
    pagination: response?.pagination ?? { limit, offset, total: 0 },
  }
}

export const getTaskAttempts = async (
  id: string,
  { limit = 20, offset = 0, accountUid = '', itemId = '', success = '' } = {},
) => {
  const q = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  if (accountUid) q.set('account_uid', accountUid)
  if (itemId) q.set('item_id', itemId)
  if (success !== '' && success !== null && success !== undefined) q.set('success', String(success))
  const response = await request<{ items: unknown[]; pagination: { limit: number; offset: number; total: number } }>(
    'GET', `/api/tasks/${id}/attempts?${q.toString()}`,
  )
  return {
    items: Array.isArray(response?.items) ? response.items.map(normalizeTaskAttempt) : [],
    pagination: response?.pagination ?? { limit, offset, total: 0 },
  }
}

export const createTask = async ({
  account_uids = [] as string[],
  target_item_ids = [] as string[],
  title = '',
  comment_templates = [] as string[],
  publish_interval_min_seconds = 0,
  publish_interval_max_seconds = 0,
  delete_interval_min_seconds = 0,
  delete_interval_max_seconds = 0,
  delete_after_seconds = 3600,
  enable_delete_comments = true,
} = {}) =>
  normalizeTask(await request('POST', '/api/tasks', {
    account_uids, target_item_ids, title, comment_templates,
    publish_interval_min_seconds, publish_interval_max_seconds,
    delete_interval_min_seconds, delete_interval_max_seconds,
    delete_after_seconds, enable_delete_comments,
  }))

export const updateTask = async (id: string, data: Partial<Task>) =>
  normalizeTask(await request('PATCH', `/api/tasks/${id}`, data))
export const startTask = (id: string) => request('POST', `/api/tasks/${id}/start`)
export const pauseTask = (id: string) => request('POST', `/api/tasks/${id}/pause`)
export const deleteTask = (id: string) => request('DELETE', `/api/tasks/${id}`)

// ==================== 评论目标池 ====================

export const batchAddCommentTargets = (items: unknown[]) =>
  request('POST', '/api/comment-targets/batch-add', { items })

export const getCommentTargets = ({ status = '', page = 1, pageSize = 20 } = {}) => {
  const q = new URLSearchParams()
  if (status) q.set('status', status)
  q.set('page', String(page))
  q.set('page_size', String(pageSize))
  return request('GET', `/api/comment-targets?${q.toString()}`)
}

export const getCommentCandidates = (
  { uid = '', quality = '', category = '', acceptedForTxt = null as boolean | null, page = 1, pageSize = 50 } = {},
) => {
  const q = new URLSearchParams({ uid: String(uid), page: String(page), page_size: String(pageSize) })
  if (quality) q.set('quality', quality)
  if (category) q.set('category', category)
  if (acceptedForTxt !== null && acceptedForTxt !== undefined) {
    q.set('accepted_for_txt', String(Boolean(acceptedForTxt)))
  }
  return request('GET', `/api/comment-candidates?${q.toString()}`)
}

export const exportCommentCandidates = (
  { uid = '', quality = '', category = '', acceptedForTxt = null as boolean | null, directory = '', fileName = '' } = {},
) =>
  request('POST', '/api/comment-candidates/export', {
    uid, quality, category,
    accepted_for_txt: acceptedForTxt !== null && acceptedForTxt !== undefined ? Boolean(acceptedForTxt) : null,
    directory, file_name: fileName,
  })

export const importCommentCandidates = (
  { uid = '', filePath = '', category = '', sourceApi = 'txt_import', sourceType = 'txt', itemId = 'txt-import' } = {},
) =>
  request('POST', '/api/comment-candidates/import', {
    uid, file_path: filePath, category,
    source_api: sourceApi, source_type: sourceType, item_id: itemId,
  })

export const getCommentLibrary = ({ enabled = null as boolean | null, page = 1, pageSize = 50 } = {}) => {
  const q = new URLSearchParams({ page: String(page), page_size: String(pageSize) })
  if (enabled !== null && enabled !== undefined) q.set('enabled', String(Boolean(enabled)))
  return request('GET', `/api/comment-library?${q.toString()}`)
}

export const createCommentLibraryEntry = ({ content }: { content: string }) =>
  request('POST', '/api/comment-library', { content })

export const batchCreateCommentLibrary = (contents: string[]) =>
  request('POST', '/api/comment-library/batch', { contents })

export const importCommentLibraryFromTxt = (
  { filePath = '', sourceLabel = 'txt_import', sourceRef = '', sourceItemId = '' } = {},
) =>
  request('POST', '/api/comment-library/import/txt', {
    file_path: filePath, source_label: sourceLabel,
    source_ref: sourceRef, source_item_id: sourceItemId,
  })

export const importCommentLibraryFromCandidates = ({ uid = '', quality = '', category = '' } = {}) =>
  request('POST', '/api/comment-library/import/candidates', { uid, quality, category })

export const setCommentLibraryEnabled = ({ contentHash, enabled }: { contentHash: string; enabled: boolean }) =>
  request('POST', '/api/comment-library/enabled', { content_hash: contentHash, enabled: Boolean(enabled) })

// ==================== 日志 ====================

export const getLogs = (params: {
  stream?: string
  level?: string
  keyword?: string
  logger?: string
  module?: string
  traceId?: string
  requestId?: string
  incidentId?: string
  username?: string
  deviceId?: string
  accountUid?: string
  sessionId?: string
  itemId?: string
  jobId?: string
  action?: string
  outcome?: string
  errorCode?: string
  limit?: number
  since?: string | number
  afterSequence?: number
} = {}) => {
  const q = new URLSearchParams()
  if (params.stream) q.set('stream', params.stream)
  if (params.level) q.set('level', params.level)
  if (params.keyword) q.set('keyword', params.keyword)
  if (params.logger) q.set('logger', params.logger)
  if (params.module) q.set('module', params.module)
  if (params.traceId) q.set('trace_id', params.traceId)
  if (params.requestId) q.set('request_id', params.requestId)
  if (params.incidentId) q.set('incident_id', params.incidentId)
  if (params.username) q.set('username', params.username)
  if (params.deviceId) q.set('device_id', params.deviceId)
  if (params.accountUid) q.set('account_uid', params.accountUid)
  if (params.sessionId) q.set('session_id', params.sessionId)
  if (params.itemId) q.set('item_id', params.itemId)
  if (params.jobId) q.set('job_id', params.jobId)
  if (params.action) q.set('action', params.action)
  if (params.outcome) q.set('outcome', params.outcome)
  if (params.errorCode) q.set('error_code', params.errorCode)
  if (params.limit) q.set('limit', String(params.limit))
  if (params.since) q.set('since', String(params.since))
  if (params.afterSequence) q.set('after_sequence', String(params.afterSequence))
  return request('GET', `/api/logs?${q.toString()}`)
}

export const clearLogs = () => request('DELETE', '/api/logs')

export const getIncidents = (params: {
  level?: string
  module?: string
  errorCode?: string
  incidentId?: string
  limit?: number
  since?: string | number
} = {}) => {
  const q = new URLSearchParams()
  if (params.level) q.set('level', params.level)
  if (params.module) q.set('module', params.module)
  if (params.errorCode) q.set('error_code', params.errorCode)
  if (params.incidentId) q.set('incident_id', params.incidentId)
  if (params.limit) q.set('limit', String(params.limit))
  if (params.since) q.set('since', String(params.since))
  return request('GET', `/api/logs/incidents?${q.toString()}`)
}

export const getCommentDeleteQueue = ({ page = 1, pageSize = 50 } = {}) =>
  request('GET', `/api/comment-delete-queue?page=${page}&page_size=${pageSize}`)

// ==================== 操作日志 / 后台任务 ====================

export const getOperationLogs = ({
  module = '', action = '', uid = '', status = '', keyword = '', limit = 200, since = 0,
}: {
  module?: string; action?: string; uid?: string; status?: string; keyword?: string; limit?: number; since?: number
} = {}) => {
  const q = new URLSearchParams()
  if (module) q.set('module', module)
  if (action) q.set('action', action)
  if (uid) q.set('uid', uid)
  if (status) q.set('status', status)
  if (keyword) q.set('keyword', keyword)
  if (limit) q.set('limit', String(limit))
  if (since) q.set('since', String(since))
  return request('GET', `/api/operation-logs?${q.toString()}`)
}

export const getOperationJobs = () => request('GET', '/api/operation-jobs')
export const getOperationJob = (id: string) => request('GET', `/api/operation-jobs/${id}`)
export const createOperationJob = (payload: {
  kind: string
  module: string
  action: string
  operator?: string
  trigger_source?: string
  summary?: Record<string, unknown>
  items: Array<{
    uid: string
    account_nick?: string
    target_type?: string
    target_id: string
    target_title?: string
    params?: Record<string, unknown>
  }>
}) => request('POST', '/api/operation-jobs', payload)

export const getMessageDailyStats = ({
  uid = '', startDate = '', endDate = '', limit = 30,
}: { uid?: string; startDate?: string; endDate?: string; limit?: number } = {}) => {
  const q = new URLSearchParams()
  if (uid) q.set('uid', uid)
  if (startDate) q.set('start_date', startDate)
  if (endDate) q.set('end_date', endDate)
  if (limit) q.set('limit', String(limit))
  return request('GET', `/api/message-stats/daily?${q.toString()}`)
}

// Search-keyword harvesting → batch-add into comment_targets pool.
// `search()` returns the raw mtop payload; caller is responsible for
// extracting items + posting them to /comment-targets/batch-add.
export const harvestCommentCandidates = async (
  uid: string,
  keyword: string,
  { page = 1, pageSize = 30 }: { page?: number; pageSize?: number } = {},
) => search(uid, keyword, { page, pageSize })
