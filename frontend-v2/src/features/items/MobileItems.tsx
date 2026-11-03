// @ts-nocheck
// Mobile items page — shares all business logic with DesktopItems via
// useItemsPageState. Render layer is mobile-first:
//   - 2-col grid for cards (reuse ItemCard with a tap wrapper)
//   - Long-press to enter select mode
//   - Tap (when not in select mode) opens a bottom action sheet for the item
//   - Sticky top bar with sync + search + status tabs + sort + publish + select toggle
//   - Bottom fixed batch action bar
//   - Account chip strip remains a horizontal scroll row
import { useRef, useState } from 'react'
import {
  Package, RefreshCw, Plus, Search as SearchIcon, X, CheckSquare,
  Sparkles, TrendingDown, Coins, ArrowDownToLine, UploadCloud, Trash2, CheckCheck,
} from 'lucide-react'

import { TopBar } from '../../components/TopBar'
import { SensitiveValue, SensitiveVisibilityToggle } from '../../components/SensitiveValue'
import { EmptyState } from '../../components/EmptyState'
import { useNotify } from '../../components/Notify'

import { isOnSaleListed } from './itemActions'
import { colorForAccount, ITEM_SORT_OPTIONS } from './itemUtils'
import { ItemCard } from './ItemCard'
import { DialogModal, GrouponSheetModal } from './DialogModal'
import { EditItemModal } from './EditItemModal'
import { useItemsPageState } from './hooks/useItemsPageState'
import { MobileItemActionSheet } from './mobile/MobileActionSheet'
import {
  PriceReduceSheet,
  CoinDeductionSheet,
  KnifeSheet,
  PublishCheckSheet,
} from './mobile/ItemSheets'
import { PublishWizardSheet } from './mobile/PublishWizardSheet'

