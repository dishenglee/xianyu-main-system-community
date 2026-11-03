// @ts-nocheck
import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  Plus, RefreshCw, Play, Pause, Trash2, Pencil, ChevronDown, ChevronUp,
  CheckCircle, XCircle, Clock, Target,
} from 'lucide-react'
import {
  getTasks, getTask, createTask, updateTask, startTask, pauseTask, deleteTask,
  getTaskAttempts, getCommentTargets,
} from '../../api/index'
import { getAuthToken } from '../../api/client'
import { useAccounts } from '../../hooks/useAccounts'
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { Modal } from '../../components/Modal'
import { StatusBadge } from '../../components/StatusBadge'
import { useSafeNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import { useDevice } from '../../hooks/useDevice'
import { filterBusinessAccounts } from '../../utils/accountAccess'

const STATUS_MAP = {
  draft:          { kind: 'neutral', label: '草稿' },
  queued:         { kind: 'warning', label: '排队中' },
  running:        { kind: 'info',    label: '运行中' },
  paused:         { kind: 'neutral', label: '已暂停' },
  waiting_delete: { kind: 'warning', label: '等待删评' },
  finished:       { kind: 'success', label: '已完成' },
  failed:         { kind: 'danger',  label: '失败' },
}

const EDITABLE_STATUSES = new Set(['draft', 'paused', 'failed'])
const STARTABLE_STATUSES = new Set(['draft', 'paused', 'failed'])
const PAUSABLE_STATUSES = new Set(['queued', 'running', 'waiting_delete'])

function fmtTime(ts) {
  if (!ts) return '—'
  const ms = ts > 1e12 ? ts : ts * 1000
  return new Date(ms).toLocaleString('zh-CN')
}

function unwrapList(r) {
  if (!r) return []
  if (Array.isArray(r)) return r
  if (Array.isArray(r.items)) return r.items
  if (Array.isArray(r.data)) return r.data
  if (Array.isArray(r.tasks)) return r.tasks
  return []
}

function defaultForm() {
  return {
    title: '',
    account_uids: [],
    target_item_ids: [],
    templatesText: '你好请问还在吗\n可以包邮吗\n请问还能优惠吗',
    publishMin: 5,
    publishMax: 15,
    publishUnit: 'seconds',
    deleteMin: 60,
    deleteMax: 180,
    deleteUnit: 'seconds',
    deleteAfter: 60,
    deleteAfterUnit: 'minutes',
    enableDelete: true,
  }
}

function formFromTask(task) {
  const f = defaultForm()
  f.title = task.title || ''
  f.account_uids = [...(task.account_uids || [])]
  f.target_item_ids = (task.targets || []).map((t) => t.item_id)
  f.templatesText = (task.comment_templates || []).join('\n')
  f.publishMin = Math.max(0, Math.round(task.publish_interval_min_seconds || 0))
  f.publishMax = Math.max(0, Math.round(task.publish_interval_max_seconds || 0))
  f.deleteMin = Math.max(0, Math.round(task.delete_interval_min_seconds || 0))
  f.deleteMax = Math.max(0, Math.round(task.delete_interval_max_seconds || 0))
  const da = Math.max(0, Math.round(task.delete_after_seconds || 0))
  if (da >= 3600 && da % 3600 === 0) { f.deleteAfter = da / 3600; f.deleteAfterUnit = 'hours' }
  else if (da >= 60 && da % 60 === 0) { f.deleteAfter = da / 60; f.deleteAfterUnit = 'minutes' }
  else { f.deleteAfter = da; f.deleteAfterUnit = 'seconds' }
  f.enableDelete = Boolean(task.enable_delete_comments)
  return f
}

function unitFactor(u) {
  if (u === 'minutes') return 60
  if (u === 'hours') return 3600
  return 1
}

export default function TasksPage({ embedded = false, actionSlot = null } = {}) {
  const { accounts, loadAccounts } = useAccounts()
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const compact = isMobile || isTablet
  const [tasks, setTasks] = useState([])
  const [loading, setLoading] = useState(false)
  const [expandedId, setExpandedId] = useState(null)
  const [attemptsById, setAttemptsById] = useState({})
  const [busyId, setBusyId] = useState({})

  // Editor state
  const [editorOpen, setEditorOpen] = useState(false)
  const [editingId, setEditingId] = useState(null)
  const [form, setForm] = useState(defaultForm())
  const [saving, setSaving] = useState(false)
  const [editorErr, setEditorErr] = useState('')

  // Targets picker state
  const [targetsLoading, setTargetsLoading] = useState(false)
  const [allTargets, setAllTargets] = useState([])

  const pollRef = useRef(null)

  const activeAccounts = useMemo(
    () => filterBusinessAccounts(accounts),
    [accounts],
  )
  const [authToken, setAuthToken] = useState(() => getAuthToken())
  const activeAccountKey = useMemo(
    () => activeAccounts.map((account) => account.uid).join('|'),
    [activeAccounts],
  )
  const activeAccountsRef = useRef(activeAccounts)
  const activeAccountKeyRef = useRef(activeAccountKey)
  activeAccountsRef.current = activeAccounts
  activeAccountKeyRef.current = activeAccountKey

  function clearTaskState() {
    setTasks([])
    setLoading(false)
    setExpandedId(null)
    setAttemptsById({})
    setBusyId({})
    setEditorOpen(false)
    setEditingId(null)
    setForm(defaultForm())
    setSaving(false)
    setEditorErr('')
    setTargetsLoading(false)
    setAllTargets([])
  }

  function isScopeCurrent(requestToken, requestAccountKey = activeAccountKeyRef.current) {
    return getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey
  }

  function pruneTasksToActiveAccounts(list) {
    const activeUidSet = new Set(activeAccountsRef.current.map((account) => account.uid))
    return list.filter((task) => {
      const taskUids = Array.isArray(task?.account_uids) ? task.account_uids : []
      if (!taskUids.length) return true
      return taskUids.some((uid) => activeUidSet.has(uid))
    })
  }

  async function load() {
    if (!activeAccountsRef.current.length) {
      clearTaskState()
      return
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setLoading(true)
    try {
      const r = await getTasks({ pageSize: 100 })
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setTasks(pruneTasksToActiveAccounts(unwrapList(r)))
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify(e?.message || '加载失败', 'error')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setLoading(false)
    }
  }

  useEffect(() => {
    const syncToken = () => setAuthToken(getAuthToken())
    window.addEventListener('xy:authChanged', syncToken)
    window.addEventListener('storage', syncToken)
    return () => {
      window.removeEventListener('xy:authChanged', syncToken)
      window.removeEventListener('storage', syncToken)
    }
  }, [])

  useEffect(() => {
    clearTaskState()
    void loadAccounts({ force: true })
  }, [authToken])

  useEffect(() => {
    if (!activeAccountKey) {
      clearTaskState()
      return
    }
    void load()
  }, [activeAccountKey, authToken])

  // Poll while any task is running.
  useEffect(() => {
    const hasRunning = tasks.some((t) => t.status === 'running' || t.status === 'queued' || t.status === 'waiting_delete')
    if (hasRunning && !pollRef.current) {
      pollRef.current = setInterval(load, 5000)
    } else if (!hasRunning && pollRef.current) {
      clearInterval(pollRef.current); pollRef.current = null
    }
    return () => { if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null } }
  }, [tasks])

  async function loadTargets() {
    if (!activeAccountsRef.current.length) {
      setAllTargets([])
      return
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setTargetsLoading(true)
    try {
      const r = await getCommentTargets({ pageSize: 200 })
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setAllTargets(unwrapList(r))
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify(e?.message || '加载评论目标失败', 'error')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setTargetsLoading(false)
    }
  }

  function openCreate() {
    setEditingId(null)
    setForm(defaultForm())
    setEditorErr('')
    setEditorOpen(true)
    if (!allTargets.length) loadTargets()
  }

  async function openEdit(task) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setEditingId(task.task_id)
    setEditorErr('')
    try {
      const fresh = await getTask(task.task_id)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setForm(formFromTask(fresh))
    } catch {
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setForm(formFromTask(task))
    }
    setEditorOpen(true)
    if (!allTargets.length) loadTargets()
  }

  async function toggleExpand(task) {
    if (expandedId === task.task_id) { setExpandedId(null); return }
    setExpandedId(task.task_id)
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    try {
      const r = await getTaskAttempts(task.task_id, { limit: 10 })
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setAttemptsById((m) => ({ ...m, [task.task_id]: unwrapList(r) }))
    } catch { /* noop */ }
  }

  async function doStart(task) {
    const taskUids = Array.isArray(task.account_uids) ? task.account_uids : []
    if (taskUids.length && !activeAccountsRef.current.some((account) => taskUids.includes(account.uid))) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setBusyId((m) => ({ ...m, [task.task_id]: true }))
    try {
      await startTask(task.task_id)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      await load()
      notify('任务已启动', 'success')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify(e?.message || '启动失败', 'error')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setBusyId((m) => ({ ...m, [task.task_id]: false }))
    }
  }

  async function doPause(task) {
    const taskUids = Array.isArray(task.account_uids) ? task.account_uids : []
    if (taskUids.length && !activeAccountsRef.current.some((account) => taskUids.includes(account.uid))) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setBusyId((m) => ({ ...m, [task.task_id]: true }))
    try {
      await pauseTask(task.task_id)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      await load()
      notify('任务已暂停', 'success')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify(e?.message || '暂停失败', 'error')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setBusyId((m) => ({ ...m, [task.task_id]: false }))
    }
  }

  async function doDelete(task) {
    const confirmed = await showConfirm(`确认删除任务 “${task.title || task.task_id}”？该操作不可恢复。`, {
      title: '删除任务',
      confirmLabel: '删除',
      danger: true,
      width: 460,
    })
    if (!confirmed) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setBusyId((m) => ({ ...m, [task.task_id]: true }))
    try {
      await deleteTask(task.task_id)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      await load()
      notify('任务已删除', 'success')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify(e?.message || '删除失败', 'error')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setBusyId((m) => ({ ...m, [task.task_id]: false }))
    }
  }

  function setField(key, value) { setForm((f) => ({ ...f, [key]: value })) }
  function toggleAccount(uid) {
    setForm((f) => ({
      ...f,
      account_uids: f.account_uids.includes(uid)
        ? f.account_uids.filter((x) => x !== uid)
        : [...f.account_uids, uid],
    }))
  }
  function toggleTarget(itemId) {
    setForm((f) => ({
      ...f,
      target_item_ids: f.target_item_ids.includes(itemId)
        ? f.target_item_ids.filter((x) => x !== itemId)
        : [...f.target_item_ids, itemId],
    }))
  }

  async function submit() {
    setEditorErr('')
    if (!form.account_uids.length) { setEditorErr('至少选择一个账号'); return }
    const activeUidSet = new Set(activeAccountsRef.current.map((account) => account.uid))
    if (form.account_uids.some((uid) => !activeUidSet.has(uid))) {
      setEditorErr('所选账号已不属于当前账号列表，请重新选择')
      return
    }
    if (!form.target_item_ids.length) { setEditorErr('至少选择一个评论目标'); return }
    const templates = (form.templatesText || '').split('\n').map((s) => s.trim()).filter(Boolean)
    if (!templates.length) { setEditorErr('至少填写一条评论模板'); return }
    const pf = unitFactor(form.publishUnit)
    const df = unitFactor(form.deleteUnit)
    const af = unitFactor(form.deleteAfterUnit)
    const payload = {
      account_uids: form.account_uids,
      target_item_ids: form.target_item_ids,
      title: form.title.trim(),
      comment_templates: templates,
      publish_interval_min_seconds: Number(form.publishMin) * pf,
      publish_interval_max_seconds: Number(form.publishMax) * pf,
      delete_interval_min_seconds: Number(form.deleteMin) * df,
      delete_interval_max_seconds: Number(form.deleteMax) * df,
      delete_after_seconds: Number(form.deleteAfter) * af,
      enable_delete_comments: form.enableDelete,
    }
    if (payload.publish_interval_min_seconds > payload.publish_interval_max_seconds) {
      setEditorErr('发布间隔最小值不能超过最大值'); return
    }
    if (payload.enable_delete_comments && payload.delete_interval_min_seconds > payload.delete_interval_max_seconds) {
      setEditorErr('删评间隔最小值不能超过最大值'); return
    }

    setSaving(true)
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    try {
      if (editingId) {
        await updateTask(editingId, payload)
      } else {
        const created = await createTask(payload)
        // Auto-start newly-created tasks (matches Vue behavior).
        try { await startTask(created.task_id) } catch (e) { console.warn('start task failed', e) }
      }
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setEditorOpen(false)
      await load()
      notify(editingId ? '任务已保存' : '任务已创建', 'success')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) setEditorErr(e.message || '保存失败')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setSaving(false)
    }
  }

  const actions = (
    <div className="flex items-center gap-1">
      <button onClick={load} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40">
        <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
      </button>
      <button onClick={openCreate} className="btn-primary btn-sm">
        <Plus className="w-3.5 h-3.5" /> 新建任务
      </button>
    </div>
  )

  const rootClass = compact
    ? `h-full flex flex-col${embedded ? '' : ' mobile-page-shell'}`
    : 'contents'
  const bodyClass = compact
    ? `flex-1 overflow-y-auto${embedded ? ' px-4 py-4' : ' mobile-page-body'}`
    : 'flex-1 overflow-y-auto px-4 py-4 md:px-6'

  return (
    <div className={rootClass} style={compact ? { backgroundColor: 'var(--bg-page)' } : undefined}>
      {embedded
        ? (actionSlot ? createPortal(actions, actionSlot) : null)
        : <TopBar title="评论任务" right={actions} />}

      <div className={bodyClass}>
        {loading && !tasks.length ? (
          <div className={compact ? 'mobile-soft-card flex items-center justify-center py-24 gap-2' : 'flex items-center justify-center py-24 gap-2'} style={{ color: 'var(--text-secondary)' }}>
            <RefreshCw className="w-4 h-4 animate-spin" /> 加载中…
          </div>
        ) : !tasks.length ? (
          <EmptyState icon={Target} title="暂无评论任务" description="点击右上角新建" />
        ) : (
          <div className={compact ? 'mobile-list-stack' : 'space-y-3'}>
            {tasks.map((task) => {
              const sm = STATUS_MAP[task.status] || { kind: 'neutral', label: task.status }
              const expanded = expandedId === task.task_id
              const editable = EDITABLE_STATUSES.has(task.status)
              const startable = STARTABLE_STATUSES.has(task.status)
              const pausable = PAUSABLE_STATUSES.has(task.status)
              const busy = !!busyId[task.task_id]
              const progress = task.progress
              const total = progress?.total_targets || task.target_total || 0
              const done = progress?.completed_targets || 0
              const percent = total ? Math.min(100, Math.round((done / total) * 100)) : 0
              const attempts = attemptsById[task.task_id] || []
              const dj = task.delete_job_summary || {}
              return (
                <div key={task.task_id} className={compact ? 'mobile-surface-card overflow-hidden' : 'card overflow-hidden'}>
                  <button
                    className={`w-full text-left p-4 flex flex-col gap-2 ${compact ? 'active:bg-transparent' : ''}`}
                    style={compact ? undefined : { '--tw-bg-opacity': '1' }}
                    onClick={() => toggleExpand(task)}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 flex-wrap mb-1">
                          <StatusBadge status={sm.kind} label={sm.label} />
                          <span className="text-[14px] font-semibold truncate" style={{ color: 'var(--text-main)' }}>
                            {task.title || '未命名任务'}
                          </span>
                        </div>
                        <p className="text-[11.5px] font-mono truncate" style={{ color: 'var(--text-muted)' }}>
                          {task.task_id} · 创建 {fmtTime(task.created_at)}
                        </p>
                      </div>
                      {expanded ? <ChevronUp className="w-4 h-4 shrink-0" /> : <ChevronDown className="w-4 h-4 shrink-0" />}
                    </div>

                    <div className="flex flex-wrap gap-x-4 gap-y-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      <span>目标 <b style={{ color: 'var(--text-main)' }}>{total}</b></span>
                      <span>账号 <b style={{ color: 'var(--text-main)' }}>{task.account_total || (task.account_uids || []).length}</b></span>
                      <span>成功 <b style={{ color: 'var(--status-success-text)' }}>{task.attempt_summary?.success || 0}</b></span>
                      <span>失败 <b style={{ color: 'var(--status-danger-text)' }}>{task.attempt_summary?.failed || 0}</b></span>
                      {task.enable_delete_comments && (
                        <span>待删 <b style={{ color: 'var(--primary)' }}>{(dj.pending || 0) + (dj.retryable || 0) + (dj.running || 0)}</b></span>
                      )}
                    </div>

                    <div className="mt-1">
                      <div className="h-1.5 rounded-full overflow-hidden" style={{ backgroundColor: 'var(--border-light)' }}>
                        <div className="h-full rounded-full transition-all"
                          style={{ width: `${percent}%`, backgroundColor: 'var(--primary)' }} />
                      </div>
                      <p className="text-[11px] mt-1" style={{ color: 'var(--text-muted)' }}>
                        进度 {done}/{total} ({percent}%){progress?.last_message ? ` · ${progress.last_message}` : ''}
                      </p>
                    </div>
                  </button>

                  {expanded && (
                    <div className="border-t px-4 py-3 space-y-3" style={{ borderColor: 'var(--border-light)' }}>
                      {task.enable_delete_comments && (
                        <div className={compact ? 'mobile-soft-card p-3' : 'rounded-xl p-3'} style={compact ? undefined : { background: 'var(--surface-soft)' }}>
                          <div className="text-[12px] font-semibold mb-1" style={{ color: 'var(--text-main)' }}>删评队列</div>
                          <div className="flex flex-wrap gap-x-4 gap-y-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                            <span>待执行 <b>{dj.pending || 0}</b></span>
                            <span>运行中 <b>{dj.running || 0}</b></span>
                            <span>可重试 <b>{dj.retryable || 0}</b></span>
                            <span>成功 <b style={{ color: 'var(--status-success-text)' }}>{dj.succeeded || 0}</b></span>
                            <span>失败 <b style={{ color: 'var(--status-danger-text)' }}>{dj.failed || 0}</b></span>
                            <span>取消 <b>{dj.cancelled || 0}</b></span>
                          </div>
                        </div>
                      )}

                      <div>
                        <div className="text-[12px] font-semibold mb-1" style={{ color: 'var(--text-main)' }}>最近评论尝试</div>
                        {attempts.length ? (
                          <div className="space-y-1.5 max-h-56 overflow-y-auto">
                            {attempts.map((a) => (
                              <div key={a.id} className="flex items-start gap-2 text-[12px] py-1.5 border-b last:border-0"
                                style={{ borderColor: 'var(--border-light)' }}>
                                {a.success
                                  ? <CheckCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" style={{ color: 'var(--status-success-text)' }} />
                                  : <XCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" style={{ color: 'var(--status-danger-text)' }} />}
                                <div className="flex-1 min-w-0">
                                  <p className="truncate" style={{ color: 'var(--text-main)' }}>
                                    “{a.comment_content || '—'}”
                                  </p>
                                  <p className="text-[11px] truncate font-mono" style={{ color: 'var(--text-muted)' }}>
                                    {a.account_uid} · {a.item_id} · {fmtTime(a.created_at)}
                                    {!a.success && a.failure_reason_message ? ` · ${a.failure_reason_message}` : ''}
                                  </p>
                                </div>
                              </div>
                            ))}
                          </div>
                        ) : (
                          <p className="text-[12px]" style={{ color: 'var(--text-muted)' }}>暂无尝试记录</p>
                        )}
                      </div>

                      {task.last_error && (
                        <div className="liquid-notice liquid-notice-danger rounded-xl px-3 py-2 text-[12px]">
                          {task.last_error}
                        </div>
                      )}

                      <div className="flex flex-wrap gap-2 pt-1">
                        {startable && (
                          <button onClick={(e) => { e.stopPropagation(); doStart(task) }}
                            disabled={busy} className="btn-primary btn-sm disabled:opacity-40">
                            <Play className="w-3.5 h-3.5" /> {task.status === 'paused' ? '继续' : '启动'}
                          </button>
                        )}
                        {pausable && (
                          <button onClick={(e) => { e.stopPropagation(); doPause(task) }}
                            disabled={busy} className="btn-secondary btn-sm disabled:opacity-40">
                            <Pause className="w-3.5 h-3.5" /> 暂停
                          </button>
                        )}
                        <button onClick={(e) => { e.stopPropagation(); openEdit(task) }}
                          disabled={!editable || busy}
                          title={editable ? '编辑' : '仅 草稿/已暂停/失败 状态可编辑'}
                          className="btn-secondary btn-sm disabled:opacity-40">
                          <Pencil className="w-3.5 h-3.5" /> 编辑
                        </button>
                        <button onClick={(e) => { e.stopPropagation(); doDelete(task) }}
                          disabled={busy} className="btn-secondary btn-sm disabled:opacity-40 ml-auto"
                          style={{ color: 'var(--status-danger-text)' }}>
                          <Trash2 className="w-3.5 h-3.5" /> 删除
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>

      <Modal open={editorOpen} onClose={() => setEditorOpen(false)}
        title={editingId ? '编辑任务' : '新建评论任务'} width={560}>
        <div className="space-y-4">
          <label className="block">
            <span className="label">任务标题</span>
            <input className="input" value={form.title}
              onChange={(e) => setField('title', e.target.value)}
              placeholder="便于识别的任务名（可选）" />
          </label>

          <div>
            <span className="label">使用账号 ({form.account_uids.length} 已选)</span>
            {!activeAccounts.length ? (
              <p className="text-[12px]" style={{ color: 'var(--text-muted)' }}>暂无活跃账号，先在账号管理登录账号</p>
            ) : (
              <div className="flex flex-wrap gap-2">
                {activeAccounts.map((a) => {
                  const on = form.account_uids.includes(a.uid)
                  const score = Number(a.security_score ?? -1)
                  const restricted = score >= 0 && score < 81
                  return (
                    <button key={a.uid} type="button"
                      onClick={() => toggleAccount(a.uid)}
                      disabled={restricted}
                      title={restricted ? `安全分 ${score}，无法评论` : ''}
                      className="px-3 py-1.5 rounded-xl text-[12px] font-medium border transition-colors disabled:opacity-40"
                      style={on
                        ? { background: 'var(--primary-light)', borderColor: 'var(--primary)', color: 'var(--primary)' }
                        : { background: 'var(--surface-soft)', borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}>
                      {a.nick || a.uid}{restricted ? ` (${score})` : ''}
                    </button>
                  )
                })}
              </div>
            )}
            <p className="helper-text mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
              提示：安全分 &lt; 81 的账号无法发布评论，已被禁用
            </p>
          </div>

          <div>
            <div className="flex items-center justify-between mb-1">
              <span className="label mb-0">评论目标 ({form.target_item_ids.length} 已选)</span>
              <button onClick={loadTargets} disabled={targetsLoading}
                className="text-[12px]" style={{ color: 'var(--primary)' }}>
                {targetsLoading ? '加载中…' : '刷新'}
              </button>
            </div>
            <div className="rounded-xl border max-h-60 overflow-y-auto"
              style={{ borderColor: 'var(--border-light)' }}>
              {targetsLoading && !allTargets.length ? (
                <p className="p-3 text-[12px]" style={{ color: 'var(--text-muted)' }}>加载中…</p>
              ) : !allTargets.length ? (
                <p className="p-3 text-[12px]" style={{ color: 'var(--text-muted)' }}>
                  评论目标池为空，请先在“评论目标池”页面添加
                </p>
              ) : (
                <div className="divide-y" style={{ borderColor: 'var(--border-light)' }}>
                  {allTargets.map((t) => {
                    const itemId = String(t.item_id || t.itemId || '')
                    const on = form.target_item_ids.includes(itemId)
                    return (
                      <label key={itemId} className="flex items-center gap-2 px-3 py-2 cursor-pointer">
                        <input type="checkbox" checked={on}
                          onChange={() => toggleTarget(itemId)}
                          className="h-4 w-4"
                          style={{ accentColor: 'var(--primary)' }} />
                        <div className="flex-1 min-w-0">
                          <p className="text-[12.5px] truncate" style={{ color: 'var(--text-main)' }}>
                            {t.title || itemId}
                          </p>
                          <p className="text-[11px] font-mono truncate" style={{ color: 'var(--text-muted)' }}>
                            {itemId}{t.price ? ` · ¥${t.price}` : ''}{t.seller_nick ? ` · ${t.seller_nick}` : ''}
                          </p>
                        </div>
                      </label>
                    )
                  })}
                </div>
              )}
            </div>
          </div>

          <label className="block">
            <span className="label">评论模板（每行一条）</span>
            <textarea className="input min-h-[100px] resize-y" value={form.templatesText}
              onChange={(e) => setField('templatesText', e.target.value)}
              placeholder="你好请问还在吗&#10;可以包邮吗" />
          </label>

          <div>
            <span className="label">发布间隔</span>
            <div className="grid grid-cols-3 gap-2">
              <input type="number" min="0" className="input" value={form.publishMin}
                onChange={(e) => setField('publishMin', e.target.value)} placeholder="最小" />
              <input type="number" min="0" className="input" value={form.publishMax}
                onChange={(e) => setField('publishMax', e.target.value)} placeholder="最大" />
              <select className="input" value={form.publishUnit}
                onChange={(e) => setField('publishUnit', e.target.value)}>
                <option value="seconds">秒</option>
                <option value="minutes">分钟</option>
              </select>
            </div>
          </div>

          <label className="flex items-center justify-between rounded-xl border px-4 py-3 text-[13px] cursor-pointer"
            style={{ borderColor: 'var(--border-light)' }}>
            <span style={{ color: 'var(--text-main)' }}>启用自动删评</span>
            <input type="checkbox" checked={form.enableDelete}
              onChange={(e) => setField('enableDelete', e.target.checked)}
              className="h-4 w-4"
              style={{ accentColor: 'var(--primary)' }} />
          </label>

          {form.enableDelete && (
            <>
              <div>
                <span className="label">删评间隔</span>
                <div className="grid grid-cols-3 gap-2">
                  <input type="number" min="0" className="input" value={form.deleteMin}
                    onChange={(e) => setField('deleteMin', e.target.value)} placeholder="最小" />
                  <input type="number" min="0" className="input" value={form.deleteMax}
                    onChange={(e) => setField('deleteMax', e.target.value)} placeholder="最大" />
                  <select className="input" value={form.deleteUnit}
                    onChange={(e) => setField('deleteUnit', e.target.value)}>
                    <option value="seconds">秒</option>
                    <option value="minutes">分钟</option>
                  </select>
                </div>
              </div>
              <div>
                <span className="label">发布后多久开始删评</span>
                <div className="grid grid-cols-2 gap-2">
                  <input type="number" min="0" className="input" value={form.deleteAfter}
                    onChange={(e) => setField('deleteAfter', e.target.value)} />
                  <select className="input" value={form.deleteAfterUnit}
                    onChange={(e) => setField('deleteAfterUnit', e.target.value)}>
                    <option value="seconds">秒</option>
                    <option value="minutes">分钟</option>
                    <option value="hours">小时</option>
                  </select>
                </div>
              </div>
            </>
          )}

          {editorErr && (
            <p className="liquid-notice liquid-notice-danger rounded-xl px-4 py-2.5 text-[12.5px]">
              {editorErr}
            </p>
          )}

          <div className="flex gap-2 pt-1">
            <button onClick={() => setEditorOpen(false)} className="btn-secondary flex-1">取消</button>
            <button onClick={submit} disabled={saving}
              className="btn-primary flex-1 disabled:opacity-40">
              {saving ? '保存中…' : (editingId ? '保存' : '创建并启动')}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
