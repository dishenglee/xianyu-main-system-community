// @ts-nocheck
// 搜索页 —— 移动端渲染层。保留原有搜索 / 筛选 / 排序 / 批量上架逻辑，
// 只重构移动端视觉层级，让搜索头、筛选摘要、结果卡片语言更统一。
import { useEffect, useRef, useState } from 'react'
import {
  Search, RefreshCw, AlertCircle, ChevronDown,
  ShoppingBag, Check, X, ArrowDownUp, ExternalLink, SlidersHorizontal, MapPin,
} from 'lucide-react'
import { EmptyState } from '../../components/EmptyState'
import { BatchListModal } from './BatchListModal'
import { useSearchPageState, SOURCE_MODES } from './hooks/useSearchPageState'
import { RegionFilterControl } from './RegionFilterControl'
import { SensitiveValue } from '../../components/SensitiveValue'

interface SortOption { key: string; field: string; value: string; label: string }

function getSortOptions(priceDir: 'asc' | 'desc'): SortOption[] {
  return [
    { key: 'all', field: '', value: '', label: '综合排序' },
    { key: 'price', field: 'price', value: priceDir, label: priceDir === 'asc' ? '价格升序' : '价格降序' },
    { key: 'reduce', field: 'reduce', value: 'desc', label: '降价优先' },
    { key: 'create', field: 'create', value: 'desc', label: '最新发布' },
  ]
}

function SurfaceCard({ children, className = '', style = {} }: any) {
  return (
    <div
      className={`mobile-surface-card ${className}`}
      style={style}
    >
      {children}
    </div>
  )
}

function SectionEyebrow({ children }: any) {
  return (
    <div
      className="text-[10px] font-semibold tracking-[0.18em] uppercase"
      style={{ color: 'color-mix(in srgb, var(--primary) 72%, var(--text-secondary))' }}
    >
      {children}
    </div>
  )
}

function FilterChip({ children, active = false, accent = 'var(--primary)' }: any) {
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-3 py-1.5 text-[11px] font-medium whitespace-nowrap"
      style={{
        color: active ? accent : 'var(--text-main)',
        background: active ? 'color-mix(in srgb, var(--primary) 10%, transparent)' : 'var(--mobile-glass-surface)',
        border: `1px solid ${active ? 'color-mix(in srgb, var(--primary) 18%, transparent)' : 'var(--glass-border)'}`,
      }}
    >
      {children}
    </span>
  )
}

