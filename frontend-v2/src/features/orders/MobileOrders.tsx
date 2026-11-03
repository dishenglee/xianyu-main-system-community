// @ts-nocheck
import { useState } from 'react'
import {
  Boxes,
  CheckCircle2,
  Clock3,
  RefreshCw,
  Receipt,
  ShoppingCart,
} from 'lucide-react'
import { EmptyState } from '../../components/EmptyState'
import { PullToRefresh } from '../../components/PullToRefresh'
import {
  getOverdueHours,
  getOrderPrice,
  formatMoney,
} from './orderUtils'
import { orderStageForOrder as kanbanStatusForOrder } from './orderStatus'
import { filterOrdersForList } from './OrderList'
import { useOrdersPageState } from './hooks/useOrdersPageState'
import { SensitiveValue, SensitiveVisibilityToggle } from '../../components/SensitiveValue'
import { MobileOrderCard } from './MobileOrderCard'
import { MobileOrdersToolbar, MobileAccountDrawer } from './MobileOrderFilters'
import { MobileShippingSheet } from './MobileShippingSheet'

export function MobileOrders() {
  const {
    activeAccounts,
    loading,
    groups,
    syncingUids,
    requestingRedFlowerIds,
    allOrders,
    accountStats,
    totalWaitSend,
    totalOverdue,
    shipModal,
    setShipModal,
    shipForm,
    setShipForm,
    shipMode,
    setShipMode,
    shipContextLoading,
    shipContextError,
    senderAddress,
    receiverAddress,
    copiedAddress,
    shipping,
    logisticsDisabled,
    loadAllOrders,
    syncOne,
    syncAll,
    handleDelete,
    handleClose,
    handleAdjustPrice,
    handleRequestRedFlower,
    handleShipmentSubmit,
    copyAddress,
  } = useOrdersPageState()

  const [acctFilter, setAcctFilter] = useState(null)
  const [statusFilter, setStatusFilter] = useState('all')
  const [searchQuery, setSearchQuery] = useState('')
  const [drawerOpen, setDrawerOpen] = useState(false)

  const boardOrders = acctFilter ? allOrders.filter((order) => order._uid === acctFilter) : allOrders
  const pendingCount = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'pending').length
  const tosendCount = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'tosend').length
  const sentCount = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'sent').length
  const aftersaleCount = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'aftersale').length
  const doneCount = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'done').length
  const closedCount = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'closed').length
  const overdueOnBoard = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'tosend' && getOverdueHours(order) > 0).length
  const kpiGmv = boardOrders.reduce((sum, order) => sum + getOrderPrice(order), 0)

  async function handleRefresh() {
    await loadAllOrders(false)
  }

  const visibleOrders = filterOrdersForList(boardOrders, statusFilter, searchQuery)
  const syncBusy = loading || Object.values(syncingUids || {}).some(Boolean)

  return (
    <div className="relative flex h-full flex-col mobile-page-shell overflow-hidden" style={{ backgroundColor: 'var(--bg-page)' }}>
      <div className="pointer-events-none absolute -top-12 right-12 h-40 w-40 rounded-full blur-[70px]" style={{ background: 'color-mix(in srgb, var(--primary) 18%, transparent)' }} />
      <div className="pointer-events-none absolute bottom-8 left-0 h-32 w-32 rounded-full blur-[60px]" style={{ background: 'color-mix(in srgb, var(--primary) 12%, transparent)' }} />

      {!activeAccounts.length ? (
        <div className="relative flex-1 mobile-page-body pt-3">
          <div
            className="mobile-surface-card flex h-full items-center justify-center px-6"
          >
            <EmptyState icon={ShoppingCart} title="暂无账号" description="请先在账号管理中添加激活账号" />
          </div>
        </div>
      ) : (
        <div className="relative min-h-0 flex-1 mobile-page-body pt-3">
          <div className="flex h-full min-h-0 flex-col mobile-page-stack">
            <section
              className="mobile-surface-card px-4 py-4"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <h2
                    className="text-[18px] font-semibold leading-none"
                    style={{ color: 'var(--text-main)' }}
                  >
                    概况
                  </h2>
                </div>

                <div className="flex shrink-0 items-center gap-2">
                  <SensitiveVisibilityToggle compact />
                  <button
                    type="button"
                    onClick={syncAll}
                    disabled={syncBusy}
                    className="shrink-0 inline-flex items-center gap-1.5 rounded-full border px-3 py-2 text-[12px] font-medium disabled:opacity-40"
                    style={{ background: 'var(--surface-card)', borderColor: 'var(--glass-border)', color: 'var(--text-main)' }}
                  >
                    <RefreshCw className={`h-3.5 w-3.5 ${syncBusy ? 'animate-spin' : ''}`} />
                    同步订单
                  </button>
                </div>
              </div>

              <div className="mt-4 grid grid-cols-2 gap-2">
                <MetricTile icon={Receipt} label="订单总数" value={boardOrders.length} />
                <MetricTile icon={Clock3} label="待发货" value={tosendCount} tone={tosendCount > 0 ? 'gold' : 'blue'} />
                <MetricTile icon={CheckCircle2} label="已完成" value={doneCount} tone="blue" />
                <MetricTile icon={Boxes} label="累计金额" value={<SensitiveValue kind="money" value={formatMoney(kpiGmv)} />} tone="blue" />
              </div>

              {overdueOnBoard > 0 ? (
                <div
                  className="mt-3 flex items-center justify-between gap-3 rounded-[22px] border px-3 py-3"
                  style={{ background: 'var(--status-danger-bg)', borderColor: 'color-mix(in srgb, var(--danger) 18%, transparent)' }}
                >
                  <div>
                    <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>待发货风险</div>
                    <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      当前有 {overdueOnBoard} 单超过 24 小时未发货。
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => setStatusFilter('tosend')}
                    className="shrink-0 rounded-full px-3 py-1.5 text-[12px] font-semibold"
                    style={{ background: 'var(--danger)', color: 'var(--liquid-highlight)' }}
                  >
                    立即查看
                  </button>
                </div>
              ) : null}
            </section>

            <section
              className="mobile-surface-card flex min-h-0 flex-1 flex-col overflow-hidden"
            >
              <div className="border-b px-4 py-3" style={{ borderColor: 'var(--glass-border)' }}>
                <h3 className="mt-1 text-[18px] font-semibold" style={{ color: 'var(--text-main)' }}>
                  订单列表
                </h3>
              </div>

              <MobileOrdersToolbar
                searchQuery={searchQuery}
                setSearchQuery={setSearchQuery}
                statusFilter={statusFilter}
                setStatusFilter={setStatusFilter}
                acctFilter={acctFilter}
                totalOrders={boardOrders.length}
                overdueCount={overdueOnBoard}
                pendingCount={pendingCount}
                tosendCount={tosendCount}
                sentCount={sentCount}
                aftersaleCount={aftersaleCount}
                doneCount={doneCount}
                closedCount={closedCount}
                onOpenAccountDrawer={() => setDrawerOpen(true)}
                accountStats={accountStats}
                surfaceBackground="var(--mobile-glass-surface-strong, var(--mobile-glass-surface))"
              />

              <div className="min-h-0 flex-1 overflow-hidden">
                <PullToRefresh onRefresh={handleRefresh}>
                  {loading && !allOrders.length ? (
                    <div className="flex items-center justify-center gap-2 py-24 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
                      <RefreshCw className="h-4 w-4 animate-spin" />
                      正在加载订单…
                    </div>
                  ) : !allOrders.length ? (
                    <div className="px-3 py-4">
                      <div className="mobile-soft-card">
                        <EmptyState icon={ShoppingCart} title="暂无订单" description="所有账号都没有订单数据" />
                      </div>
                    </div>
                  ) : !visibleOrders.length ? (
                    <div className="px-4 py-16 text-center text-[13px]" style={{ color: 'var(--text-secondary)' }}>
                      暂无匹配的订单
                    </div>
                  ) : (
                    <div className="px-2 pb-4 pt-2">
                      {visibleOrders.map((order, index) => (
                        <MobileOrderCard
                          key={`${order._uid}-${order.orderId || order.bizOrderId || index}`}
                          order={order}
                          onShip={(uid, currentOrder) => setShipModal({ uid, order: currentOrder })}
                          onAdjustPrice={handleAdjustPrice}
                          onClose={handleClose}
                          onDelete={handleDelete}
                          onRequestRedFlower={handleRequestRedFlower}
                          onSync={syncOne}
                          syncing={!!syncingUids[order._uid]}
                          requestingRedFlower={!!requestingRedFlowerIds?.[`${order._uid}:${order.bizOrderId || order.orderId}`]}
                        />
                      ))}
                    </div>
                  )}
                </PullToRefresh>
              </div>
            </section>
          </div>
        </div>
      )}

      <MobileAccountDrawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        acctFilter={acctFilter}
        setAcctFilter={setAcctFilter}
        accountStats={accountStats}
        totalOrders={allOrders.length}
        totalWaitSend={totalWaitSend}
        totalOverdue={totalOverdue}
      />

      <MobileShippingSheet
        shipModal={shipModal}
        onClose={() => setShipModal(null)}
        shipMode={shipMode}
        setShipMode={setShipMode}
        shipForm={shipForm}
        setShipForm={setShipForm}
        shipContextLoading={shipContextLoading}
        shipContextError={shipContextError}
        senderAddress={senderAddress}
        receiverAddress={receiverAddress}
        copiedAddress={copiedAddress}
        onCopyAddress={copyAddress}
        shipping={shipping}
        logisticsDisabled={logisticsDisabled}
        onSubmit={handleShipmentSubmit}
      />
    </div>
  )
}

function MetricTile({ icon: Icon, label, value, tone = 'blue' }) {
  const accent = 'var(--primary)'
  const accentSoft = tone === 'gold' ? 'var(--status-warning-bg)' : 'var(--primary-light)'

  return (
    <div
      className="mobile-soft-card px-3 py-3"
    >
      <div className="flex items-center gap-2">
        <span
          className="inline-flex h-9 w-9 items-center justify-center rounded-[14px]"
          style={{ background: accentSoft, color: accent }}
        >
          <Icon className="h-4.5 w-4.5" />
        </span>
        <div className="min-w-0">
          <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{label}</div>
          <div className="truncate text-[18px] font-semibold" style={{ color: 'var(--text-main)' }}>{value}</div>
        </div>
      </div>
    </div>
  )
}
