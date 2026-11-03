import { useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { Check, ChevronRight, PackagePlus, RefreshCw, Search } from 'lucide-react'
import { EmptyState } from '../../components/EmptyState'
import { SafeRemoteImage } from '../messages/SafeRemoteImage'
import { BatchListModal } from '../search/BatchListModal'
import { SOURCE_MODES, useSearchPageState } from '../search/hooks/useSearchPageState'
import { PeerMonitorTabs } from './PeerMonitorTabs'
import { Avatar } from './PeerMonitorPage'

type SearchItem = ReturnType<typeof useSearchPageState>['results'][number]

export default function PeerMonitorSearch() {
  const s = useSearchPageState()
  const { doSearch, hasMore, loading, loadingMore, page } = s
  const navigate = useNavigate()
  const sentinelRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    const node = sentinelRef.current
    if (!node) return
    const observer = new IntersectionObserver(entries => {
      if (entries[0].isIntersecting && hasMore && !loading && !loadingMore) doSearch(undefined, { nextPage: page + 1 })
    }, { rootMargin: '240px' })
    observer.observe(node)
    return () => observer.disconnect()
  }, [doSearch, hasMore, loading, loadingMore, page])

  if (s.accountsLoading) {
    return <div className="h-full flex flex-col">
      <PeerMonitorTabs />
      <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6">
        <div className="surface-card rounded-2xl py-20 text-center text-[13px]" role="status" style={{ color: 'var(--text-secondary)' }}>
          <RefreshCw className="w-4 h-4 animate-spin inline mr-2" />正在加载搜索账号…
        </div>
      </div>
    </div>
  }
  return <div className="h-full flex flex-col">
    <PeerMonitorTabs />
    <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6 space-y-3">
      <section className="surface-card rounded-2xl p-4">
        <div className="flex flex-col gap-3 md:flex-row md:items-center"><div className="min-w-0 flex-1"><h1 className="text-[16px] font-semibold" style={{ color: 'var(--text-main)' }}>搜索商品</h1><p className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>发现同行商品后，可继续批量上架，或打开详情加入同行监控。</p></div><div className="flex flex-1 max-w-[620px] items-center gap-2 rounded-xl border px-3 py-2" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}><Search className="w-4 h-4 shrink-0" style={{ color: 'var(--text-tertiary)' }} /><input value={s.keyword} onChange={e => s.setKeyword(e.target.value)} onKeyDown={e => e.key === 'Enter' && s.doSearch()} className="min-w-0 flex-1 bg-transparent outline-none text-[13px]" placeholder="输入商品关键词…" /><button type="button" onClick={() => s.doSearch()} disabled={s.loading || !s.keyword.trim()} className="btn-primary shrink-0 px-3 py-1.5 text-[12px] disabled:opacity-40">{s.loading ? <RefreshCw className="w-3.5 h-3.5 animate-spin" /> : '搜索'}</button></div></div>
        <div className="mt-3 flex gap-2 overflow-x-auto">{SOURCE_MODES.map(mode => <button key={mode.value} type="button" onClick={() => s.changeSourceMode(mode.value)} className="shrink-0 rounded-full px-3 py-1 text-[12px]" style={{ background: s.sourceMode === mode.value ? 'var(--primary)' : 'var(--surface-control)', color: s.sourceMode === mode.value ? 'var(--liquid-highlight)' : 'var(--text-main)', border: `1px solid ${s.sourceMode === mode.value ? 'var(--primary)' : 'var(--control-border)'}` }}>{mode.label}</button>)}</div>
      </section>
      {s.error ? <div className="rounded-xl px-3 py-2 text-[12px]" style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}>{s.error}</div> : null}
      {s.loading ? <div className="py-20 text-center text-[13px]" style={{ color: 'var(--text-secondary)' }}><RefreshCw className="w-4 h-4 animate-spin inline mr-2" />正在搜索商品…</div> : !s.searched ? <EmptyState icon={Search} title="输入关键词开始搜索" description="商品详情里可以查看同行主页或加入监控。" /> : !s.results.length ? <EmptyState icon={Search} title="没有搜索结果" description={`没找到「${s.keyword}」相关商品`} /> : <div className="grid gap-3 grid-cols-1 sm:grid-cols-2 xl:grid-cols-3">{s.results.map((item, index) => <SearchItemCard key={`${item.itemId}-${index}`} item={item} selected={s.selectedIds.has(item.itemId)} onToggle={() => s.toggleSelect(item.itemId)} onOpen={() => navigate(`/peer-monitor/items/${encodeURIComponent(item.itemId)}?uid=${encodeURIComponent(s.account.uid)}`, { state: { item, sourceUid: s.account.uid } })} />)}</div>}
      {s.searched && s.results.length ? <div ref={sentinelRef} className="py-5 text-center text-[12px]" style={{ color: 'var(--text-secondary)' }}>{s.loadingMore ? <><RefreshCw className="w-3.5 h-3.5 inline animate-spin mr-1" />加载更多…</> : s.hasMore ? '继续下滑加载更多' : '没有更多商品了'}</div> : null}
    </div>
    {s.selectedIds.size ? <div className="fixed bottom-5 left-1/2 z-40 flex -translate-x-1/2 items-center gap-2 rounded-full px-4 py-2.5 shadow-lg" style={{ background: 'var(--surface-card-strong)', border: '1px solid var(--control-border)' }}><span className="text-[12px] font-medium" style={{ color: 'var(--text-main)' }}>已选 {s.selectedIds.size} 件</span><button type="button" onClick={s.clearSelection} className="px-2 py-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>清除</button><button type="button" onClick={() => s.setShowBatchList(true)} className="btn-primary px-3 py-1.5 text-[12px] inline-flex items-center gap-1"><PackagePlus className="w-3.5 h-3.5" />一键上架</button></div> : null}
    {s.showBatchList && s.selectedItems.length ? <BatchListModal items={s.selectedItems} accounts={s.activeAccounts} onClose={() => s.setShowBatchList(false)} /> : null}
  </div>
}

