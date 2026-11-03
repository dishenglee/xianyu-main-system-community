// @ts-nocheck
// 搜索页 —— 桌面端渲染层。业务状态全部来自 useSearchPageState；本文件只负责布局与样式。
import { useEffect, useRef, useState } from 'react'
import {
  Search, RefreshCw, AlertCircle, ArrowUpDown, ChevronDown,
  ShoppingBag, Check,
} from 'lucide-react'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { EmptyState } from '../../components/EmptyState'
import { BatchListModal } from './BatchListModal'
import { useSearchPageState, SOURCE_MODES } from './hooks/useSearchPageState'
import { RegionFilterControl } from './RegionFilterControl'
import { SensitiveValue } from '../../components/SensitiveValue'

export default function DesktopSearch() {
  const s = useSearchPageState()
  const [expandedPanel, setExpandedPanel] = useState<string | null>(null)
  const sentinelRef = useRef<HTMLDivElement>(null)

  // Infinite scroll
  useEffect(() => {
    const sentinel = sentinelRef.current
    if (!sentinel) return
    const observer = new IntersectionObserver(entries => {
      if (entries[0].isIntersecting && s.hasMore && !s.loading && !s.loadingMore) {
        s.doSearch(undefined, { nextPage: s.page + 1 })
      }
    }, { rootMargin: '200px' })
    observer.observe(sentinel)
    return () => observer.disconnect()
  }, [s.hasMore, s.loading, s.loadingMore, s.page, s.doSearch])

  if (s.accountsLoading || !s.account) return null

  return (
    <>
      <HubTabs tabs={HUB_PRESETS.items} />
      <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6 space-y-3">
        {/* Search bar */}
        <div className="glass-panel rounded-2xl p-4 flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <div className="flex-1 flex items-center gap-2 rounded-xl px-3 py-1.5 border" style={{ borderColor: 'var(--border-subtle)', background: 'var(--bg-subtle)' }}>
              <Search className="w-4 h-4" style={{ color: 'var(--text-tertiary)' }} />
              <input value={s.keyword} onChange={e => s.setKeyword(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && s.doSearch()}
                className="flex-1 bg-transparent outline-none text-[13px]" placeholder="输入商品关键词…" />
            </div>
            <button type="button" onClick={() => s.doSearch()} disabled={!s.keyword.trim() || s.loading}
              className="btn-primary px-4 py-2 text-[13px] flex items-center gap-1.5 disabled:opacity-40">
              {s.loading ? <RefreshCw className="w-4 h-4 animate-spin" /> : <Search className="w-4 h-4" />}
              搜索
            </button>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-[12px] shrink-0" style={{ color: 'var(--text-secondary)' }}>通道</span>
            {SOURCE_MODES.map(m => (
              <button key={m.value}
                onClick={() => s.changeSourceMode(m.value)}
                className="px-2.5 py-1 rounded-full text-[12px] font-medium cursor-pointer transition-colors"
                style={{ background: s.sourceMode === m.value ? 'var(--primary)' : 'var(--surface-control)', color: s.sourceMode === m.value ? 'var(--liquid-highlight)' : 'var(--text-main)', border: `1px solid ${s.sourceMode === m.value ? 'var(--primary)' : 'var(--control-border)'}` }}>
                {m.label}
              </button>
            ))}
          </div>
        </div>

        {/* Sort bar */}
        {s.searched ? (
          <div className="flex items-center gap-2 overflow-x-auto pb-1 no-scrollbar">
            <ArrowUpDown className="w-3.5 h-3.5 shrink-0" style={{ color: 'var(--text-secondary)' }} />
            <SortChip label="综合" active={!s.sortField} onClick={() => s.handleSort('', '')} />
            <SortChip label={`价格${s.sortField === 'price' ? (s.sortValue === 'asc' ? '↑' : '↓') : ''}`}
              active={s.sortActive('price')} onClick={() => s.handleSort('price', '')} />
            <SortChip label="降价" active={s.sortActive('reduce', 'desc')} onClick={() => s.handleSort('reduce', 'desc')} />
            <SortChip label="新发" active={s.sortActive('create', 'desc')} onClick={() => s.handleSort('create', 'desc')} />
          </div>
        ) : null}

        {/* Filters */}
        {s.searched ? (
          <div className="surface-card rounded-xl p-3 space-y-3">
            <div className="flex items-center justify-between gap-3">
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                筛选
              </div>
              {s.activeFilters.size > 0 ? (
                <button onClick={s.clearFilters} className="liquid-notice liquid-notice-danger px-2.5 py-1 rounded-full text-[12px] font-medium cursor-pointer">
                  清除动态筛选
                </button>
              ) : null}
            </div>
            <div className="flex flex-wrap gap-2 items-start">
              <RegionFilterControl
                uid={s.account.uid}
                region={s.nearby}
                onApply={(region) => {
                  s.setNearby(region)
                  if (s.searched && s.keyword.trim()) s.doSearch(undefined, { region })
                }}
              />
              {s.dynFilters.map(panel => {
                const isMulti = panel.values.length > 1
                const activeVal = s.activeFilters.get(panel.propId)
                if (!isMulti) {
                  const v = panel.values[0]
                  const active = activeVal === v.valueId
                  return (
                    <button key={panel.propId} onClick={() => s.toggleFilter(panel.propId, v.valueId)}
                      className="px-3 py-1.5 rounded-lg text-[12px] font-medium transition-colors cursor-pointer"
                      style={{ background: active ? 'var(--control-active)' : 'var(--surface-control)', color: active ? 'var(--primary)' : 'var(--text-main)', border: `1px solid ${active ? 'color-mix(in srgb, var(--primary) 22%, transparent)' : 'transparent'}` }}>
                      {v.valueName}
                    </button>
                  )
                }
                const isOpen = expandedPanel === panel.propId
                return (
                  <div key={panel.propId} className="relative">
                    <button onClick={() => setExpandedPanel(isOpen ? null : panel.propId)}
                      className="px-3 py-1.5 rounded-lg text-[12px] font-medium flex items-center gap-1 cursor-pointer transition-colors"
                      style={{ background: activeVal ? 'var(--control-active)' : 'var(--surface-control)', color: activeVal ? 'var(--primary)' : 'var(--text-main)', border: `1px solid ${activeVal ? 'color-mix(in srgb, var(--primary) 22%, transparent)' : 'transparent'}` }}>
                      {panel.propName}{activeVal ? `: ${panel.values.find(v => v.valueId === activeVal)?.valueName ?? ''}` : ''}
                      <ChevronDown className={`w-3 h-3 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
                    </button>
                    {isOpen ? (
                      <div className="absolute left-0 top-full mt-1 z-20 rounded-lg p-2 flex flex-wrap gap-1.5 min-w-[180px] max-w-[320px]"
                        style={{ background: 'var(--surface-card-strong)', border: '1px solid var(--control-border)', boxShadow: 'var(--glass-shadow-float)' }}>
                        {panel.values.map(v => (
                          <button key={v.valueId} onClick={() => { s.toggleFilter(panel.propId, v.valueId); setExpandedPanel(null) }}
                            className="px-2.5 py-1 rounded-lg text-[11px] font-medium transition-colors cursor-pointer whitespace-nowrap"
                            style={{ background: activeVal === v.valueId ? 'var(--control-active)' : 'var(--surface-control)', color: activeVal === v.valueId ? 'var(--primary)' : 'var(--text-main)', border: `1px solid ${activeVal === v.valueId ? 'color-mix(in srgb, var(--primary) 22%, transparent)' : 'transparent'}` }}>
                            {v.valueName}
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                )
              })}
            </div>
          </div>
        ) : null}

        {/* Error */}
        {s.error ? (
          <div className="rounded-[10px] px-3 py-2 text-[13px] flex items-center gap-2"
            style={{ background: 'var(--status-danger-bg)', border: '1px solid color-mix(in srgb, var(--danger) 20%, transparent)', color: 'var(--status-danger-text)' }}>
            <AlertCircle className="w-4 h-4 shrink-0" />{s.error}
          </div>
        ) : null}

        {/* Results */}
        {s.loading ? (
          <div className="flex items-center justify-center py-24 gap-2" style={{ color: 'var(--text-secondary)' }}>
            <RefreshCw className="w-4 h-4 animate-spin" /> 搜索中…
          </div>
        ) : !s.searched ? (
          <EmptyState icon={Search} title="输入关键词开始搜索" description="开始搜索" />
        ) : !s.results.length && !s.error ? (
          <EmptyState icon={Search} title="没有搜索结果" description={`没找到「${s.keyword}」相关的商品`} />
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-5 lg:grid-cols-7 xl:grid-cols-10 gap-2">
            {s.results.map((item, i) => {
              const isSelected = s.selectedIds.has(item.itemId)
              return (
              <a key={`${item.itemId}-${i}`}
                href={`https://www.goofish.com/item?id=${item.itemId}`} target="_blank" rel="noreferrer"
                className="rounded-[8px] overflow-hidden flex flex-col transition cursor-pointer"
                style={{
                  background: 'var(--surface-card)', textDecoration: 'none', color: 'inherit',
                  border: isSelected ? '2px solid var(--primary)' : '1px solid var(--glass-border)',
                  boxShadow: isSelected ? 'var(--glass-shadow-soft)' : 'none',
                }}>
                <div className="relative overflow-hidden aspect-square" style={{ background: 'var(--surface-sunken)' }}>
                  {item.picUrl ? <img src={item.picUrl} alt="" className="w-full h-full object-cover block" loading="lazy" />
                    : <div className="w-full h-full flex items-center justify-center" style={{ color: 'var(--text-tertiary)' }}>—</div>}
                  {item.freeShip ? (
                    <span className="absolute top-0 left-0 text-[9px] font-bold px-1 py-0.5 rounded-br-md" style={{ background: 'var(--warning)', color: 'var(--liquid-highlight)' }}>包邮</span>
                  ) : null}
                  <button
                    onClick={e => { e.preventDefault(); e.stopPropagation(); s.toggleSelect(item.itemId) }}
                    className="absolute top-1 right-1 z-10 w-5 h-5 rounded-full flex items-center justify-center transition-all cursor-pointer"
                    style={{
                      background: isSelected ? 'var(--primary)' : 'rgba(0,0,0,0.25)',
                      border: isSelected ? '2px solid var(--primary)' : '2px solid rgba(255,255,255,0.8)',
                    }}>
                    {isSelected ? <Check className="w-3 h-3 text-white" strokeWidth={3} /> : null}
                  </button>
                </div>
                <div className="px-1.5 pt-1.5 pb-1.5 flex-1 flex flex-col gap-[3px]">
                  <p className="text-[11px] leading-[1.35] line-clamp-2" style={{ color: 'var(--text-main)' }}>
                    {item.sellerBadgeUrl ? <img src={item.sellerBadgeUrl} alt="" className="inline-block align-middle mr-0.5" style={{ height: 11, position: 'relative', top: -1 }} /> : null}
                    <SensitiveValue kind="item-title" value={item.title} />
                  </p>
                  <div className="text-[9px] truncate leading-snug" style={{ color: 'var(--text-muted)' }}>
                    {item.serviceTags.length > 0
                      ? item.serviceTags.slice(0, 2).join(' · ')
                      : item.statTags.length > 0
                        ? item.statTags.join(' · ')
                        : item.infoTags.length > 0 ? item.infoTags[0] : ' '}
                  </div>
                  <div className="flex items-baseline gap-1 mt-auto">
                    <span className="text-[13px] font-bold leading-none" style={{ color: 'var(--danger)' }}>
                      <span className="text-[9px]">¥</span><SensitiveValue kind="money" value={item.price ?? '—'} />
                    </span>
                    {item.wantText ? <span className="text-[9px] leading-none" style={{ color: 'var(--text-muted)' }}>{item.wantText}</span> : null}
                  </div>
                  <div className="flex items-center gap-1">
                    {item.userAvatar ? (
                      <img src={item.userAvatar} alt="" className="w-[14px] h-[14px] rounded-full shrink-0 object-cover" />
                    ) : (
                      <div className="w-[14px] h-[14px] rounded-full shrink-0 flex items-center justify-center text-[8px]" style={{ background: 'var(--status-neutral-bg)', color: 'var(--text-muted)' }}>
                        {(item.sellerNick || '?')[0]}
                      </div>
                    )}
                    <span className="text-[9px] truncate" style={{ color: 'var(--text-muted)' }}>
                      <SensitiveValue kind="account" value={item.area || item.sellerNick} />
                    </span>
                    {item.userExtraTags.length > 0 ? (
                      <span className="ml-auto shrink-0 text-[8px] px-1 rounded-full leading-[16px]" style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}>
                        {item.userExtraTags[0]}
                      </span>
                    ) : null}
                  </div>
                </div>
              </a>
              )
            })}
          </div>
        )}

        {/* Floating selection bar */}
        {s.selectedIds.size > 0 ? (
          <div className="glass-dock fixed bottom-6 left-1/2 -translate-x-1/2 z-40 rounded-full px-5 py-2.5 flex items-center gap-3"
            style={{ color: 'var(--text-main)' }}>
            <span className="text-[13px] font-medium">已选 {s.selectedIds.size} 件</span>
            <button onClick={s.clearSelection}
              className="text-[12px] px-2.5 py-1 rounded-full cursor-pointer transition-colors"
              style={{ background: 'var(--surface-control)', color: 'var(--text-main)' }}>
              清除
            </button>
            <button onClick={() => s.setShowBatchList(true)}
              className="text-[12px] px-3 py-1 rounded-full font-medium flex items-center gap-1.5 cursor-pointer"
              style={{ background: 'var(--primary)', color: 'var(--liquid-highlight)' }}>
              <ShoppingBag className="w-3.5 h-3.5" /> 一键上架
            </button>
          </div>
        ) : null}

        {/* Infinite scroll sentinel */}
        {s.searched && s.results.length > 0 ? (
          <div ref={sentinelRef} className="flex items-center justify-center py-6 gap-2 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            {s.loadingMore ? <><RefreshCw className="w-4 h-4 animate-spin" /> 加载更多…</> : s.hasMore ? <span style={{ color: 'var(--text-tertiary)' }}>下滑加载更多</span> : <span style={{ color: 'var(--text-tertiary)' }}>没有更多了</span>}
          </div>
        ) : null}
      </div>

      {s.showBatchList && s.selectedItems.length > 0 ? (
        <BatchListModal
          items={s.selectedItems}
          accounts={s.activeAccounts}
          onClose={() => s.setShowBatchList(false)}
        />
      ) : null}
    </>
  )
}

function SortChip({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button onClick={onClick}
      className="shrink-0 px-2.5 py-1 rounded-full text-[12px] font-medium transition-colors cursor-pointer"
      style={{ background: active ? 'var(--primary)' : 'var(--surface-control)', color: active ? 'var(--liquid-highlight)' : 'var(--text-main)', border: `1px solid ${active ? 'var(--primary)' : 'var(--control-border)'}` }}>
      {label}
    </button>
  )
}