export default function MobileItems() {
  const notify = useNotify()
  const s = useItemsPageState()
  const {
    accounts, activeAccounts,
    groups, allItems, filtered, counts,
    loading, syncing, polishingUids, polishedUids, loadingSoldOut,
    batchRunning, batchProgress,
    accountFilter, setAccountFilter,
    statusFilter, setStatusFilter,
    searchQuery, setSearchQuery,
    sortKey, setSortKey,
    selectMode, enterSelectMode, exitSelectMode,
    selectedKeys, toggleSelect, toggleSelectAll, itemKey,
    editingItem, setEditingItem,
    publishOpen, setPublishOpen,
    dialog, grouponSheet, setGrouponSheet, closeGrouponSheet,
    doSync, doSyncAll, doPolish, doPolishAll,
    doReducePrice, doDownshelf, doUpshelf, doDelete, doShare,
    doToggleCoin, doToggleGroupon, submitGrouponSheet,
    doBatchDelete, doBatchDownshelf, doBatchUpshelf,
    doBatchReducePrice, doBatchCoinOpen, doBatchCoinClose,
    doBatchGrouponOpen, doBatchGrouponClose,
    runBatchPublish, doEditItem,
  } = s

  const [actionItem, setActionItem] = useState<any>(null)
  const [showFilters, setShowFilters] = useState(false)
  const [priceSheetItem, setPriceSheetItem] = useState<any>(null)
  const [coinSheetItem, setCoinSheetItem] = useState<any>(null)
  const [knifeSheetItem, setKnifeSheetItem] = useState<any>(null)
  const [publishCheckItem, setPublishCheckItem] = useState<any>(null)

  const longPressTimer = useRef<any>(null)
  const longPressFired = useRef(false)

  function startLongPress(it: any) {
    if (selectMode) return
    longPressFired.current = false
    longPressTimer.current = setTimeout(() => {
      longPressFired.current = true
      enterSelectMode()
      setTimeout(() => toggleSelect(it), 0)
      if (navigator.vibrate) navigator.vibrate(15)
    }, 380)
  }

  function cancelLongPress() {
    if (longPressTimer.current) clearTimeout(longPressTimer.current)
    longPressTimer.current = null
  }

  function handleTap(it: any) {
    if (longPressFired.current) {
      longPressFired.current = false
      return
    }
    if (selectMode) toggleSelect(it)
    else setActionItem(it)
  }

  async function reloadItem(uid: string) {
    await doSync(uid, { silent: true } as any)
  }

  function openReduceSheet(uid: string, item: any) {
    setActionItem(null)
    setPriceSheetItem({ ...item, _uid: uid })
  }

  function openCoinSheet(uid: string, item: any) {
    setActionItem(null)
    setCoinSheetItem({ ...item, _uid: uid })
  }

  function openKnifeSheet(uid: string, item: any) {
    setActionItem(null)
    setKnifeSheetItem({ ...item, _uid: uid })
  }

  function openPublishCheckSheet(uid: string, item: any) {
    setActionItem(null)
    setPublishCheckItem({ ...item, _uid: uid })
  }

  if (!activeAccounts.length && !accounts.length) {
    return (
      <div className="mobile-page-shell" style={{ backgroundColor: 'var(--bg-page)' }}>
        <TopBar title="商品" />
        <div className="mobile-page-body">
          <div className="mobile-page-stack">
            <div className="mobile-surface-card flex min-h-[220px] items-center justify-center px-6">
              <EmptyState icon={Package} title="加载账号中…" description="" />
            </div>
          </div>
        </div>
      </div>
    )
  }

  if (!activeAccounts.length) {
    return (
      <div className="mobile-page-shell" style={{ backgroundColor: 'var(--bg-page)' }}>
        <TopBar title="商品" />
        <div className="mobile-page-body">
          <div className="mobile-page-stack">
            <div className="mobile-surface-card flex min-h-[220px] items-center justify-center px-6">
              <EmptyState icon={Package} title="暂无活跃账号" description="请先在账号管理中添加并登录账号" />
            </div>
          </div>
        </div>
      </div>
    )
  }

  const totalViews = allItems.reduce((acc: number, it: any) => acc + Number(it.browseCount ?? it.viewCount ?? 0), 0)
  const totalLikes = allItems.reduce((acc: number, it: any) => acc + Number(it.wantCount ?? it.favorCount ?? 0), 0)
  const onsaleList = allItems
    .filter(isOnSaleListed)
    .map((it: any) => parseFloat(it.price || '0') || 0)
    .filter((v) => v > 0)
  const minPrice = onsaleList.length ? Math.min(...onsaleList) : 0
  const anyPolishing = Object.values(polishingUids).some(Boolean)

  return (
    <div className="mobile-page-shell" style={{ backgroundColor: 'var(--surface-soft)' }}>
      <div className="mobile-page-body">
        <div className="mobile-page-stack">
          {groups.length ? (
            <section className="mobile-surface-card px-4 py-4">
              <div className="mobile-section-head items-start gap-2">
                <div className="mobile-section-copy">
                  <h2 className="mobile-section-title">概况</h2>
                </div>
                <div className="ml-auto flex shrink-0 items-center gap-1.5">
                  <SensitiveVisibilityToggle compact />
                  {selectMode ? (
                    <>
                      <span className="mobile-pill mobile-muted-pill">已选 {selectedKeys.size}</span>
                      <button
                        onClick={exitSelectMode}
                        className="btn-ghost btn-sm tap-target"
                        aria-label="退出多选"
                        title="退出多选"
                      >
                        <X className="w-4 h-4" />
                      </button>
                    </>
                  ) : (
                    <>
                      <span className="mobile-pill mobile-muted-pill">已筛选</span>
                      <button
                        onClick={doSyncAll}
                        disabled={syncing}
                        className="btn-ghost btn-sm tap-target disabled:opacity-40"
                        aria-label="同步全部账号"
                        title="同步全部账号"
                      >
                        <RefreshCw className={`w-4 h-4 ${syncing ? 'animate-spin' : ''}`} />
                      </button>
                      <button
                        onClick={doPolishAll}
                        disabled={anyPolishing}
                        className="btn-ghost btn-sm tap-target disabled:opacity-40"
                        aria-label="一键擦亮"
                        title={accountFilter ? '对当前账号全部商品擦亮' : '一键擦亮'}
                      >
                        <Sparkles className={`w-4 h-4 ${anyPolishing ? 'animate-spin' : ''}`} />
                      </button>
                      <button
                        onClick={enterSelectMode}
                        className="btn-ghost btn-sm tap-target"
                        aria-label="多选"
                        title="批量选择"
                      >
                        <CheckSquare className="w-4 h-4" />
                      </button>
                    </>
                  )}
                </div>
              </div>

              <div className="mt-4 flex gap-2 overflow-x-auto pb-1 [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden">
                {[
                  { label: '商品', value: allItems.length, sub: `${groups.length} 账号` },
                  { label: '在售', value: counts.onsale, sub: '含审核中' },
                  { label: '浏览', value: totalViews.toLocaleString(), sub: '累计' },
                  { label: '想要', value: totalLikes.toLocaleString(), sub: '累计' },
                  { label: '最低价', value: minPrice ? `¥${minPrice}` : '—', sub: '在售' },
                ].map((k) => (
                  <div key={k.label} className="mobile-soft-card min-w-[110px] shrink-0 px-3 py-3">
                    <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>{k.label}</div>
                    <div className="mt-1 text-[18px] font-semibold leading-none" style={{ color: 'var(--text-main)' }}>{k.value}</div>
                    <div className="mt-1 text-[10px]" style={{ color: 'var(--text-secondary)' }}>{k.sub}</div>
                  </div>
                ))}
              </div>

              <div className="mt-4 flex gap-2 overflow-x-auto pb-1 [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden">
                <button
                  onClick={() => setAccountFilter(null)}
                  className={`mobile-pill shrink-0 cursor-pointer ${accountFilter == null ? '' : 'mobile-muted-pill'}`}
                  style={accountFilter == null ? { background: 'var(--primary-light)', color: 'var(--primary)', borderColor: 'color-mix(in srgb, var(--primary) 18%, transparent)' } : undefined}
                >
                  全部 ({allItems.length})
                </button>
                {groups.map((g: any) => {
                  const active = accountFilter === g.uid
                  const color = colorForAccount(g.uid)
                  const count = allItems.filter((i: any) => i._uid === g.uid).length
                  const polishing = !!polishingUids[g.uid]
                  const polished = !!polishedUids[g.uid]
                  return (
                    <button
                      key={g.uid}
                      onClick={() => setAccountFilter(active ? null : g.uid)}
                      className={`mobile-pill shrink-0 cursor-pointer ${active ? '' : 'mobile-muted-pill'}`}
                      style={active ? { borderColor: `${color}33`, background: `${color}18`, color } : undefined}
                    >
                      <span className="h-2 w-2 rounded-full" style={{ backgroundColor: active ? color : color }} />
                      <SensitiveValue kind="account" className="max-w-[88px] truncate" value={g.nick || g.uid} />
                      <span className="font-mono opacity-70">({count})</span>
                      {polishing ? <Sparkles className="w-3 h-3 animate-spin" /> : null}
                      {!polishing && polished ? (
                        <span
                          className="rounded-full px-1.5 py-[1px] text-[10px] font-semibold"
                          style={{ color: 'var(--status-success-text)', background: 'var(--status-success-bg)' }}
                        >
                          已擦亮
                        </span>
                      ) : null}
                    </button>
                  )
                })}
              </div>
            </section>
          ) : null}

          <section className="mobile-surface-card overflow-hidden">
            <div className="border-b px-4 py-3" style={{ borderColor: 'var(--border-light)' }}>
              <div className="flex items-center gap-2">
                <div className="glass-control flex items-center gap-0.5 rounded-[12px] p-[4px]">
                  {[
                    { v: 'onsale', label: '在售', n: counts.onsale },
                    { v: 'off', label: '已下架', n: counts.off },
                  ].map((t) => (
                    <button
                      key={t.v}
                      onClick={() => {
                        setStatusFilter(t.v)
                        exitSelectMode()
                      }}
                      className="rounded-[10px] px-3 py-[7px] text-[13px] font-medium cursor-pointer transition-all"
                      style={{
                        background: statusFilter === t.v ? 'var(--surface-control)' : 'transparent',
                        color: statusFilter === t.v ? 'var(--text-main)' : 'var(--text-secondary)',
                        boxShadow: statusFilter === t.v ? 'inset 0 1px 0 rgba(255,255,255,0.62), 0 8px 18px rgba(25,35,55,0.08)' : 'none',
                      }}
                    >
                      {t.label} <span className="font-mono opacity-60">{t.n}</span>
                    </button>
                  ))}
                </div>

                <button
                  onClick={() => setShowFilters((v) => !v)}
                  className="btn-ghost btn-sm ml-auto"
                  aria-label="搜索/排序"
                >
                  <SearchIcon className="w-4 h-4" />
                </button>
                {accountFilter ? (
                  <button
                    onClick={() => doSync(accountFilter)}
                    className="btn-ghost btn-sm"
                    aria-label="同步当前账号"
                    title="同步当前账号"
                  >
                    <RefreshCw className="w-4 h-4" />
                  </button>
                ) : null}
              </div>

              {showFilters || searchQuery ? (
                <div className="mt-3 flex gap-2">
                  <div className="relative flex-1">
                    <SearchIcon className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4" style={{ color: 'var(--text-muted)' }} />
                    <input
                      className="input pl-9"
                      placeholder="搜索商品标题 / ID"
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                    />
                    {searchQuery ? (
                      <button
                        onClick={() => setSearchQuery('')}
                        className="absolute right-2 top-1/2 -translate-y-1/2 w-7 h-7 flex items-center justify-center rounded-full"
                      >
                        <X className="w-3.5 h-3.5" style={{ color: 'var(--text-muted)' }} />
                      </button>
                    ) : null}
                  </div>

                  <select
                    value={sortKey}
                    onChange={(e) => setSortKey(e.target.value)}
                    className="rounded-[12px] border px-2.5 py-[6px] text-[13px] cursor-pointer"
                    style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}
                  >
                    {ITEM_SORT_OPTIONS.map((o) => (
                      <option key={o.value} value={o.value}>{o.shortLabel}</option>
                    ))}
                  </select>
                </div>
              ) : null}
            </div>

            {batchProgress ? (
              <div
                className="mx-4 mt-3 flex items-center gap-2 rounded-[14px] px-3 py-2"
                style={{ background: 'var(--status-warning-bg)', border: '1px solid color-mix(in srgb, var(--warning) 22%, transparent)', color: 'var(--status-warning-text)' }}
              >
                <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                <span className="text-[12.5px] font-medium">{batchProgress.message}</span>
                <span className="text-[11px] opacity-75 ml-auto">共 {batchProgress.total} 项</span>
              </div>
            ) : null}

            {selectMode ? (
              <div className="mx-4 mt-3 flex items-center gap-2 rounded-[14px] px-3 py-2" style={{ background: 'var(--primary-light)', border: '1px solid color-mix(in srgb, var(--primary) 18%, transparent)' }}>
                <button onClick={toggleSelectAll} className="flex items-center gap-1.5 text-[13px] font-medium cursor-pointer" style={{ color: 'var(--primary)' }}>
                  <CheckCheck className="w-4 h-4" />
                  {selectedKeys.size === filtered.length && filtered.length > 0 ? '取消全选' : '全选'}
                </button>
                <span className="text-[12px] ml-auto" style={{ color: 'var(--text-secondary)' }}>已选 {selectedKeys.size} / {filtered.length}</span>
              </div>
            ) : null}

            <div
              className="px-3 pt-3"
              style={{ paddingBottom: selectMode && selectedKeys.size > 0 ? '168px' : '96px' }}
            >
              {loading && !groups.length ? (
                <div className="flex items-center justify-center py-24 gap-2" style={{ color: 'var(--text-secondary)' }}>
                  <RefreshCw className="w-4 h-4 animate-spin" /> 正在加载商品…
                </div>
              ) : !groups.length ? (
                <div className="mobile-soft-card flex min-h-[220px] items-center justify-center px-6">
                  <EmptyState icon={Package} title="暂无商品" description="点击右上角同步图标拉取商品">
                    <button onClick={doSyncAll} className="btn-primary btn-sm mt-4">
                      <RefreshCw className="w-3.5 h-3.5" /> 同步商品
                    </button>
                  </EmptyState>
                </div>
              ) : filtered.length === 0 ? (
                <div className="mobile-soft-card flex min-h-[220px] items-center justify-center px-6">
                  <EmptyState icon={Package} title="暂无匹配的商品" description="试试切换账号 / 状态 / 搜索条件" />
                </div>
              ) : (
                <>
                  {statusFilter === 'off' && groups.some((g: any) => loadingSoldOut[g.uid]) ? (
                    <div className="liquid-muted-card mb-2 rounded-[14px] py-2 text-center text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      <RefreshCw className="mr-1 inline h-3.5 w-3.5 animate-spin" /> 加载中
                    </div>
                  ) : null}

                  <div className="grid grid-cols-2 gap-2.5">
                    {filtered.map((it: any) => (
                      <div
                        key={`${it._uid}-${it.itemId}-${it._sold ? 'sold' : 'onsale'}`}
                        onPointerDown={() => startLongPress(it)}
                        onPointerUp={cancelLongPress}
                        onPointerLeave={cancelLongPress}
                        onPointerCancel={cancelLongPress}
                        onClick={() => handleTap(it)}
                        className="mobile-soft-card overflow-hidden cursor-pointer select-none"
                      >
                        <ItemCard
                          it={it}
                          selectMode={selectMode}
                          selected={selectedKeys.has(itemKey(it))}
                          onToggleSelect={() => toggleSelect(it)}
                          onDown={doDownshelf}
                          onUp={doUpshelf}
                          onDel={doDelete}
                          onReduce={doReducePrice}
                          onEdit={doEditItem}
                          onShare={doShare}
                          onToggleCoin={doToggleCoin}
                          onToggleGroupon={doToggleGroupon}
                        />
                      </div>
                    ))}
                  </div>
                </>
              )}
            </div>
          </section>
        </div>
      </div>

      {!selectMode && (
        <button
          onClick={() => setPublishOpen(true)}
          className="fixed right-4 z-[70] w-14 h-14 rounded-full flex items-center justify-center cursor-pointer active:scale-95 transition-transform liquid-fab"
          style={{
            bottom: 'calc(env(safe-area-inset-bottom) + 96px)',
          }}
          aria-label="发布商品"
        >
          <Plus className="w-6 h-6" />
        </button>
      )}

      {selectMode && selectedKeys.size > 0 ? (
        <div
          className="glass-dock fixed left-3 right-3 z-[60] flex gap-1.5 overflow-x-auto rounded-[24px] border px-3 pt-3 pb-3 [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden"
          style={{
            bottom: 'calc(env(safe-area-inset-bottom) + 96px)',
            borderColor: 'var(--glass-border)',
          }}
        >
          {statusFilter === 'onsale' ? (
            <>
              <button onClick={doBatchDownshelf} disabled={batchRunning} className="btn-secondary btn-sm shrink-0 disabled:opacity-50">
                <ArrowDownToLine className="w-4 h-4" /> 下架
              </button>
              <button onClick={doBatchReducePrice} disabled={batchRunning} className="btn-secondary btn-sm shrink-0 disabled:opacity-50">
                <TrendingDown className="w-4 h-4" /> 降价
              </button>
              <button onClick={doBatchGrouponOpen} disabled={batchRunning} className="btn-secondary btn-sm shrink-0 disabled:opacity-50">
                <TrendingDown className="w-4 h-4" /> 开小刀
              </button>
              <button onClick={doBatchGrouponClose} disabled={batchRunning} className="btn-secondary btn-sm shrink-0 disabled:opacity-50">
                <TrendingDown className="w-4 h-4" /> 关小刀
              </button>
              <button onClick={doBatchCoinOpen} disabled={batchRunning} className="btn-secondary btn-sm shrink-0 disabled:opacity-50">
                <Coins className="w-4 h-4" /> 开闲鱼币
              </button>
              <button onClick={doBatchCoinClose} disabled={batchRunning} className="btn-secondary btn-sm shrink-0 disabled:opacity-50">
                <Coins className="w-4 h-4" /> 关闲鱼币
              </button>
            </>
          ) : (
            <button onClick={doBatchUpshelf} disabled={batchRunning} className="btn-secondary btn-sm shrink-0 disabled:opacity-50">
              <UploadCloud className="w-4 h-4" /> 上架
            </button>
          )}

          <button onClick={doBatchDelete} disabled={batchRunning} className="btn-danger btn-sm shrink-0 ml-auto disabled:opacity-50">
            <Trash2 className="w-4 h-4" /> 删除 ({selectedKeys.size})
          </button>
        </div>
      ) : null}

      <MobileItemActionSheet
        open={!!actionItem}
        item={actionItem}
        onClose={() => setActionItem(null)}
        onReduce={openReduceSheet}
        onDown={doDownshelf}
        onUp={doUpshelf}
        onEdit={doEditItem}
        onShare={doShare}
        onDelete={doDelete}
        onToggleCoin={openCoinSheet}
        onToggleGroupon={openKnifeSheet}
        onPublishCheck={openPublishCheckSheet}
        onPolish={doPolish}
      />

      {editingItem ? (
        <EditItemModal
          uid={editingItem.uid}
          item={editingItem.item}
          mode={editingItem.mode || 'edit'}
          onClose={() => setEditingItem(null)}
          onSaved={async () => {
            const uid = editingItem.uid
            setEditingItem(null)
            await doSync(uid, { silent: true })
          }}
        />
      ) : null}

      {publishOpen ? (
        <PublishWizardSheet
          open={publishOpen}
          accounts={activeAccounts}
          onClose={() => setPublishOpen(false)}
          onPublished={async () => {
            setPublishOpen(false)
            await doSyncAll()
          }}
          notify={notify}
          onBatchPublish={runBatchPublish}
        />
      ) : null}

      <PriceReduceSheet
        open={!!priceSheetItem}
        item={priceSheetItem}
        onClose={() => setPriceSheetItem(null)}
        onReload={async () => {
          if (priceSheetItem?._uid) await reloadItem(priceSheetItem._uid)
        }}
        notify={notify}
      />

      <CoinDeductionSheet
        open={!!coinSheetItem}
        item={coinSheetItem}
        onClose={() => setCoinSheetItem(null)}
        onReload={async () => {
          if (coinSheetItem?._uid) await reloadItem(coinSheetItem._uid)
        }}
        notify={notify}
      />

      <KnifeSheet
        open={!!knifeSheetItem}
        item={knifeSheetItem}
        onClose={() => setKnifeSheetItem(null)}
        onReload={async () => {
          if (knifeSheetItem?._uid) await reloadItem(knifeSheetItem._uid)
        }}
        notify={notify}
      />

      <PublishCheckSheet
        open={!!publishCheckItem}
        item={publishCheckItem}
        onClose={() => setPublishCheckItem(null)}
        notify={notify}
      />

      {dialog ? <DialogModal dialog={dialog} /> : null}
      {grouponSheet ? (
        <GrouponSheetModal
          sheet={grouponSheet}
          onClose={closeGrouponSheet}
          onSelect={(value) => setGrouponSheet((prev: any) => (prev ? { ...prev, selectedPrice: value } : prev))}
          onCustomPriceChange={(value) => setGrouponSheet((prev: any) => (prev ? { ...prev, customPrice: value } : prev))}
          onSubmit={submitGrouponSheet}
        />
      ) : null}
    </div>
  )
}
