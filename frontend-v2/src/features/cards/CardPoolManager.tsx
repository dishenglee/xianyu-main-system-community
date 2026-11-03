import { useState } from 'react'
import { ChevronDown, CreditCard, Loader2, Plus, RefreshCw } from 'lucide-react'
import { EmptyState } from '../../components/EmptyState'
import { Modal } from '../../components/Modal'
import { useAppDialog } from '../../components/AppDialog'
import { useSafeNotify } from '../../components/Notify'
import { countCardImportEntries } from './cardImport'
import { useCardPools } from './useCardPools'
import type { LoadedCard } from './useCardPools'
import { getCardPoolEarliestCreatedAt } from './cardPoolGroups'
import type { CardPoolGroup } from './cardPoolGroups'

function formatPoolAddedTime(cards: LoadedCard[]) {
  const value = getCardPoolEarliestCreatedAt(cards)
  if (!value) return '添加时间未知'
  const date = new Date(value)
  const pad = (part: number) => String(part).padStart(2, '0')
  return `添加时间 ${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

export default function CardPoolManager({ mobile = false, embedded = false }: { mobile?: boolean; embedded?: boolean }) {
  const { pools, loading, saving, load, importCards, updatePool, deletePool } = useCardPools()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const [expanded, setExpanded] = useState(() => new Set<string>())
  const [showImport, setShowImport] = useState(false)
  const [tag, setTag] = useState('')
  const [rawText, setRawText] = useState('')
  const [reusable, setReusable] = useState(false)
  const [error, setError] = useState('')
  const [editingPool, setEditingPool] = useState<CardPoolGroup<LoadedCard> | null>(null)
  const [editTag, setEditTag] = useState('')
  const [editContents, setEditContents] = useState<Record<number, string>>({})
  const [editError, setEditError] = useState('')

  async function submitImport() {
    try {
      const count = await importCards({ tag, rawText, reusable })
      notify(`已导入 ${count} 条卡密`, 'success')
      setShowImport(false); setTag(''); setRawText(''); setReusable(false); setError('')
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
  }

  function openEditor(pool: CardPoolGroup<LoadedCard>) {
    setEditingPool(pool)
    setEditTag(pool.tag)
    setEditContents(Object.fromEntries(pool.cards.flatMap((card) => typeof card.id === 'number' ? [[card.id, String(card.cardContent ?? card.content ?? '')]] : [])))
    setEditError('')
  }

  async function saveEdit() {
    if (!editingPool) return
    try {
      const result = await updatePool(editingPool, { tag: editTag, contentsById: editContents })
      if (result.failed) {
        const message = `部分更新成功：成功 ${result.updated} 条，失败 ${result.failed} 条，已刷新当前数据`
        setEditError(message); notify(message, 'error'); return
      }
      notify('卡密标签已更新', 'success')
      setEditingPool(null)
    } catch (cause) { setEditError(cause instanceof Error ? cause.message : String(cause)) }
  }

  async function removePool(pool: CardPoolGroup<LoadedCard>) {
    if (!await showConfirm(`确认删除「${pool.tag}」整组卡密？该操作会逐条真实删除且不可恢复。`, { title: '删除整组卡密', confirmLabel: '整组删除', danger: true })) return
    try {
      const result = await deletePool(pool)
      if (result.failed) notify(`整组删除部分完成：成功 ${result.deleted} 条，失败 ${result.failed} 条，已刷新当前数据`, 'error')
      else notify(`已删除「${pool.tag}」整组卡密`, 'success')
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause), 'error') }
  }

  return <>
    <div className={embedded ? '' : mobile ? 'mobile-page-stack' : 'px-4 py-4 md:px-6'}>
      <div className={mobile ? 'mobile-surface-card overflow-hidden' : 'card overflow-hidden'}>
        <div className="flex items-center justify-between gap-3 border-b px-4 py-3" style={{ borderColor: 'var(--border-light)' }}>
          <div><h2 className="text-[16px] font-semibold">卡密管理</h2><p className="mt-0.5 text-[12px]" style={{ color: 'var(--text-secondary)' }}>每个标签是一组独立卡密</p></div>
          <div className="flex items-center gap-2"><button type="button" onClick={load} disabled={loading} className="btn-ghost btn-sm" aria-label="刷新"><RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} /></button><button type="button" onClick={() => setShowImport(true)} className="btn-primary btn-sm"><Plus className="h-4 w-4" />导入卡密</button></div>
        </div>
        {loading && !pools.length ? <div className="flex min-h-[220px] items-center justify-center gap-2"><Loader2 className="h-4 w-4 animate-spin" />正在读取卡密…</div>
          : !pools.length ? <div className="flex min-h-[220px] items-center justify-center"><EmptyState icon={CreditCard} title="暂无卡密" description="导入后会按标签形成卡密组" /></div>
          : <div>{pools.map((pool, poolIndex) => {
            const open = expanded.has(pool.key)
            const typeLabel = pool.type === 'reusable' ? '固定卡密' : pool.type === 'one-time' ? '一次性卡密' : '类型冲突'
            return <section key={pool.key} className={`px-4 py-3 ${poolIndex ? 'border-t' : ''}`} style={poolIndex ? { borderColor: 'var(--border-light)' } : undefined}>
              <div className="flex flex-wrap items-center gap-2"><div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-2"><span className="truncate text-[14px] font-semibold">{pool.tag}</span><span className="mobile-pill mobile-muted-pill px-2 py-0.5 text-[11px]">{typeLabel}</span><span className="text-[11px]" style={{ color: 'var(--text-muted)' }}>{formatPoolAddedTime(pool.cards)}</span></div></div><button className="btn-secondary btn-sm" onClick={() => openEditor(pool)}>编辑</button><button className="btn-ghost btn-sm" style={{ color: 'var(--danger)' }} onClick={() => removePool(pool)}>整组删除</button><button className="btn-ghost btn-sm" onClick={() => setExpanded((current) => { const next = new Set(current); if (open) next.delete(pool.key); else next.add(pool.key); return next })}>查看明细<ChevronDown className={`h-4 w-4 ${open ? 'rotate-180' : ''}`} /></button></div>
              {open && <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">{pool.cards.map((card) => <div key={card.id} className="rounded-xl border p-3" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}><div className="flex min-w-0 items-start gap-3"><div className="min-w-0 flex-1 whitespace-pre-wrap break-all text-[13px]">{card.cardContent || card.content || '—'}</div><div className="shrink-0 text-right text-[11px]" style={{ color: 'var(--text-secondary)' }}><span className="mobile-pill mobile-muted-pill inline-flex px-2 py-0.5">{card.used ? '已使用' : '未使用'}</span>{card.used && (card.usedOrderId || card.used_order_id) ? <div className="mt-1 max-w-[140px] break-all">订单：{card.usedOrderId || card.used_order_id}</div> : null}</div></div></div>)}</div>}
            </section>
          })}</div>}
      </div>
    </div>

    <Modal open={showImport} onClose={() => !saving && setShowImport(false)} title="批量导入卡密" footer={<><button className="btn-secondary btn-sm" onClick={() => setShowImport(false)}>取消</button><button className="btn-primary btn-sm" disabled={saving} onClick={submitImport}>{saving ? '导入中…' : '开始导入'}</button></>}><div className="space-y-4"><label className="block"><span className="label">卡密标签</span><input className="input" value={tag} onChange={(event) => setTag(event.target.value)} /></label><div><span className="label">卡密类型</span><div className="grid grid-cols-2 gap-2"><button className={reusable ? 'btn-secondary' : 'btn-primary'} onClick={() => setReusable(false)}>一次性卡密</button><button className={reusable ? 'btn-primary' : 'btn-secondary'} onClick={() => setReusable(true)}>固定卡密</button></div></div><label className="block"><span className="label">{reusable ? '固定卡密内容' : '卡密列表'}</span><textarea className="input min-h-[160px]" value={rawText} onChange={(event) => setRawText(event.target.value)} placeholder={reusable ? '整段内容作为一条固定卡密' : '每行一条一次性卡密'} /><span className="mt-1 block text-[12px]" style={{ color: 'var(--text-muted)' }}>将导入 {countCardImportEntries(rawText, reusable)} 条</span></label>{error && <div className="liquid-notice liquid-notice-danger rounded-xl px-3 py-2 text-[12px]">{error}</div>}</div></Modal>

    <Modal open={Boolean(editingPool)} onClose={() => !saving && setEditingPool(null)} title={`编辑标签 · ${editingPool?.tag || ''}`} footer={<><button className="btn-secondary btn-sm" disabled={saving} onClick={() => setEditingPool(null)}>取消</button><button className="btn-primary btn-sm" disabled={saving} onClick={saveEdit}>{saving ? '保存中…' : '保存修改'}</button></>}><div className="space-y-4"><label className="block"><span className="label">标签名称</span><input className="input" value={editTag} onChange={(event) => setEditTag(event.target.value)} /></label><div><span className="label">卡密类型</span><div className="input opacity-70">{editingPool?.type === 'reusable' ? '固定卡密' : editingPool?.type === 'one-time' ? '一次性卡密' : '类型冲突'}（不可修改）</div></div><div className="space-y-3"><span className="label">卡密内容</span>{editingPool?.cards.map((card, index) => typeof card.id === 'number' ? <label key={card.id} className="block"><span className="mb-1 block text-[11px]" style={{ color: 'var(--text-muted)' }}>第 {index + 1} 条</span><textarea className="input min-h-[84px]" value={editContents[card.id] ?? ''} onChange={(event) => setEditContents((current) => ({ ...current, [card.id!]: event.target.value }))} /></label> : null)}</div>{editError && <div className="liquid-notice liquid-notice-danger rounded-xl px-3 py-2 text-[12px]">{editError}</div>}</div></Modal>
  </>
}