function SearchResultCard({ item, isSelected, onToggleSelect }: any) {
  const visibleTags = [
    ...(item.serviceTags || []).slice(0, 2),
    ...(item.statTags || []).slice(0, 1),
    ...(item.infoTags || []).slice(0, 1),
  ].filter(Boolean).slice(0, 3)

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onToggleSelect}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          onToggleSelect()
        }
      }}
      className="rounded-[26px] p-3 cursor-pointer transition active:scale-[0.99]"
      style={{
        background: 'var(--surface-card)',
        border: isSelected ? '1.5px solid color-mix(in srgb, var(--primary) 42%, transparent)' : '1px solid var(--glass-border)',
        boxShadow: isSelected ? '0 18px 36px color-mix(in srgb, var(--primary) 12%, transparent)' : 'var(--glass-shadow, 0 14px 28px rgba(17,24,39,0.06))',
      }}
    >
      <div className="flex gap-3">
        <div
          className="relative shrink-0 overflow-hidden rounded-[20px]"
          style={{ width: 112, height: 118, background: 'var(--mobile-glass-surface)' }}
        >
          {item.picUrl ? (
            <img src={item.picUrl} alt="" className="w-full h-full object-cover block" loading="lazy" />
          ) : (
            <div className="w-full h-full flex items-center justify-center text-[20px]" style={{ color: 'var(--text-muted)' }}>
              ···
            </div>
          )}
          {item.freeShip ? (
            <span
              className="absolute top-2 left-2 rounded-full px-2 py-1 text-[10px] font-semibold"
              style={{ background: 'var(--warning)', color: 'var(--liquid-highlight)' }}
            >
              包邮
            </span>
          ) : null}
          <button
            type="button"
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              onToggleSelect()
            }}
            className="absolute top-2 right-2 z-10 h-7 min-w-7 rounded-full px-2 flex items-center justify-center cursor-pointer"
            style={{
              background: isSelected ? 'var(--primary)' : 'color-mix(in srgb, var(--text-main) 40%, transparent)',
              color: 'var(--liquid-highlight)',
              border: `1px solid ${isSelected ? 'var(--primary)' : 'var(--glass-border)'}`,
            }}
            aria-label={isSelected ? '取消选择' : '选择此商品'}
          >
            {isSelected ? <Check className="w-4 h-4" strokeWidth={3} /> : <span className="text-[10px] font-semibold">选</span>}
          </button>
        </div>

        <div className="min-w-0 flex-1 flex flex-col">
          <div className="min-w-0">
            <div className="mb-1 flex items-start justify-between gap-2">
              <div className="min-w-0 flex flex-wrap items-center gap-1.5">
                <FilterChip active={isSelected} accent="var(--primary)">
                  {isSelected ? '已加入待上架' : '待选择'}
                </FilterChip>
                {!isSelected ? (
                  <span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    点商品卡片或右上角勾选，可批量一键上架
                  </span>
                ) : null}
                {item.userExtraTags?.[0] ? (
                  <FilterChip accent="var(--primary)">{item.userExtraTags[0]}</FilterChip>
                ) : null}
              </div>
              <a
                href={`https://www.goofish.com/item?id=${item.itemId}`}
                target="_blank"
                rel="noreferrer"
                onClick={(e) => e.stopPropagation()}
                className="inline-flex shrink-0 items-center gap-1 text-[11px] font-medium"
                style={{ color: 'var(--primary)' }}
              >
                原帖
                <ExternalLink className="w-3 h-3" />
              </a>
            </div>
            <p className="text-[14px] font-semibold leading-[1.35] line-clamp-2" style={{ color: 'var(--text-main)' }}>
              {item.sellerBadgeUrl ? (
                <img
                  src={item.sellerBadgeUrl}
                  alt=""
                  className="inline-block align-middle mr-1"
                  style={{ height: 13, position: 'relative', top: -1 }}
                />
              ) : null}
              <SensitiveValue kind="item-title" value={item.title} />
            </p>
          </div>

          {visibleTags.length > 0 ? (
            <div className="mt-2 flex flex-wrap gap-1.5">
              {visibleTags.map((tag: string) => (
                <FilterChip key={tag}>{tag}</FilterChip>
              ))}
            </div>
          ) : null}

          <div className="mt-3 flex items-end justify-between gap-3">
            <div className="min-w-0">
              <div className="flex items-baseline gap-1.5">
                <span className="text-[20px] font-semibold leading-none" style={{ color: 'var(--danger)' }}>
                  <span className="text-[11px]">¥</span>
                  <SensitiveValue kind="money" value={item.price ?? '—'} />
                </span>
                {item.wantText ? (
                  <span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{item.wantText}</span>
                ) : null}
              </div>
              <div className="mt-1.5 flex items-center gap-1.5 min-w-0 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                {item.userAvatar ? (
                  <img src={item.userAvatar} alt="" className="w-4 h-4 rounded-full object-cover shrink-0" />
                ) : (
                  <div
                    className="w-4 h-4 rounded-full shrink-0 flex items-center justify-center text-[9px]"
                    style={{ background: 'color-mix(in srgb, var(--primary) 10%, transparent)', color: 'var(--primary)' }}
                  >
                    {(item.sellerNick || '?')[0]}
                  </div>
                )}
                <SensitiveValue kind="account" className="truncate" value={item.area || item.sellerNick || '未知卖家'} />
              </div>
            </div>

            <button
              type="button"
              onClick={(e) => {
                e.preventDefault()
                e.stopPropagation()
                onToggleSelect()
              }}
              className="shrink-0 rounded-full px-3 py-2 text-[11px] font-medium cursor-pointer"
              style={{
                background: isSelected ? 'var(--primary)' : 'color-mix(in srgb, var(--primary) 10%, transparent)',
                color: isSelected ? 'var(--liquid-highlight)' : 'var(--primary)',
              }}
            >
              {isSelected ? '取消选择' : '加入上架'}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

