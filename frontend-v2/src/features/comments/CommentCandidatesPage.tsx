// @ts-nocheck
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { RefreshCw, MessageSquare, Search, ArrowUpRight } from 'lucide-react'
import {
  getCommentCandidates, batchAddCommentTargets, harvestCommentCandidates,
} from '../../api/index'
import { useAccounts } from '../../hooks/useAccounts'
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { Modal } from '../../components/Modal'
import { StatusBadge } from '../../components/StatusBadge'
import { useSafeNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import { useDevice } from '../../hooks/useDevice'
import { useAuthScope } from '../../hooks/useAuthScope'
import { filterBusinessAccounts } from '../../utils/accountAccess'

const QUALITY_OPTIONS = [
  { key: '', label: '全部质量' },
  { key: 'good', label: '优质' },
  { key: 'review', label: '待审' },
  { key: 'bad', label: '低质' },
]

const CATEGORY_OPTIONS = [
  { key: '', label: '全部类别' },
  { key: '物流', label: '物流' },
  { key: '包装', label: '包装' },
  { key: '商品', label: '商品' },
  { key: '服务', label: '服务' },
  { key: '价格', label: '价格' },
  { key: '其他', label: '其他' },
]

function unwrapList(r) {
  if (!r) return []
  if (Array.isArray(r)) return r
  if (Array.isArray(r.items)) return r.items
  if (Array.isArray(r.data)) return r.data
  return []
}

function qualityKind(q) {
  if (q === 'good') return 'success'
  if (q === 'bad') return 'danger'
  if (q === 'review') return 'warning'
  return 'neutral'
}

export default function CommentCandidatesPage({ embedded = false, actionSlot = null } = {}) {
  const { accounts, loadAccounts } = useAccounts()
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const compact = isMobile || isTablet
  const [uid, setUid] = useState('')
  const [quality, setQuality] = useState('')
  const [category, setCategory] = useState('')
  const [items, setItems] = useState([])
  const [loading, setLoading] = useState(false)
  const [pagination, setPagination] = useState({ page: 1, page_size: 50, total: 0 })
  const [selected, setSelected] = useState({})

  // Harvest search
  const [harvestOpen, setHarvestOpen] = useState(false)
  const [harvestUid, setHarvestUid] = useState('')
  const [harvestKw, setHarvestKw] = useState('')
  const [harvestBusy, setHarvestBusy] = useState(false)
  const [harvestErr, setHarvestErr] = useState('')
  const loadSeqRef = useRef(0)

  const activeAccounts = useMemo(() => filterBusinessAccounts(accounts), [accounts])
  const activeAccountKey = useMemo(() => activeAccounts.map((a) => a.uid).join('|'), [activeAccounts])
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    setUid('')
    setItems([])
    setPagination({ page: 1, page_size: 50, total: 0 })
    setSelected({})
    setLoading(false)
    setHarvestOpen(false)
    setHarvestBusy(false)
    setHarvestErr('')
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  useEffect(() => { loadAccounts() }, [])
  useEffect(() => {
    if (!uid && activeAccounts.length) setUid(activeAccounts[0].uid)
  }, [activeAccounts, uid])

  async function load(page = 1) {
    if (!uid) { setItems([]); return }
    const requestKey = `${activeAccountKey}|${uid}|${quality}|${category}`
    const scope = captureScope(requestKey)
    const loadSeq = ++loadSeqRef.current
    const isCurrent = () => isScopeCurrent(scope, requestKey) && loadSeqRef.current === loadSeq
    setLoading(true)
    try {
      const r = await getCommentCandidates({ uid, quality, category, page, pageSize: 50 })
      if (!isCurrent()) return
      const list = unwrapList(r)
      setItems(page === 1 ? list : (prev) => [...prev, ...list])
      setPagination(r?.pagination || { page, page_size: 50, total: list.length })
    } catch (e) { if (isCurrent()) notify(e?.message || '加载失败', 'error') }
    finally { if (isCurrent()) setLoading(false) }
  }

  useEffect(() => { load(1); setSelected({}) }, [uid, quality, category, authToken, activeAccountKey])

  function toggle(id) {
    setSelected((m) => ({ ...m, [id]: !m[id] }))
  }

  const selectedItems = items.filter((it) => selected[it.id])

  async function promote() {
    if (!selectedItems.length) { notify('请先选择候选', 'error'); return }
    const confirmed = await showConfirm(`将 ${selectedItems.length} 条候选作为商品加入评论目标池？`, {
      title: '提升候选商品',
      confirmLabel: '加入目标池',
      width: 460,
    })
    if (!confirmed) return
    // Candidates are comments — promote uses item_id of the source product.
    const payload = selectedItems
      .map((it) => ({
        item_id: String(it.item_id || ''),
        title: it.author_nick ? `候选: ${it.author_nick}` : '',
        source: 'candidate_promote',
        status: 'pending',
      }))
      .filter((x) => x.item_id)
    if (!payload.length) { notify('选中的候选缺少 item_id', 'error'); return }
    try {
      const r = await batchAddCommentTargets(payload)
      notify(`已加入 ${r?.added ?? 0} 条目标，去重 ${r?.deduplicated ?? 0} 条`, 'success', 3200)
      setSelected({})
    } catch (e) { notify(e?.message || '加入目标池失败', 'error') }
  }

  async function runHarvest() {
    setHarvestErr('')
    if (!harvestUid) { setHarvestErr('请选择账号'); return }
    if (!harvestKw.trim()) { setHarvestErr('请输入关键词'); return }
    setHarvestBusy(true)
    const requestKey = `${activeAccountKey}|${harvestUid}`
    const scope = captureScope(requestKey)
    try {
      const r = await harvestCommentCandidates(harvestUid, harvestKw.trim(), { pageSize: 30 })
      if (!isScopeCurrent(scope, requestKey)) return
      // Try to unearth a list of item nodes from the mtop payload.
      const data = r?.data || r
      const candidates = []
      const stack = [data]
      while (stack.length && candidates.length < 100) {
        const cur = stack.pop()
        if (!cur) continue
        if (Array.isArray(cur)) { for (const x of cur) stack.push(x); continue }
        if (typeof cur === 'object') {
          const id = cur.itemId || cur.item_id || cur.id
          const title = cur.title || cur.itemTitle
          if (id && title) {
            candidates.push({
              item_id: String(id),
              title: String(title),
              price: String(cur.price || cur.priceText || ''),
              pic_url: String(cur.picUrl || cur.pic_url || cur.image || ''),
              item_url: String(cur.itemUrl || cur.item_url || ''),
              seller_id: String(cur.userId || cur.seller_id || ''),
              seller_nick: String(cur.userNick || cur.seller_nick || ''),
              source: 'search',
              status: 'pending',
            })
          }
          for (const v of Object.values(cur)) {
            if (v && typeof v === 'object') stack.push(v)
          }
        }
      }
      // Deduplicate by item_id.
      const seen = new Set()
      const dedup = candidates.filter((c) => {
        if (seen.has(c.item_id)) return false
        seen.add(c.item_id); return true
      })
      if (!dedup.length) { setHarvestErr('未从搜索结果解析出商品，换个关键词试试'); return }
      const resp = await batchAddCommentTargets(dedup)
      if (!isScopeCurrent(scope, requestKey)) return
      notify(`关键词“${harvestKw}”共获取 ${dedup.length} 条，加入 ${resp?.added ?? 0} 条评论目标，去重 ${resp?.deduplicated ?? 0} 条`, 'success', 3600)
      setHarvestOpen(false)
      setHarvestKw('')
    } catch (e) { if (isScopeCurrent(scope, requestKey)) setHarvestErr(e.message || '采集失败') }
    finally { if (isScopeCurrent(scope, requestKey)) setHarvestBusy(false) }
  }

  const hasMore = items.length < (pagination?.total || 0)

  const actions = (
    <div className="flex items-center gap-1">
      <button onClick={() => load(1)} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40">
        <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
      </button>
      <button onClick={() => { setHarvestUid(uid); setHarvestKw(''); setHarvestErr(''); setHarvestOpen(true) }}
        className="btn-primary btn-sm">
        <Search className="w-3.5 h-3.5" /> 关键词采集
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
        : <TopBar title="候选商品" right={actions} />}

      <div className={compact ? `px-4 ${embedded ? 'pt-4 pb-3' : 'py-3'} space-y-3 shrink-0` : 'px-4 py-3 space-y-2 shrink-0 border-b'} style={compact ? undefined : { borderColor: 'var(--border-light)' }}>
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
          <select className="input" value={uid} onChange={(e) => setUid(e.target.value)}>
            <option value="">选择账号</option>
            {activeAccounts.map((a) => (
              <option key={a.uid} value={a.uid}>{a.nick || a.uid}</option>
            ))}
          </select>
          <div className="grid grid-cols-2 gap-2 sm:contents">
            <select className="input" value={quality} onChange={(e) => setQuality(e.target.value)}>
              {QUALITY_OPTIONS.map((o) => <option key={o.key} value={o.key}>{o.label}</option>)}
            </select>
            <select className="input" value={category} onChange={(e) => setCategory(e.target.value)}>
              {CATEGORY_OPTIONS.map((o) => <option key={o.key} value={o.key}>{o.label}</option>)}
            </select>
          </div>
        </div>
        {selectedItems.length > 0 && (
          <button onClick={promote} className="btn-primary w-full btn-sm">
            <ArrowUpRight className="w-3.5 h-3.5" /> 提升 {selectedItems.length} 条到评论目标池
          </button>
        )}
      </div>

      <div className={bodyClass}>
        {!uid ? (
          <EmptyState icon={MessageSquare} title="请先选择账号" description="按账号查看候选商品" />
        ) : loading && !items.length ? (
          <div className={compact ? 'mobile-soft-card flex items-center justify-center py-24 gap-2' : 'flex items-center justify-center py-24 gap-2'} style={{ color: 'var(--text-secondary)' }}>
            <RefreshCw className="w-4 h-4 animate-spin" /> 加载中…
          </div>
        ) : !items.length ? (
          <EmptyState icon={MessageSquare} title="暂无候选" description="点击右上角关键词采集" />
        ) : (
          <div className={compact ? 'mobile-list-stack' : 'space-y-2'}>
            {items.map((it) => (
              <label key={it.id} className={compact ? 'mobile-soft-card p-3.5 flex items-start gap-3 cursor-pointer' : 'card p-3 flex items-start gap-3 cursor-pointer'}>
                <input type="checkbox" checked={Boolean(selected[it.id])}
                  onChange={() => toggle(it.id)} className="h-4 w-4 mt-1 accent-[var(--primary)]" />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 flex-wrap">
                    <StatusBadge status={qualityKind(it.quality)} label={it.quality || 'review'} />
                    {it.category && (
                      <span className="px-2 py-0.5 rounded-full text-[11px]"
                        style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>{it.category}</span>
                    )}
                    {typeof it.score === 'number' && (
                      <span className="text-[11px]" style={{ color: 'var(--text-muted)' }}>分 {it.score.toFixed(2)}</span>
                    )}
                    {it.accepted_for_txt && (
                      <span className="text-[11px]" style={{ color: 'var(--primary)' }}>已导入TXT</span>
                    )}
                  </div>
                  <p className="text-[13px] mt-1" style={{ color: 'var(--text-main)' }}>{it.content}</p>
                  <p className="text-[11px] font-mono truncate mt-1" style={{ color: 'var(--text-muted)' }}>
                    {it.author_nick || it.author_id || '匿名'} · {it.item_id} · {it.published_at || ''}
                  </p>
                </div>
              </label>
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

      <Modal open={harvestOpen} onClose={() => setHarvestOpen(false)} title="关键词采集到评论目标">
        <div className="space-y-3">
          <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
            搜索后批量加入评论目标池。
          </p>
          <label className="block">
            <span className="label">账号</span>
            <select className="input" value={harvestUid} onChange={(e) => setHarvestUid(e.target.value)}>
              <option value="">选择账号</option>
              {activeAccounts.map((a) => <option key={a.uid} value={a.uid}>{a.nick || a.uid}</option>)}
            </select>
          </label>
          <label className="block">
            <span className="label">搜索关键词</span>
            <input className="input" value={harvestKw} onChange={(e) => setHarvestKw(e.target.value)}
              placeholder="如：iPhone 15 Pro" />
          </label>
          {harvestErr && (
            <p className="liquid-notice liquid-notice-danger rounded-xl px-4 py-2.5 text-[12.5px]">
              {harvestErr}
            </p>
          )}
          <div className="flex gap-2">
            <button onClick={() => setHarvestOpen(false)} className="btn-secondary flex-1">取消</button>
            <button onClick={runHarvest} disabled={harvestBusy}
              className="btn-primary flex-1 disabled:opacity-40">
              {harvestBusy ? '采集中…' : '采集并加入'}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
