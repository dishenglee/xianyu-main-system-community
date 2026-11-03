// @ts-nocheck
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Plus, RefreshCw, Target } from 'lucide-react'
import { getCommentTargets, batchAddCommentTargets } from '../../api/index'
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { Modal } from '../../components/Modal'
import { StatusBadge } from '../../components/StatusBadge'
import { useSafeNotify } from '../../components/Notify'
import { useDevice } from '../../hooks/useDevice'
import { useAuthScope } from '../../hooks/useAuthScope'

const STATUS_FILTERS = [
  { key: '', label: '全部' },
  { key: 'pending', label: '待评论' },
  { key: 'commenting', label: '评论中' },
  { key: 'done', label: '已完成' },
  { key: 'failed', label: '失败' },
]

function unwrapList(r) {
  if (!r) return []
  if (Array.isArray(r)) return r
  if (Array.isArray(r.items)) return r.items
  if (Array.isArray(r.data)) return r.data
  return []
}

function statusKind(s) {
  if (s === 'done') return 'success'
  if (s === 'failed') return 'danger'
  if (s === 'commenting') return 'info'
  return 'neutral'
}

export default function CommentTargetsPage({ embedded = false, actionSlot = null } = {}) {
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const compact = isMobile || isTablet
  const [items, setItems] = useState([])
  const [status, setStatus] = useState('')
  const [loading, setLoading] = useState(false)
  const [pagination, setPagination] = useState({ page: 1, page_size: 50, total: 0 })

  const [addOpen, setAddOpen] = useState(false)
  const [addText, setAddText] = useState('')
  const [adding, setAdding] = useState(false)
  const [addErr, setAddErr] = useState('')
  const loadSeqRef = useRef(0)
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    setItems([])
    setPagination({ page: 1, page_size: 50, total: 0 })
    setLoading(false)
    setAdding(false)
    setAddOpen(false)
    setAddErr('')
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  async function load(page = 1) {
    const scope = captureScope()
    const loadSeq = ++loadSeqRef.current
    const isCurrent = () => isScopeCurrent(scope) && loadSeqRef.current === loadSeq
    setLoading(true)
    try {
      const r = await getCommentTargets({ status, page, pageSize: 50 })
      if (!isCurrent()) return
      const list = unwrapList(r)
      setItems(page === 1 ? list : (prev) => [...prev, ...list])
      setPagination(r?.pagination || { page, page_size: 50, total: list.length })
    } catch (e) { if (isCurrent()) notify(e?.message || '加载失败', 'error') }
    finally { if (isCurrent()) setLoading(false) }
  }

  useEffect(() => { load(1) }, [status, authToken])

  async function submitAdd() {
    setAddErr('')
    const lines = (addText || '').split(/[\n,，\s]+/).map((s) => s.trim()).filter(Boolean)
    if (!lines.length) { setAddErr('请输入至少一个 item_id'); return }
    const payload = lines.map((line) => {
      // Allow user to paste tab-separated `item_id<TAB>title`
      const [id, title = ''] = line.split(/\t/)
      return { item_id: id, title, source: 'manual', status: 'pending' }
    })
    setAdding(true)
    const scope = captureScope()
    try {
      const r = await batchAddCommentTargets(payload)
      if (!isScopeCurrent(scope)) return
      setAddOpen(false)
      setAddText('')
      notify(`已加入 ${r?.added ?? 0} 条，去重 ${r?.deduplicated ?? 0} 条`, 'success', 3200)
      await load(1)
    } catch (e) { if (isScopeCurrent(scope)) setAddErr(e.message || '加入失败') }
    finally { if (isScopeCurrent(scope)) setAdding(false) }
  }

  const hasMore = items.length < (pagination?.total || 0)

  const actions = (
    <div className="flex items-center gap-1">
      <button onClick={() => load(1)} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40">
        <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
      </button>
      <button onClick={() => { setAddText(''); setAddErr(''); setAddOpen(true) }} className="btn-primary btn-sm">
        <Plus className="w-3.5 h-3.5" /> 加入
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
        : <TopBar title="评论目标池" right={actions} />}

      <div className={compact ? `mobile-toolbar-scroll shrink-0 px-4 ${embedded ? 'pt-4 pb-3' : 'py-3'}` : 'flex border-b overflow-x-auto px-4 shrink-0'} style={compact ? undefined : { borderColor: 'var(--border-light)' }}>
        {STATUS_FILTERS.map((f) => (
          <button key={f.key} onClick={() => setStatus(f.key)}
            className={compact
              ? `mobile-pill shrink-0 ${status === f.key ? '' : 'mobile-muted-pill'}`
              : `px-3 py-2.5 text-[12.5px] whitespace-nowrap border-b-2 ${status === f.key ? 'border-[var(--primary)] text-[var(--primary)] font-semibold' : 'border-transparent'}`}
            style={compact
              ? (status === f.key ? { color: 'var(--primary)', background: 'var(--primary-light)' } : { color: 'var(--text-secondary)' })
              : (status !== f.key ? { color: 'var(--text-secondary)' } : {})}>
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
          <EmptyState icon={Target} title="评论目标池为空" description="点击右上角加入" />
        ) : (
          <div className={compact ? 'mobile-list-stack' : 'space-y-2'}>
            {items.map((it, i) => (
              <div key={(it.item_id || it.itemId || i) + '-' + i} className={compact ? 'mobile-soft-card p-3.5' : 'card p-3'}>
                <div className="flex items-start gap-3">
                  {it.pic_url && (
                    <img src={it.pic_url} alt="" className="w-12 h-12 rounded-lg object-cover shrink-0" />
                  )}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <StatusBadge status={statusKind(it.status)} label={it.status || 'pending'} />
                      <p className="text-[13px] font-medium truncate" style={{ color: 'var(--text-main)' }}>
                        {it.title || it.item_id}
                      </p>
                    </div>
                    <p className="text-[11.5px] font-mono truncate mt-1" style={{ color: 'var(--text-muted)' }}>
                      {it.item_id || it.itemId}
                      {it.price ? ` · ¥${it.price}` : ''}
                      {it.seller_nick ? ` · ${it.seller_nick}` : ''}
                    </p>
                    {it.item_url && (
                      <a href={it.item_url} target="_blank" rel="noreferrer"
                        className="text-[11.5px]" style={{ color: 'var(--primary)' }}>
                        查看商品 ↗
                      </a>
                    )}
                  </div>
                </div>
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

      <Modal open={addOpen} onClose={() => setAddOpen(false)} title="批量加入评论目标">
        <div className="space-y-3">
          <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
            每行一个 item_id（可选 Tab 分隔标题），逗号或空格也可。
          </p>
          <textarea className="input min-h-[160px] font-mono text-[12px] resize-y"
            value={addText} onChange={(e) => setAddText(e.target.value)}
            placeholder="901234567890&#10;901234567891&#9;商品标题示例" />
          {addErr && (
            <p className="liquid-notice liquid-notice-danger rounded-xl px-4 py-2.5 text-[12.5px]">
              {addErr}
            </p>
          )}
          <div className="flex gap-2">
            <button onClick={() => setAddOpen(false)} className="btn-secondary flex-1">取消</button>
            <button onClick={submitAdd} disabled={adding}
              className="btn-primary flex-1 disabled:opacity-40">
              {adding ? '加入中…' : '加入目标池'}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