function SearchItemCard({ item, selected, onToggle, onOpen }: { item: SearchItem; selected: boolean; onToggle: () => void; onOpen: () => void }) {
  return <article className="surface-card rounded-2xl overflow-hidden flex min-w-0"><button type="button" onClick={onOpen} className="w-28 shrink-0 relative bg-[var(--surface-sunken)]"><SafeRemoteImage src={item.picUrl} alt="" className="h-full w-full object-cover" fallback={<span className="h-full flex items-center justify-center text-[12px]" style={{ color: 'var(--text-tertiary)' }}>暂无图片</span>} /><span className="absolute bottom-2 left-2 rounded-full px-1.5 py-0.5 text-[10px]" style={{ background: 'rgba(0,0,0,.52)', color: '#fff' }}>查看详情</span></button><div className="min-w-0 flex-1 p-3 flex flex-col"><button type="button" onClick={onOpen} className="text-left"><p className="line-clamp-2 text-[13px] font-semibold leading-5" style={{ color: 'var(--text-main)' }}>{item.title}</p><div className="mt-2 flex items-end gap-2"><span className="text-[18px] font-semibold" style={{ color: 'var(--danger)' }}>¥{item.price || '—'}</span><span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{item.wantText}</span></div></button><div className="mt-auto pt-3 flex items-center justify-between gap-2"><button type="button" onClick={onOpen} className="min-w-0 flex items-center gap-1.5"><Avatar src={item.userAvatar} name={item.sellerNick} size="w-5 h-5" /><span className="truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>{item.sellerNick || item.area || '同行卖家'}</span><ChevronRight className="w-3.5 h-3.5 shrink-0" style={{ color: 'var(--text-tertiary)' }} /></button><button type="button" onClick={onToggle} className="shrink-0 rounded-full px-2.5 py-1.5 text-[11px] font-medium inline-flex items-center gap-1" style={{ color: selected ? 'var(--liquid-highlight)' : 'var(--primary)', background: selected ? 'var(--primary)' : 'var(--status-warning-bg)' }}>{selected ? <Check className="w-3 h-3" /> : null}{selected ? '已选择' : '加入上架'}</button></div></div></article>
}
