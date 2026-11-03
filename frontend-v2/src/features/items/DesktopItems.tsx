// @ts-nocheck
// Desktop items page — render-only. All business state + handlers come from
// useItemsPageState (shared with MobileItems).
import { Package, RefreshCw } from "lucide-react";
import { EmptyState } from "../../components/EmptyState";
import { SensitiveVisibilityToggle } from "../../components/SensitiveValue";

import { isOnSaleListed } from "./itemActions";
import { ItemCard } from "./ItemCard";
import { DialogModal, GrouponSheetModal } from "./DialogModal";
import { EditItemModal } from "./EditItemModal";
import { PublishItemModal } from "./PublishItemModal";
import { ItemAccountMultiSelect, ItmKpi } from "./ItemKpi";
import { ItemToolbar, BatchBar } from "./ItemToolbar";
import { colorForAccount } from "./itemUtils";
import { useItemsPageState } from "./hooks/useItemsPageState";

export default function DesktopItems() {
  const s = useItemsPageState();
  const {
    activeAccounts,
    groups,
    allItems,
    accountScopedItems,
    filtered,
    counts,
    loading,
    syncing,
    syncingUids,
    polishingUids,
    loadingSoldOut,
    batchRunning,
    batchProgress,
    desktopAccountFilterUids,
    setDesktopAccountFilterUids,
    statusFilter,
    setStatusFilter,
    searchQuery,
    setSearchQuery,
    sortKey,
    setSortKey,
    selectMode,
    enterSelectMode,
    exitSelectMode,
    selectedKeys,
    toggleSelect,
    toggleSelectAll,
    itemKey,
    editingItem,
    setEditingItem,
    publishOpen,
    setPublishOpen,
    dialog,
    grouponSheet,
    setGrouponSheet,
    closeGrouponSheet,
    doSync,
    doSyncAll,
    doSyncCurrentAccounts,
    doPolishAll,
    doReducePrice,
    doDownshelf,
    doUpshelf,
    doDelete,
    doShare,
    doToggleCoin,
    doToggleGroupon,
    submitGrouponSheet,
    doBatchDelete,
    doBatchDownshelf,
    doBatchUpshelf,
    doBatchReducePrice,
    doBatchCoinOpen,
    doBatchCoinClose,
    doBatchGrouponOpen,
    doBatchGrouponClose,
    runBatchPublish,
    doEditItem,
    notify,
  } = s;

  const selectedAccountUids = desktopAccountFilterUids ?? [];
  const selectedAccountCount = selectedAccountUids.length;
  const accountOptions = activeAccounts.map((account) => ({
    uid: account.uid,
    nick: account.nick || account.uid,
    color: colorForAccount(account.uid),
    total: allItems.filter((item) => item._uid === account.uid).length,
  }));
  const syncingCurrentAccounts = selectedAccountCount
    ? selectedAccountUids.some((uid) => syncingUids[uid])
    : syncing || Object.values(syncingUids).some(Boolean);

  return (
    <>
      {/* 统一卡片栅格：8 列 @ 桌面 / 响应式收窄 */}
      <style>{`
        .items-grid{
          display:grid;
          grid-template-columns: repeat(2, minmax(0, 1fr));
          gap:10px;
          grid-auto-rows: 1fr;
        }
        @media (min-width:640px){ .items-grid{ grid-template-columns: repeat(3, minmax(0, 1fr)); gap:11px } }
        @media (min-width:768px){ .items-grid{ grid-template-columns: repeat(4, minmax(0, 1fr)); gap:12px } }
        @media (min-width:1024px){ .items-grid{ grid-template-columns: repeat(6, minmax(0, 1fr)) } }
        @media (min-width:1280px){ .items-grid{ grid-template-columns: repeat(8, minmax(0, 1fr)) } }
        @media (min-width:1536px){ .items-grid{ grid-template-columns: repeat(8, minmax(0, 1fr)) } }
        .items-grid > *{ width:100%; height:100%; margin:0; min-width:0 }
      `}</style>

      <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6">
        <section
          aria-labelledby="desktop-items-heading"
          className="surface-card mb-4 rounded-[24px] px-5 py-4"
        >
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0">
              <h1
                id="desktop-items-heading"
                className="text-[20px] font-semibold tracking-[-0.02em]"
                style={{ color: "var(--text-main)" }}
              >
                商品
              </h1>
            </div>
            <div className="flex items-center gap-2">
              <SensitiveVisibilityToggle compact />
              <button
                onClick={doSyncCurrentAccounts}
                disabled={syncingCurrentAccounts}
                className="btn-ghost btn-sm disabled:opacity-40"
              >
                <RefreshCw
                  className={`w-3.5 h-3.5 ${syncingCurrentAccounts ? "animate-spin" : ""}`}
                />{" "}
                同步
              </button>
            </div>
          </div>

          {/* KPI 条 */}
          {groups.length
            ? (() => {
                const totalViews = accountScopedItems.reduce(
                  (s, it) => s + Number(it.browseCount ?? it.viewCount ?? 0),
                  0,
                );
                const totalLikes = accountScopedItems.reduce(
                  (s, it) => s + Number(it.wantCount ?? it.favorCount ?? 0),
                  0,
                );
                const onsaleList = accountScopedItems
                  .filter(isOnSaleListed)
                  .map((it) => parseFloat(it.price || "0") || 0)
                  .filter((v) => v > 0);
                const minPrice = onsaleList.length
                  ? Math.min(...onsaleList)
                  : 0;
                return (
                  <div className="mt-4 grid grid-cols-2 gap-2.5 md:grid-cols-5">
                    <ItmKpi
                      label="商品总数"
                      value={accountScopedItems.length}
                      sub={`${selectedAccountCount || groups.length} 个账号`}
                    />
                    <ItmKpi
                      label="在售"
                      value={accountScopedItems.filter(isOnSaleListed).length}
                      sub="含审核中"
                    />
                    <ItmKpi
                      label="浏览"
                      value={totalViews.toLocaleString()}
                      sub="累计"
                    />
                    <ItmKpi
                      label="想要"
                      value={totalLikes.toLocaleString()}
                      sub="累计"
                    />
                    <ItmKpi
                      label="最低价"
                      value={minPrice ? `¥${minPrice}` : "—"}
                      sub="在售"
                      sensitiveKind="money"
                    />
                  </div>
                );
              })()
            : null}
        </section>

        {/* 工具栏：状态 tab + 搜索 + 排序 + 按钮 */}
        {groups.length ? (
          <ItemToolbar
            counts={counts}
            statusFilter={statusFilter}
            searchQuery={searchQuery}
            sortKey={sortKey}
            selectMode={selectMode}
            syncingSelection={syncingCurrentAccounts}
            selectedAccountCount={selectedAccountCount}
            accountSelector={(
              <ItemAccountMultiSelect
                accounts={accountOptions}
                selectedUids={selectedAccountUids}
                onChange={(uids) => {
                  setDesktopAccountFilterUids(uids);
                  exitSelectMode();
                }}
              />
            )}
            polishingUids={polishingUids}
            activeAccountsLen={activeAccounts.length}
            onStatusChange={(v) => {
              setStatusFilter(v);
              exitSelectMode();
            }}
            onSearchChange={setSearchQuery}
            onSortChange={setSortKey}
            onPublish={() => setPublishOpen(true)}
            onPolishAll={doPolishAll}
            onToggleSelect={() =>
              selectMode ? exitSelectMode() : enterSelectMode()
            }
            onSync={doSyncCurrentAccounts}
          />
        ) : null}

        {/* 批量删除进度条 */}
        {batchProgress ? (
          <div className="liquid-notice liquid-notice-warning flex items-center gap-2 mb-3 px-3 py-2 rounded-[10px]">
            <RefreshCw className="w-3.5 h-3.5 animate-spin" />
            <span className="text-[12.5px] font-medium">
              {batchProgress.message}
            </span>
            <span className="text-[11px] opacity-75">
              共 {batchProgress.total} 项
            </span>
          </div>
        ) : null}

        {/* 批量操作条 */}
        {selectMode && filtered.length > 0 ? (
          <BatchBar
            filtered={filtered}
            selectedKeys={selectedKeys}
            statusFilter={statusFilter}
            batchRunning={batchRunning}
            onToggleSelectAll={toggleSelectAll}
            onBatchDownshelf={doBatchDownshelf}
            onBatchReducePrice={doBatchReducePrice}
            onBatchGrouponOpen={doBatchGrouponOpen}
            onBatchGrouponClose={doBatchGrouponClose}
            onBatchCoinOpen={doBatchCoinOpen}
            onBatchCoinClose={doBatchCoinClose}
            onBatchUpshelf={doBatchUpshelf}
            onBatchDelete={doBatchDelete}
          />
        ) : null}

        {/* 内容区 */}
        {loading && !groups.length ? (
          <div
            className="flex items-center justify-center py-24 gap-2"
            style={{ color: "var(--text-secondary)" }}
          >
            <RefreshCw className="w-4 h-4 animate-spin" /> 正在加载商品…
          </div>
        ) : !groups.length ? (
          <EmptyState
            icon={Package}
            title="暂无商品"
            description="点击右上角同步商品从闲鱼拉取"
          >
            <button onClick={doSyncAll} className="btn-primary btn-sm mt-4">
              <RefreshCw className="w-3.5 h-3.5" /> 同步商品
            </button>
          </EmptyState>
        ) : (
          <>
            {statusFilter === "off" &&
            groups.some((g) => loadingSoldOut[g.uid]) ? (
              <div
                className="liquid-muted-card mb-3 rounded-[18px] px-3 py-2 text-center text-[12px]"
                style={{ color: "var(--text-secondary)" }}
              >
                <RefreshCw className="mr-1 inline h-3.5 w-3.5 animate-spin" />{" "}
                加载中
              </div>
            ) : null}

            {filtered.length === 0 ? (
              <EmptyState
                icon={Package}
                title="暂无匹配的商品"
                description="试试切换账号 / 状态 / 搜索条件"
              />
            ) : (
              <div className="items-grid">
                {filtered.map((it) => (
                  <ItemCard
                    key={`${it._uid}-${it.itemId}-${it._sold ? "sold" : "onsale"}`}
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
                ))}
              </div>
            )}
          </>
        )}
      </div>

      {editingItem ? (
        <EditItemModal
          uid={editingItem.uid}
          item={editingItem.item}
          mode={editingItem.mode || "edit"}
          onClose={() => setEditingItem(null)}
          onSaved={async () => {
            const uid = editingItem.uid;
            setEditingItem(null);
            await doSync(uid, { silent: true });
          }}
        />
      ) : null}

      {publishOpen ? (
        <PublishItemModal
          accounts={activeAccounts}
          onClose={() => setPublishOpen(false)}
          onBatchPublish={runBatchPublish}
          notify={notify}
        />
      ) : null}

      {dialog ? <DialogModal dialog={dialog} /> : null}
      {grouponSheet ? (
        <GrouponSheetModal
          sheet={grouponSheet}
          onClose={closeGrouponSheet}
          onSelect={(value) =>
            setGrouponSheet((prev) =>
              prev ? { ...prev, selectedPrice: value } : prev,
            )
          }
          onCustomPriceChange={(value) =>
            setGrouponSheet((prev) =>
              prev ? { ...prev, customPrice: value } : prev,
            )
          }
          onSubmit={submitGrouponSheet}
        />
      ) : null}
    </>
  );
}
