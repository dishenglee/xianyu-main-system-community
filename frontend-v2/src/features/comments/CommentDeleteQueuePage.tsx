// @ts-nocheck
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { RefreshCw, Trash2, RotateCw, XCircle } from 'lucide-react'
import {
  getTaskDeleteJobs, retryTaskDeleteJob, cancelTaskDeleteJob,
} from '../../api/index'
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { StatusBadge } from '../../components/StatusBadge'
import { useSafeNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import { useDevice } from '../../hooks/useDevice'
import { useAuthScope } from '../../hooks/useAuthScope'

const STATUS_FILTERS = [
  { key: '', label: '全部' },
  { key: 'pending', label: '待执行' },
  { key: 'running', label: '运行中' },
  { key: 'retryable', label: '可重试' },
  { key: 'succeeded', label: '已成功' },
  { key: 'failed', label: '失败' },
  { key: 'cancelled', label: '已取消' },
  { key: 'already_removed', label: '已移除' },
  { key: 'item_gone', label: '商品消失' },
]

const STATUS_KIND = {
  pending: 'warning', running: 'info', retryable: 'warning',
  succeeded: 'success', failed: 'danger', cancelled: 'neutral',
  already_removed: 'neutral', item_gone: 'neutral',
}

const RETRYABLE_STATUSES = new Set(['retryable', 'failed'])
const CANCELLABLE_STATUSES = new Set(['pending', 'retryable'])

function fmtTime(ts) {
  if (!ts) return '—'
  const ms = ts > 1e12 ? ts : ts * 1000
  return new Date(ms).toLocaleString('zh-CN')
}

export default function CommentDeleteQueuePage({ embedded = false, actionSlot = null } = {}) {
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const compact = isMobile || isTablet
  const [items, setItems] = useState([])
  const [pagination, setPagination] = useState({ page: 1, page_size: 30, total: 0 })
  const [status, setStatus] = useState('')
  const [accountUid, setAccountUid] = useState('')
  const [itemId, setItemId] = useState('')
  const [loading, setLoading] = useState(false)
  const [busyId, setBusyId] = useState({})
  const loadSeqRef = useRef(0)
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    setItems([])
    setPagination({ page: 1, page_size: 30, total: 0 })
    setBusyId({})
    setLoading(false)
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  async function load(page = 1) {
    const requestKey = `${status}|${accountUid}|${itemId}`
    const scope = captureScope(requestKey)
    const loadSeq = ++loadSeqRef.current
    const isCurrent = () => isScopeCurrent(scope, requestKey) && loadSeqRef.current === loadSeq
    setLoading(true)
    try {
      const r = await getTaskDeleteJobs({
        status, accountUid, itemId, page, pageSize: 30,
      })
      if (!isCurrent()) return
      const list = Array.isArray(r?.items) ? r.items : []
      setItems(page === 1 ? list : (prev) => [...prev, ...list])
      setPagination(r?.pagination || { page, page_size: 30, total: list.length })
    } catch (e) { if (isCurrent()) notify(e?.message || '加载失败', 'error') }
    finally { if (isCurrent()) setLoading(false) }
  }

  useEffect(() => { load(1) }, [status, accountUid, itemId, authToken])

  async function doRetry(j) {
    const confirmed = await showConfirm(`重试删评任务 #${j.id}？`, {
      title: '重试删评任务',
      confirmLabel: '确认重试',
    })
    if (!confirmed) return
    setBusyId((m) => ({ ...m, [j.id]: true }))
    const scope = captureScope()
    try {
      await retryTaskDeleteJob(j.id)
      if (!isScopeCurrent(scope)) return
      await load(1)
      notify('删评任务已重试', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '重试失败', 'error') }
    finally { if (isScopeCurrent(scope)) setBusyId((m) => ({ ...m, [j.id]: false })) }
  }

  async function doCancel(j) {
    const confirmed = await showConfirm(`取消删评任务 #${j.id}？`, {
      title: '取消删评任务',
      confirmLabel: '确认取消',
      danger: true,
    })
    if (!confirmed) return
    setBusyId((m) => ({ ...m, [j.id]: true }))
    const scope = captureScope()
    try {
      await cancelTaskDeleteJob(j.id)
      if (!isScopeCurrent(scope)) return
      await load(1)
      notify('删评任务已取消', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '取消失败', 'error') }
    finally { if (isScopeCurrent(scope)) setBusyId((m) => ({ ...m, [j.id]: false })) }
  }

  const hasMore = items.length < (pagination?.total || 0)

  const actions = (
    <button onClick={() => load(1)} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40">
      <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
    </button>
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
        : <TopBar title="删评队列" right={actions} />}

      <div className={compact ? `px-4 ${embedded ? 'pt-4 pb-3' : 'py-3'} space-y-3 shrink-0` : 'px-4 py-3 space-y-2 shrink-0 border-b'} style={compact ? undefined : { borderColor: 'var(--border-light)' }}>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
          <input className="input" value={accountUid} onChange={(e) => setAccountUid(e.target.value)}
            placeholder="账号 UID（可选）" />
          <input className="input" value={itemId} onChange={(e) => setItemId(e.target.value)}
            placeholder="商品 ID（可选）" />
        </div>
        <div className={compact ? 'mobile-toolbar-scroll' : 'flex gap-1.5 overflow-x-auto -mx-1 px-1 py-0.5'}>
          {STATUS_FILTERS.map((f) => (
            <button key={f.key} onClick={() => setStatus(f.key)}
              className={compact
                ? `mobile-pill shrink-0 ${status === f.key ? '' : 'mobile-muted-pill'}`
                : `px-3 py-1.5 rounded-full text-[12px] whitespace-nowrap shrink-0 ${status === f.key ? 'text-white' : ''}`}
              style={compact
                ? (status === f.key ? { color: 'var(--primary)', background: 'var(--primary-light)' } : { color: 'var(--text-secondary)' })
                : (status === f.key ? { background: 'var(--primary)' } : { background: 'var(--surface-soft)', color: 'var(--text-secondary)' })}>
              {f.label}
            </button>
          ))}
        </div>
      </div>

      <div className={bodyClass}>
        {loading && !items.length ? (
          <div className={compact ? 'mobile-soft-card flex items-center justify-center py-24 gap-2' : 'flex items-center justify-center py-24 gap-2'} style={{ color: 'var(--text-secondary)' }}>
            <RefreshCw className="w-4 h-4 animate-spin" /> 加载中…
          </div>
        ) : !items.length ? (
          <EmptyState icon={Trash2} title="删评队列为空" description="暂无待处理项" />
        ) : (
          <div className={compact ? 'mobile-list-stack' : 'space-y-2'}>
            {items.map((j) => {
              const kind = STATUS_KIND[j.status] || 'neutral'
              const canRetry = RETRYABLE_STATUSES.has(j.status)
              const canCancel = CANCELLABLE_STATUSES.has(j.status)
              const busy = !!busyId[j.id]
              return (
                <div key={j.id} className={compact ? 'mobile-soft-card p-3.5' : 'card p-3'}>
                  <div className="flex items-center gap-2 flex-wrap">
                    <StatusBadge status={kind} label={j.status} />
                    <span className="text-[12px] font-mono" style={{ color: 'var(--text-muted)' }}>#{j.id}</span>
                    {j.retry_count > 0 && (
                      <span className="liquid-notice liquid-notice-warning text-[11px] px-1.5 py-0.5 rounded">
                        重试 {j.retry_count}
                      </span>
                    )}
                  </div>
                  <p className="text-[12px] font-mono mt-1.5 truncate" style={{ color: 'var(--text-secondary)' }}>
                    {j.account_uid || '—'} · 商品 {j.item_id || '—'}
                  </p>
                  <p className="text-[11px] font-mono truncate" style={{ color: 'var(--text-muted)' }}>
                    评论 {j.comment_id || '—'} · 任务 {j.task_id || '—'}
                  </p>
                  <p className="text-[11px] mt-1" style={{ color: 'var(--text-muted)' }}>
                    创建 {fmtTime(j.created_at)}
                    {j.scheduled_at ? ` · 调度 ${fmtTime(j.scheduled_at)}` : ''}
                    {j.finished_at ? ` · 完成 ${fmtTime(j.finished_at)}` : ''}
                  </p>
                  {j.last_error_message && (
                    <p className="liquid-notice liquid-notice-danger rounded-lg mt-2 px-2.5 py-1.5 text-[11.5px]">
                      {j.last_error_code ? `[${j.last_error_code}] ` : ''}{j.last_error_message}
                    </p>
                  )}
                  {(canRetry || canCancel) && (
                    <div className="flex gap-2 mt-2">
                      {canRetry && (
                        <button onClick={() => doRetry(j)} disabled={busy}
                          className="btn-secondary btn-sm disabled:opacity-40">
                          <RotateCw className="w-3.5 h-3.5" /> 重试
                        </button>
                      )}
                      {canCancel && (
                        <button onClick={() => doCancel(j)} disabled={busy}
                          className="btn-secondary btn-sm disabled:opacity-40"
                          style={{ color: 'var(--status-danger-text)' }}>
                          <XCircle className="w-3.5 h-3.5" /> 取消
                        </button>
                      )}
                    </div>
                  )}
                </div>
              )
            })}
            {hasMore && (
              <button onClick={() => load((pagination?.page || 1) + 1)}
                disabled={loading} className="btn-secondary w-full disabled:opacity-40">
                {loading ? '加载中…' : '加载更多'}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