export default function MobileSearch() {
  const s = useSearchPageState()
  const [showSortMenu, setShowSortMenu] = useState(false)
  const [expandedPanel, setExpandedPanel] = useState<string | null>(null)
  const [showFiltersSheet, setShowFiltersSheet] = useState(false)
  const sentinelRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const sentinel = sentinelRef.current
    if (!sentinel) return
    const observer = new IntersectionObserver(entries => {
      if (entries[0].isIntersecting && s.hasMore && !s.loading && !s.loadingMore) {
        s.doSearch(undefined, { nextPage: s.page + 1 })
      }
    }, { rootMargin: '300px' })
    observer.observe(sentinel)
    return () => observer.disconnect()
  }, [s.hasMore, s.loading, s.loadingMore, s.page, s.doSearch])

  if (s.accountsLoading || !s.account) return null

  const sortOpts = getSortOptions(s.priceDir)
  const currentSortLabel = (() => {
    if (!s.sortField) return '综合排序'
    if (s.sortField === 'price') return s.sortValue === 'asc' ? '价格升序' : '价格降序'
    return sortOpts.find(opt => opt.field === s.sortField && opt.value === s.sortValue)?.label ?? '综合排序'
  })()

  function pickSort(opt: SortOption) {
    setShowSortMenu(false)
    if (opt.field === 'price') s.handleSort('price', '')
    else s.handleSort(opt.field, opt.value)
  }

  function submitSearch() {
    s.doSearch()
    inputRef.current?.blur()
  }

  function clearAllFilters() {
    s.clearFilters()
    s.setNearby(null)
  }

  const activeFilterCount = s.activeFilters.size + (s.nearby ? 1 : 0)
  const selectedCount = s.selectedIds.size
  const currentSourceLabel = SOURCE_MODES.find((mode) => mode.value === s.sourceMode)?.label ?? '自动'
  const selectedFilterLabels: string[] = []

  if (s.nearby?.name) selectedFilterLabels.push(s.nearby.name)
  s.activeFilters.forEach((valueId, propId) => {
    const panel = s.dynFilters.find((entry) => entry.propId === propId)
    const value = panel?.values.find((entry) => entry.valueId === valueId)
    if (value?.valueName) selectedFilterLabels.push(value.valueName)
    else if (panel?.propName) selectedFilterLabels.push(panel.propName)
  })

  return (
    <div className="h-full flex flex-col mobile-page-shell" style={{ backgroundColor: 'var(--bg-page)' }}>
      <div
        className="sticky top-0 z-30 px-3 pt-3 pb-3"
        style={{
          background: 'var(--mobile-glass-surface)',
          backdropFilter: 'saturate(180%) blur(18px)',
          WebkitBackdropFilter: 'saturate(180%) blur(18px)',
        }}
      >
        <SurfaceCard className="p-4">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <SectionEyebrow>搜索</SectionEyebrow>
              <div className="mt-1 text-[18px] font-semibold leading-none" style={{ color: 'var(--text-main)' }}>
                商品搜索
              </div>
            </div>
            <FilterChip active>{currentSourceLabel} 通道</FilterChip>
          </div>

          <div className="mt-4 flex items-center gap-2">
            <div
              className="flex-1 flex items-center gap-2 rounded-full px-3 h-11"
              style={{ border: '1px solid var(--glass-border)', background: 'var(--surface-card)' }}
            >
              <Search className="w-4 h-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
              <input
                ref={inputRef}
                value={s.keyword}
                onChange={e => s.setKeyword(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && submitSearch()}
                className="flex-1 bg-transparent outline-none text-[14px]"
                placeholder="搜索闲鱼商品"
                enterKeyHint="search"
              />
              {s.keyword ? (
                <button
                  type="button"
                  onClick={() => s.setKeyword('')}
                  className="w-6 h-6 rounded-full flex items-center justify-center cursor-pointer"
                  style={{ color: 'var(--text-muted)', background: 'color-mix(in srgb, var(--primary) 8%, transparent)' }}
                  aria-label="清除"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              ) : null}
            </div>

            <button
              onClick={submitSearch}
              disabled={!s.keyword.trim() || s.loading}
              className="h-11 px-4 rounded-full text-[13px] font-medium flex items-center gap-1 cursor-pointer disabled:opacity-40 shrink-0"
              style={{ background: 'var(--primary)', color: 'var(--liquid-highlight)', boxShadow: '0 10px 24px color-mix(in srgb, var(--primary) 22%, transparent)' }}
            >
              {s.loading ? <RefreshCw className="w-4 h-4 animate-spin" /> : '搜索'}
            </button>
          </div>

          <div className="mt-3 flex items-center gap-1.5 overflow-x-auto pb-0.5" style={{ scrollbarWidth: 'none' }}>
            {SOURCE_MODES.map((mode) => {
              const active = s.sourceMode === mode.value
              return (
                <button
                  key={mode.value}
                  onClick={() => s.changeSourceMode(mode.value)}
                  className="shrink-0 h-8 px-3 rounded-full text-[12px] font-medium cursor-pointer transition-colors"
                  style={{
                    background: active ? 'var(--primary)' : 'var(--mobile-glass-surface)',
                    color: active ? 'var(--liquid-highlight)' : 'var(--text-main)',
                    border: `1px solid ${active ? 'var(--primary)' : 'var(--glass-border)'}`,
                  }}
                >
                  {mode.label}
                </button>
              )
            })}
          </div>

          {s.searched ? (
            <>
              <div className="mt-4 flex items-center gap-2">
                <div className="relative flex-1">
                  <button
                    onClick={() => setShowSortMenu(v => !v)}
                    className="w-full h-10 px-3 rounded-[16px] text-[12.5px] font-medium flex items-center justify-between gap-1 cursor-pointer"
                    style={{ background: 'var(--surface-card)', border: '1px solid var(--glass-border)', color: 'var(--text-main)' }}
                  >
                    <span className="flex items-center gap-1.5">
                      <ArrowDownUp className="w-3.5 h-3.5" />
                      {currentSortLabel}
                    </span>
                    <ChevronDown className={`w-3.5 h-3.5 transition-transform ${showSortMenu ? 'rotate-180' : ''}`} />
                  </button>

                  {showSortMenu ? (
                    <>
                      <div className="fixed inset-0 z-30" onClick={() => setShowSortMenu(false)} />
                      <div
                        className="absolute left-0 right-0 top-full mt-1 z-40 rounded-[18px] py-1 overflow-hidden"
                        style={{
                          background: 'var(--mobile-glass-surface-strong, var(--surface-card))',
                          border: '1px solid var(--glass-border)',
                          boxShadow: 'var(--glass-shadow, 0 18px 42px rgba(17,24,39,0.10))',
                        }}
                      >
                        {sortOpts.map((opt) => {
                          const active = (!s.sortField && opt.field === '')
                            || (s.sortField === opt.field && (opt.field === 'price' || s.sortValue === opt.value))
                          return (
                            <button
                              key={opt.key}
                              onClick={() => pickSort(opt)}
                              className="w-full px-4 py-2.5 text-left text-[13px] flex items-center justify-between cursor-pointer"
                              style={{ color: active ? 'var(--primary)' : 'var(--text-main)', fontWeight: active ? 600 : 400 }}
                            >
                              {opt.label}
                              {active ? <Check className="w-4 h-4" /> : null}
                            </button>
                          )
                        })}
                      </div>
                    </>
                  ) : null}
                </div>

                <button
                  onClick={() => setShowFiltersSheet(true)}
                  className="shrink-0 h-10 px-3 rounded-[16px] text-[12.5px] font-medium flex items-center gap-1.5 cursor-pointer"
                  style={{
                    background: activeFilterCount > 0 ? 'color-mix(in srgb, var(--primary) 10%, transparent)' : 'var(--surface-card)',
                    border: `1px solid ${activeFilterCount > 0 ? 'color-mix(in srgb, var(--primary) 22%, transparent)' : 'var(--glass-border)'}`,
                    color: activeFilterCount > 0 ? 'var(--primary)' : 'var(--text-main)',
                  }}
                >
                  <SlidersHorizontal className="w-3.5 h-3.5" />
                  {activeFilterCount > 0 ? `筛选 ${activeFilterCount}` : '筛选'}
                </button>
              </div>
            </>
          ) : null}
        </SurfaceCard>
      </div>

      <div
        className="flex-1 overflow-y-auto mobile-page-body pt-1"
        style={{ paddingBottom: 'calc(env(safe-area-inset-bottom) + 176px)' }}
      >
        <div className="mobile-page-stack">
          {s.searched ? (
            <SurfaceCard className="p-4">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                    结果摘要
                  </div>
                </div>

                {(selectedCount > 0 || activeFilterCount > 0) ? (
                  <div className="flex items-center gap-2 shrink-0">
                    {selectedCount > 0 ? (
                      <button
                        onClick={s.clearSelection}
                        className="px-3 py-2 rounded-full text-[11px] font-medium cursor-pointer"
                        style={{ color: 'var(--text-main)', background: 'var(--mobile-glass-surface)' }}
                      >
                        清除选择
                      </button>
                    ) : null}
                    {activeFilterCount > 0 ? (
                      <button
                        onClick={clearAllFilters}
                        className="px-3 py-2 rounded-full text-[11px] font-medium cursor-pointer"
                        style={{ color: 'var(--danger)', background: 'color-mix(in srgb, var(--danger) 10%, transparent)' }}
                      >
                        清空筛选
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>

              <div className="mt-3 flex flex-wrap gap-1.5">
                <FilterChip active>{currentSourceLabel}</FilterChip>
                <FilterChip>{currentSortLabel}</FilterChip>
                {selectedFilterLabels.map((label, index) => (
                  <FilterChip key={`${label}-${index}`}>
                    {label === s.nearby?.name ? <MapPin className="w-3 h-3" /> : null}
                    {label}
                  </FilterChip>
                ))}
              </div>
            </SurfaceCard>
          ) : null}

          {showFiltersSheet && s.searched ? (
            <>
              <div
                className="fixed inset-0 z-[80]"
                style={{ background: 'color-mix(in srgb, var(--text-main) 24%, transparent)' }}
                onClick={() => setShowFiltersSheet(false)}
              />
              <div
                className="fixed inset-x-0 bottom-0 z-[90] rounded-t-[28px] px-4 pt-4 pb-6 max-h-[78vh] overflow-y-auto"
                style={{
                  background: 'var(--mobile-glass-surface-strong, var(--mobile-glass-surface))',
                  borderTop: '1px solid var(--glass-border)',
                  boxShadow: 'var(--glass-shadow, 0 -20px 48px rgba(17,24,39,0.18))',
                  backdropFilter: 'saturate(180%) blur(24px)',
                  WebkitBackdropFilter: 'saturate(180%) blur(24px)',
                }}
              >
                <div className="flex items-start justify-between gap-3 mb-4">
                  <div className="min-w-0">
                    <SectionEyebrow>筛选</SectionEyebrow>
                    <div className="mt-1 text-[18px] font-semibold" style={{ color: 'var(--text-main)' }}>
                      区域与筛选
                    </div>
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    {activeFilterCount > 0 ? (
                      <button
                        onClick={() => {
                          clearAllFilters()
                          setExpandedPanel(null)
                        }}
                        className="px-3 py-1.5 rounded-full text-[11px] font-medium cursor-pointer"
                        style={{ color: 'var(--danger)', background: 'color-mix(in srgb, var(--danger) 10%, transparent)' }}
                      >
                        清空
                      </button>
                    ) : null}
                    <button
                      onClick={() => setShowFiltersSheet(false)}
                      className="w-9 h-9 rounded-full flex items-center justify-center cursor-pointer"
                      style={{ background: 'var(--mobile-glass-surface)', color: 'var(--text-main)' }}
                      aria-label="关闭筛选"
                    >
                      <X className="w-4 h-4" />
                    </button>
                  </div>
                </div>

                <SurfaceCard className="p-3" style={{ background: 'var(--surface-card)' }}>
                  <div className="text-[12px] font-medium mb-2" style={{ color: 'var(--text-main)' }}>区域筛选</div>
                  <RegionFilterControl
                    uid={s.account.uid}
                    region={s.nearby}
                    onApply={(region) => {
                      s.setNearby(region)
                      if (s.searched && s.keyword.trim()) s.doSearch(undefined, { region })
                    }}
                  />
                </SurfaceCard>

                <div className="mt-3 space-y-2">
                  <div className="text-[12px] font-medium px-1" style={{ color: 'var(--text-main)' }}>动态筛选</div>
                  {s.dynFilters.map((panel) => {
                    const isMulti = panel.values.length > 1
                    const activeVal = s.activeFilters.get(panel.propId)

                    if (!isMulti) {
                      const value = panel.values[0]
                      const active = activeVal === value.valueId
                      return (
                        <button
                          key={panel.propId}
                          onClick={() => s.toggleFilter(panel.propId, value.valueId)}
                          className="w-full px-4 py-3 rounded-[20px] text-[13px] font-medium text-left cursor-pointer flex items-center justify-between"
                          style={{
                            background: active ? 'color-mix(in srgb, var(--primary) 10%, transparent)' : 'var(--surface-card)',
                            color: active ? 'var(--primary)' : 'var(--text-main)',
                            border: `1px solid ${active ? 'color-mix(in srgb, var(--primary) 20%, transparent)' : 'var(--glass-border)'}`,
                          }}
                        >
                          <span>{value.valueName}</span>
                          {active ? <Check className="w-4 h-4" /> : null}
                        </button>
                      )
                    }

                    const isOpen = expandedPanel === panel.propId
                    return (
                      <SurfaceCard key={panel.propId} className="p-3" style={{ background: 'var(--surface-card)' }}>
                        <button
                          onClick={() => setExpandedPanel(isOpen ? null : panel.propId)}
                          className="w-full text-left flex items-center justify-between gap-3 cursor-pointer"
                          style={{ color: activeVal ? 'var(--primary)' : 'var(--text-main)' }}
                        >
                          <div className="min-w-0">
                            <div className="text-[13px] font-semibold">{panel.propName}</div>
                            <div className="mt-1 text-[11px] truncate" style={{ color: 'var(--text-secondary)' }}>
                              {activeVal
                                ? `当前: ${panel.values.find(v => v.valueId === activeVal)?.valueName ?? '已选择'}`
                                : `共 ${panel.values.length} 个选项`}
                            </div>
                          </div>
                          <ChevronDown className={`w-4 h-4 shrink-0 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
                        </button>

                        {isOpen ? (
                          <div className="mt-3 flex flex-wrap gap-1.5">
                            {panel.values.map((value) => {
                              const active = activeVal === value.valueId
                              return (
                                <button
                                  key={value.valueId}
                                  onClick={() => {
                                    s.toggleFilter(panel.propId, value.valueId)
                                    setExpandedPanel(null)
                                  }}
                                  className="px-3 py-1.5 rounded-full text-[12px] font-medium cursor-pointer"
                                  style={{
                                    background: active ? 'var(--primary)' : 'var(--mobile-glass-surface)',
                                    color: active ? 'var(--liquid-highlight)' : 'var(--text-main)',
                                  }}
                                >
                                  {value.valueName}
                                </button>
                              )
                            })}
                          </div>
                        ) : null}
                      </SurfaceCard>
                    )
                  })}
                </div>
              </div>
            </>
          ) : null}

          {s.error ? (
            <SurfaceCard
              className="px-4 py-3 text-[13px] flex items-center gap-2"
              style={{ background: 'color-mix(in srgb, var(--danger) 10%, var(--surface-card))', borderColor: 'color-mix(in srgb, var(--danger) 18%, transparent)', color: 'var(--danger)' }}
            >
              <AlertCircle className="w-4 h-4 shrink-0" />
              {s.error}
            </SurfaceCard>
          ) : null}

          {s.loading ? (
            <div className="flex items-center justify-center py-24 gap-2 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
              <RefreshCw className="w-4 h-4 animate-spin" />
              搜索中…
            </div>
          ) : !s.searched ? (
            <SurfaceCard className="p-4">
              <EmptyState icon={Search} title="输入关键词开始搜索" description="开始搜索" />
            </SurfaceCard>
          ) : !s.results.length && !s.error ? (
            <SurfaceCard className="p-4">
              <EmptyState icon={Search} title="没有搜索结果" description={`没找到「${s.keyword}」相关的商品`} />
            </SurfaceCard>
          ) : (
            <>
              <div className="flex items-center justify-between px-1">
                <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>搜索结果</div>
                <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                  {s.results.length} 条结果
                </div>
              </div>

              <div className="space-y-3">
                {s.results.map((item, index) => (
                  <SearchResultCard
                    key={`${item.itemId}-${index}`}
                    item={item}
                    isSelected={s.selectedIds.has(item.itemId)}
                    onToggleSelect={() => s.toggleSelect(item.itemId)}
                  />
                ))}
              </div>
            </>
          )}

          {s.searched && s.results.length > 0 ? (
            <div ref={sentinelRef} className="flex items-center justify-center py-6 gap-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              {s.loadingMore ? (
                <>
                  <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                  加载更多…
                </>
              ) : s.hasMore ? (
                <span>下滑加载更多</span>
              ) : (
                <span>没有更多结果了</span>
              )}
            </div>
          ) : null}
        </div>
      </div>

      {s.searched && selectedCount > 0 ? (
        <div
          className="fixed left-3 right-3 z-[70] rounded-[24px] px-4 py-3 flex items-center gap-3"
          style={{
            bottom: 'calc(env(safe-area-inset-bottom) + 88px)',
            background: 'var(--mobile-glass-surface-strong, var(--mobile-glass-surface))',
            color: 'var(--text-main)',
            border: '1px solid var(--glass-border)',
            boxShadow: 'var(--glass-shadow, 0 16px 36px rgba(17,24,39,0.18))',
            backdropFilter: 'saturate(180%) blur(24px)',
            WebkitBackdropFilter: 'saturate(180%) blur(24px)',
          }}
        >
          <div className="min-w-0">
            <div className="text-[13px] font-medium">{`已选 ${selectedCount} 件商品`}</div>
            <div className="text-[10px] mt-0.5" style={{ color: 'var(--text-secondary)' }}>
              可直接导入并上架到你的账号
            </div>
          </div>
          <div className="flex-1" />
          <button
            onClick={s.clearSelection}
            className="text-[12px] px-3 py-2 rounded-full cursor-pointer shrink-0"
            style={{ background: 'var(--mobile-glass-surface)', color: 'var(--text-main)' }}
          >
            清除
          </button>
          <button
            onClick={() => s.setShowBatchList(true)}
            className="text-[12.5px] px-3 py-2 rounded-full font-medium flex items-center gap-1 cursor-pointer shrink-0"
            style={{ background: 'var(--primary)', color: 'var(--liquid-highlight)' }}
          >
            <ShoppingBag className="w-3.5 h-3.5" />
            一键上架
          </button>
        </div>
      ) : null}

      {s.showBatchList && s.selectedItems.length > 0 ? (
        <BatchListModal
          items={s.selectedItems}
          accounts={s.activeAccounts}
          onClose={() => s.setShowBatchList(false)}
        />
      ) : null}
    </div>
  )
}
