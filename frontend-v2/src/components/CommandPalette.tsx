import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Search, Loader2, Package, ShoppingCart, MessageSquare, Ticket, Sparkles, User, X } from 'lucide-react'
import { useAccounts } from '../hooks/useAccounts'
import { search } from '../api/index'
import { colorForAccount } from '../features/messages/accountColor'
import { useAuthScope } from '../hooks/useAuthScope'
import { filterBusinessAccounts } from '../utils/accountAccess'
import { SensitiveValue, useSensitiveHidden } from './SensitiveValue'

// -----------------------------------------------------------------------------
// Global ⌘K / Ctrl+K palette.
// Mounts once at <App> root. Focus on searching商品 (uses the real `search(uid,
// keyword)` endpoint, cycled per account). Other types stay as stubs with a
// "待接入" badge until their local-index helpers are wired up.
// -----------------------------------------------------------------------------

interface Result {
  type: 'item'
  uid: string
  nick: string
  color: string
  itemId: string
  title: string
  price?: string | number
  picUrl?: string
}

const TYPES = [
  { key: 'item',     label: '商品',    Icon: Package,       ready: true },
  { key: 'order',    label: '订单',    Icon: ShoppingCart,  ready: false },
  { key: 'msg',      label: '消息',    Icon: MessageSquare, ready: false },
  { key: 'card',     label: '卡密',    Icon: Ticket,        ready: false },
  { key: 'template', label: '回复模板',Icon: Sparkles,      ready: false },
  { key: 'buyer',    label: '买家',    Icon: User,          ready: false },
] as const

