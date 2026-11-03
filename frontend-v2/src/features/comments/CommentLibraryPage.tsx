// @ts-nocheck
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Plus, RefreshCw, MessageSquare, Power } from 'lucide-react'
import {
  getCommentLibrary, createCommentLibraryEntry, batchCreateCommentLibrary,
  setCommentLibraryEnabled,
} from '../../api/index'
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { Modal } from '../../components/Modal'
import { useSafeNotify } from '../../components/Notify'
import { useDevice } from '../../hooks/useDevice'
import { useAuthScope } from '../../hooks/useAuthScope'

const ENABLED_FILTERS = [
  { key: 'all', label: '全部' },
  { key: 'enabled', label: '已启用' },
  { key: 'disabled', label: '已停用' },
]

function unwrapList(r) {
  if (!r) return []
  if (Array.isArray(r)) return r
  if (Array.isArray(r.items)) return r.items
  if (Array.isArray(r.data)) return r.data
  return []
}

export default function CommentLibraryPage({ embedded = false, actionSlot = null } = {}) {
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const compact = isMobile || isTablet
  const [items, setItems] = useState([])
  const [summary, setSummary] = useState({})
  const [filter, setFilter] = useState('all')
  const [loading, setLoading] = useState(false)
  const [pagination, setPagination] = useState({ page: 1, page_size: 50, total: 0 })

  const [addOpen, setAddOpen] = useState(false)
  const [addText, setAddText] = useState('')
  const [adding, setAdding] = useState(false)
  const [addErr, setAddErr] = useState('')
  const [busyHash, setBusyHash] = useState({})
  const loadSeqRef = useRef(0)
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    setItems([])
    setSummary({})
    setPagination({ page: 1, page_size: 50, total: 0 })
    setLoading(false)
    setAdding(false)
    setBusyHash({})
    setAddOpen(false)
    setAddErr('')
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  async function load(page = 1) {
    const scope = captureScope(filter)
    const loadSeq = ++loadSeqRef.current
    const isCurrent = () => isScopeCurrent(scope, filter) && loadSeqRef.current === loadSeq
    setLoading(true)
    try {
      const enabled = filter === 'all' ? null : filter === 'enabled'
      const r = await getCommentLibrary({ enabled, page, pageSize: 50 })
      if (!isCurrent()) return
      const list = unwrapList(r)
      setItems(page === 1 ? list : (prev) => [...prev, ...list])
      setSummary(r?.summary || {})
      setPagination(r?.pagination || { page, page_size: 50, total: list.length })
    } catch (e) { if (isCurrent()) notify(e?.message || '加载失败', 'error') }
    finally { if (isCurrent()) setLoading(false) }
  }

  useEffect(() => { load(1) }, [filter, authToken])

  async function submitAdd() {
    setAddErr('')
    const lines = (addText || '').split('\n').map((s) => s.trim()).filter(Boolean)
    if (!lines.length) { setAddErr('请输入至少一条评论'); return }
    setAdding(true)
    const scope = captureScope()
    try {
      if (lines.length === 1) {
        await createCommentLibraryEntry({ content: lines[0] })
      } else {
        await batchCreateCommentLibrary(lines)
      }
      if (!isScopeCurrent(scope)) return
      setAddOpen(false); setAddText(''); await load(1)
      notify(lines.length === 1 ? '评论已新增' : `已导入 ${lines.length} 条评论`, 'success')
    } catch (e) { if (isScopeCurrent(scope)) setAddErr(e.message || '保存失败') }
    finally { if (isScopeCurrent(scope)) setAdding(false) }
  }

  async function toggleEnabled(it) {
    setBusyHash((m) => ({ ...m, [it.content_hash]: true }))
    const scope = captureScope()
    try {
      await setCommentLibraryEnabled({ contentHash: it.content_hash, enabled: !it.enabled })
      if (!isScopeCurrent(scope)) return
      await load(1)
      notify(it.enabled ? '评论已停用' : '评论已启用', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '切换失败', 'error') }
    finally { if (isScopeCurrent(scope)) setBusyHash((m) => ({ ...m, [it.content_hash]: false })) }
  }

  const hasMore = items.length < (pagination?.total || 0)

  const actions = (
    <div className="flex items-center gap-1">
      <button onClick={() => load(1)} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40">
        <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
      </button>
      <button onClick={() => { setAddText(''); setAddErr(''); setAddOpen(true) }} className="btn-primary btn-sm">
        <Plus className="w-3.5 h-3.5" /> 新增
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
        : <TopBar title="评论内容库" right={actions} />}

      <div className={compact ? `px-4 ${embedded ? 'pt-4 pb-3' : 'py-3'} shrink-0 mobile-stat-grid` : 'px-4 py-2 border-b shrink-0 flex flex-wrap gap-x-4 gap-y-1 text-[12px]'}
        style={compact ? undefined : { borderColor: 'var(--border-light)', color: 'var(--text-secondary)' }}>
        {compact ? (
          <>
            <div className="mobile-stat-card"><div className="mobile-stat-label">总数</div><div className="mobile-stat-value text-[20px]">{summary.total || 0}</div></div>
            <div className="mobile-stat-card"><div className="mobile-stat-label">已启用</div><div className="mobile-stat-value text-[20px]" style={{ color: 'var(--status-success-text)' }}>{summary.enabled || 0}</div></div>
            <div className="mobile-stat-card"><div className="mobile-stat-label">已停用</div><div className="mobile-stat-value text-[20px]">{summary.disabled || 0}</div></div>
            <div className="mobile-stat-card"><div className="mobile-stat-label">使用次数</div><div className="mobile-stat-value text-[20px]" style={{ color: 'var(--primary)' }}>{summary.usage_count || 0}</div></div>
          </>
        ) : (
          <>
        <span>总数 <b style={{ color: 'var(--text-main)' }}>{summary.total || 0}</b></span>
        <span>已启用 <b style={{ color: 'var(--status-success-text)' }}>{summary.enabled || 0}</b></span>
        <span>已停用 <b style={{ color: 'var(--text-muted)' }}>{summary.disabled || 0}</b></span>
        <span>使用次数 <b style={{ color: 'var(--primary)' }}>{summary.usage_count || 0}</b></span>
          </>
        )}
      </div>

      <div className={compact ? 'mobile-toolbar-scroll px-4 pb-3 shrink-0' : 'flex border-b overflow-x-auto px-4 shrink-0'} style={compact ? undefined : { borderColor: 'var(--border-light)' }}>
        {ENABLED_FILTERS.map((f) => (
          <button key={f.key} onClick={() => setFilter(f.key)}
            className={compact
              ? `mobile-pill shrink-0 ${filter === f.key ? '' : 'mobile-muted-pill'}`
              : `px-3 py-2.5 text-[12.5px] whitespace-nowrap border-b-2 ${filter === f.key ? 'border-[var(--primary)] text-[var(--primary)] font-semibold' : 'border-transparent'}`}
            style={compact
              ? (filter === f.key ? { color: 'var(--primary)', background: 'var(--primary-light)' } : { color: 'var(--text-secondary)' })
              : (filter !== f.key ? { color: 'var(--text-secondary)' } : {})}>
            {f.label}
          </button>
        ))}
      </div>

      <div className={bodyClass}>
        {loading && !items.length ? (
          <div className={compact ? 'mobile-soft-card flex items-center justify-center py-24 gap-2' : 'flex items-center justify-center py-24 gap-2'} style={{ color: 'var(--text-secondary)' }}>
            <RefreshCw className="w-4 h-4 animate-spin" /> 加载中…
          </div>
        ) : !items.length ? (
          <EmptyState icon={MessageSquare} title="评论库为空" description="点击右上角新增" />
        ) : (
          <div className={compact ? 'mobile-list-stack' : 'space-y-2'}>
            {items.map((it) => (
              <div key={it.content_hash || it.id} className={compact ? 'mobile-soft-card p-3.5 flex items-start gap-2' : 'card p-3 flex items-start gap-2'}>
                <div className="flex-1 min-w-0">
                  <p className="text-[13.5px]" style={{ color: it.enabled ? 'var(--text-main)' : 'var(--text-muted)' }}>
                    {it.content}
                  </p>
                  <p className="text-[11px] mt-1" style={{ color: 'var(--text-muted)' }}>
                    使用 {it.usage_count || 0} 次 · 成功 {it.success_count || 0} 次
                    {it.source_label ? ` · ${it.source_label}` : ''}
                  </p>
                </div>
                <button onClick={() => toggleEnabled(it)}
                  disabled={!!busyHash[it.content_hash]}
                  className="btn-sm shrink-0"
                  title={it.enabled ? '停用' : '启用'}
                  style={{ color: it.enabled ? 'var(--status-success-text)' : 'var(--text-muted)' }}>
                  <Power className="w-4 h-4" />
                </button>
              </div>
            ))}
            {hasMore && (
              <button onClick={() => load((pagination?.page || 1) + 1)}
                disabled={loading} className="btn-secondary w-full disabled:opacity-40">
                {loading ? '加载中…' : '加载更多'}
              </button>
            )}
          </div>
        )}
      </div>

      <Modal open={addOpen} onClose={() => setAddOpen(false)} title="新增评论">
        <div className="space-y-3">
          <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
            每行一条评论，重复内容自动去重。
          </p>
          <textarea className="input min-h-[180px] resize-y"
            value={addText} onChange={(e) => setAddText(e.target.value)}
            placeholder="你好请问还在吗&#10;可以包邮吗&#10;请问还能优惠吗" />
          {addErr && (
            <p className="liquid-notice liquid-notice-danger rounded-xl px-4 py-2.5 text-[12.5px]">
              {addErr}
            </p>
          )}
          <div className="flex gap-2">
            <button onClick={() => setAddOpen(false)} className="btn-secondary flex-1">取消</button>
            <button onClick={submitAdd} disabled={adding}
              className="btn-primary flex-1 disabled:opacity-40">
              {adding ? '保存中…' : '保存'}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