export default function CommandPalette() {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [typeKey, setTypeKey] = useState<string>('item')
  const [acct, setAcct] = useState<string>('')
  const [results, setResults] = useState<Result[]>([])
  const [loading, setLoading] = useState(false)
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null)
  const [activeIdx, setActiveIdx] = useState(0)
  const isSensitiveHidden = useSensitiveHidden()

  const { accounts, loadAccounts } = useAccounts()
  const inputRef = useRef<HTMLInputElement | null>(null)
  const debounceRef = useRef<number | null>(null)
  const reqIdRef = useRef(0)
  const resetForAuthChange = useCallback(() => {
    reqIdRef.current += 1
    setResults([])
    setLoading(false)
    setProgress(null)
    setActiveIdx(0)
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(resetForAuthChange)
  const navigate = useNavigate()

  const activeAccts = useMemo(() => filterBusinessAccounts(accounts), [accounts])

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const isMac = navigator.platform.toLowerCase().includes('mac')
      const hit = isMac ? (e.metaKey && e.key.toLowerCase() === 'k') : (e.ctrlKey && e.key.toLowerCase() === 'k')
      if (hit) {
        e.preventDefault()
        setOpen((v) => !v)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    if (open) {
      loadAccounts()
      setQuery('')
      setResults([])
      setActiveIdx(0)
      requestAnimationFrame(() => inputRef.current?.focus())
    }
  }, [open, loadAccounts, authToken])

  const runSearch = useCallback(async (q: string) => {
    if (typeKey !== 'item') { setResults([]); return }
    const kw = q.trim()
    if (!kw) { setResults([]); setLoading(false); return }
    const scope = acct ? activeAccts.filter((a) => a.uid === acct) : activeAccts
    if (scope.length === 0) { setResults([]); return }
    const scopeKey = scope.map((a) => a.uid).join('|')
    const authScope = captureScope(scopeKey)

    const myReqId = ++reqIdRef.current
    setLoading(true)
    setProgress({ done: 0, total: scope.length })
    const merged: Result[] = []
    for (const [index, a] of scope.entries()) {
      if (myReqId !== reqIdRef.current || !isScopeCurrent(authScope, scopeKey)) return
      try {
        const r: any = await search(a.uid, kw, { page: 1 })
        if (myReqId !== reqIdRef.current || !isScopeCurrent(authScope, scopeKey)) return
        const list = Array.isArray(r?.items) ? r.items : Array.isArray(r?.data?.itemList) ? r.data.itemList : Array.isArray(r?.resultList) ? r.resultList : []
        for (const it of list.slice(0, 8)) {
          merged.push({
            type: 'item',
            uid: a.uid,
            nick: a.nick || a.uid,
            color: colorForAccount(a.uid),
            itemId: String(it.itemId ?? it.id ?? ''),
            title: String(it.title ?? it.name ?? '(无标题)'),
            price: it.price ?? it.priceText,
            picUrl: it.picUrl ?? it.pic_url ?? it.pic,
          })
        }
      } catch {
        /* 忽略单账号失败 */
      }
      if (myReqId !== reqIdRef.current || !isScopeCurrent(authScope, scopeKey)) return
      setProgress({ done: Math.min(index + 1, scope.length), total: scope.length })
      setResults([...merged])
    }
    if (myReqId === reqIdRef.current && isScopeCurrent(authScope, scopeKey)) { setLoading(false); setProgress(null) }
  }, [typeKey, acct, activeAccts, captureScope, isScopeCurrent])

  // debounce input
  useEffect(() => {
    if (debounceRef.current) window.clearTimeout(debounceRef.current)
    debounceRef.current = window.setTimeout(() => runSearch(query), 280)
    return () => { if (debounceRef.current) window.clearTimeout(debounceRef.current) }
  }, [query, runSearch])

  // keyboard
  useEffect(() => {
    if (!open) return
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') { setOpen(false); return }
      if (e.key === 'ArrowDown') { e.preventDefault(); setActiveIdx((i) => Math.min(results.length - 1, i + 1)) }
      if (e.key === 'ArrowUp') { e.preventDefault(); setActiveIdx((i) => Math.max(0, i - 1)) }
      if (e.key === 'Enter' && results[activeIdx]) {
        const r = results[activeIdx]
        setOpen(false)
        navigate(`/peer-monitor/search?uid=${encodeURIComponent(r.uid)}&keyword=${encodeURIComponent(query)}`)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, results, activeIdx, navigate, query])

  if (!open) return null

  return (
    <div
      className="liquid-scrim fixed inset-0 z-[110] flex items-start justify-center p-4 pt-24"
      onClick={() => setOpen(false)}
    >
      <div
        className="modal-card w-full max-w-[680px] overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* input row */}
        <div className="px-4 py-3 flex items-center gap-2 border-b" style={{ borderColor: 'var(--border-subtle)' }}>
          <Search className="w-5 h-5" style={{ color: 'var(--text-tertiary)' }} />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => { setQuery(e.target.value); setActiveIdx(0) }}
            className="flex-1 bg-transparent outline-none text-[15px]"
            placeholder="搜索商品 · Tab 切类型 · Esc 关闭"
          />
          {loading ? <Loader2 className="w-4 h-4 animate-spin" style={{ color: 'var(--text-tertiary)' }} /> : null}
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="mobile-icon-button w-7 h-7 rounded-full flex items-center justify-center"
            aria-label="关闭"
          >
            <X className="w-4 h-4" style={{ color: 'var(--text-secondary)' }} />
          </button>
        </div>

        {/* scope row */}
        <div className="px-3 py-2 border-b flex items-center gap-2 flex-wrap" style={{ borderColor: 'var(--border-subtle)', background: 'var(--bg-subtle)' }}>
          <span className="text-[11px]" style={{ color: 'var(--text-tertiary)' }}>类型：</span>
          {TYPES.map((t) => {
            const active = t.key === typeKey
            return (
              <button
                key={t.key}
                disabled={!t.ready}
                onClick={() => t.ready && setTypeKey(t.key)}
                className="inline-flex items-center gap-1 text-[11px] font-medium px-2 py-[3px] rounded-[6px] cursor-pointer transition-colors"
                style={
                  active
                    ? { background: 'var(--primary)', color: 'var(--liquid-highlight)' }
                    : t.ready
                      ? { background: 'var(--surface-control)', color: 'var(--text-main)', border: '1px solid var(--control-border)' }
                      : { background: 'var(--surface-sunken)', color: 'var(--text-muted)', border: '1px dashed var(--control-border)', cursor: 'not-allowed' }
                }
                title={t.ready ? '' : '暂未接入'}
              >
                <t.Icon className="w-3 h-3" />
                {t.label}
                {!t.ready ? <span className="text-[9px] opacity-60">待接入</span> : null}
              </button>
            )
          })}
          <span className="w-px h-4 mx-1" style={{ background: 'var(--border-subtle)' }} />
          <span className="text-[11px]" style={{ color: 'var(--text-tertiary)' }}>账号：</span>
          <select
            value={acct}
            onChange={(e) => setAcct(e.target.value)}
            className="text-[11px] bg-transparent outline-none cursor-pointer font-medium"
            style={{ color: 'var(--text-main)' }}
          >
            <option value="">全部账号 ({activeAccts.length})</option>
            {activeAccts.map((a) => (
              <option key={a.uid} value={a.uid}>{isSensitiveHidden ? '••••••' : a.nick || a.uid}</option>
            ))}
          </select>
        </div>

        {/* results */}
        <div className="max-h-[440px] overflow-y-auto">
          {typeKey !== 'item' ? (
            <div className="p-8 text-center">
              <div className="text-[28px] mb-1">🚧</div>
              <div className="text-[13px]" style={{ color: 'var(--text-main)' }}>该类型搜索暂未接入</div>
              <div className="text-[11px] mt-1" style={{ color: 'var(--text-tertiary)' }}>当前只支持商品搜索，其他类型待上线</div>
            </div>
          ) : !query.trim() ? (
            <div className="p-8 text-center">
              <div className="text-[26px] mb-1">🔍</div>
              <div className="text-[13px]" style={{ color: 'var(--text-main)' }}>输入关键词搜索商品</div>
              <div className="text-[11px] mt-1" style={{ color: 'var(--text-tertiary)' }}>
                {activeAccts.length ? `将跨 ${acct ? 1 : activeAccts.length} 个账号调 /api/search` : '暂无活跃账号'}
              </div>
            </div>
          ) : results.length === 0 && !loading ? (
            <div className="p-8 text-center text-[13px]" style={{ color: 'var(--text-tertiary)' }}>
              没找到「{query}」相关的商品
            </div>
          ) : (
            results.map((r, i) => (
              <button
                key={`${r.uid}-${r.itemId}-${i}`}
                onMouseEnter={() => setActiveIdx(i)}
                onClick={() => { setOpen(false); navigate(`/peer-monitor/search?uid=${encodeURIComponent(r.uid)}&keyword=${encodeURIComponent(query)}`) }}
                className="w-full px-4 py-2.5 flex items-center gap-3 text-left cursor-pointer"
                style={{ borderBottom: '1px solid var(--control-border)', background: i === activeIdx ? 'var(--control-active)' : 'transparent' }}
              >
                <div className="w-10 h-10 rounded-[10px] overflow-hidden shrink-0 flex items-center justify-center" style={{ background: 'var(--surface-sunken)' }}>
                  {r.picUrl
                    ? <img src={r.picUrl} alt="" className="w-full h-full object-cover" />
                    : <Package className="w-4 h-4" style={{ color: 'var(--text-tertiary)' }} />
                  }
                </div>
                <div className="min-w-0 flex-1">
                  <SensitiveValue kind="item-title" className="text-[13px] font-medium truncate" value={r.title} />
                  <div className="text-[10px] mt-0.5 flex items-center gap-2" style={{ color: 'var(--text-muted)' }}>
                    <SensitiveValue kind="order-id" className="font-mono" value={`#${r.itemId}`} />
                    <span>·</span>
                    <SensitiveValue kind="account" className="truncate" value={r.nick} />
                  </div>
                </div>
                {r.price !== undefined ? (
                  <div className="text-[14px] font-semibold font-mono shrink-0" style={{ color: 'var(--text-main)' }}>¥<SensitiveValue kind="money" value={r.price} /></div>
                ) : null}
              </button>
            ))
          )}
        </div>

        {/* footer */}
        <div className="px-4 py-2 border-t flex items-center justify-between text-[11px]" style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-tertiary)', background: 'var(--bg-subtle)' }}>
          <div className="flex items-center gap-3">
            <span><Kbd>↑</Kbd><Kbd>↓</Kbd> 选择</span>
            <span><Kbd>↵</Kbd> 打开</span>
            <span><Kbd>Esc</Kbd> 关闭</span>
          </div>
          <span>
            {progress ? `已搜索 ${progress.done}/${progress.total} 账号` : acct ? '单账号' : `${activeAccts.length} 个账号`}
          </span>
        </div>
      </div>
    </div>
  )
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd
      className="font-mono inline-block rounded border mx-0.5 px-1.5 py-0"
      style={{ fontSize: 10, borderColor: 'var(--border-subtle)', background: 'var(--surface-control)', color: 'var(--text-tertiary)' }}
    >
      {children}
    </kbd>
  )
}
