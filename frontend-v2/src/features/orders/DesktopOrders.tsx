// @ts-nocheck
import { useState } from 'react'
import { RefreshCw, ShoppingCart } from 'lucide-react'
import { EmptyState } from '../../components/EmptyState'
import {
  KANBAN_META,
  getOverdueHours,
  getOrderPrice,
  formatMoney,
} from './orderUtils'
import { orderStageForOrder as kanbanStatusForOrder } from './orderStatus'
import { AccountMultiSelect, OrdKpi, OrdStatusTab } from './OrderFilters'
import { OrderList, filterOrdersForList } from './OrderList'
import { ShippingModal } from './ShippingModal'
import { useOrdersPageState } from './hooks/useOrdersPageState'
import { SensitiveValue, SensitiveVisibilityToggle } from '../../components/SensitiveValue'

export function DesktopOrders() {
  const state = useOrdersPageState()
  const {
    loading,
    groups,
    syncingUids,
    requestingRedFlowerIds,
    allOrders,
    accountStats,
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
    syncOne,
    syncAll,
    handleDelete,
    handleClose,
    handleAdjustPrice,
    handleRequestRedFlower,
    handleShipmentSubmit,
    copyAddress,
  } = state

  const [accountSelection, setAccountSelection] = useState({ accountKey: '', uids: [] })
  const [statusFilter, setStatusFilter] = useState('all')
  const [searchQuery, setSearchQuery] = useState('')

  const accountKey = accountStats.map((account) => account.uid).sort().join('|')
  const selectedAccountUids = accountSelection.accountKey === accountKey
    ? accountSelection.uids
    : []
  const selectedAccountUidSet = new Set(selectedAccountUids)
  const boardOrders = selectedAccountUids.length
    ? allOrders.filter((order) => selectedAccountUidSet.has(order._uid))
    : allOrders
  const pendingOrders = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'pending')
  const tosendOrders = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'tosend')
  const sentOrders = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'sent')
  const aftersaleOrders = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'aftersale')
  const doneOrders = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'done')
  const closedOrders = boardOrders.filter((order) => kanbanStatusForOrder(order) === 'closed')
  const kpiGmv = boardOrders.reduce((sum, order) => sum + getOrderPrice(order), 0)

  return (
    <>

      <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6">
        <section
          aria-labelledby="desktop-orders-heading"
          className="surface-card mb-4 rounded-[24px] px-5 py-4"
        >
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0">
              <h1 id="desktop-orders-heading" className="text-[20px] font-semibold tracking-[-0.02em]" style={{ color: 'var(--text-main)' }}>
                订单
              </h1>
            </div>
            <div className="flex items-center gap-2">
              <SensitiveVisibilityToggle compact />
              <button onClick={syncAll} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40" aria-label="同步全部订单">
                <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} /> 同步
              </button>
            </div>
          </div>

          {allOrders.length ? (
            <div className="mt-4 grid grid-cols-2 md:grid-cols-7 gap-2.5">
              <OrdKpi label="订单总数" value={boardOrders.length} />
              <OrdKpi label="待付款" value={pendingOrders.length} danger={pendingOrders.length > 0} />
              <OrdKpi label="待发货" value={tosendOrders.length} danger={tosendOrders.length > 0} />
              <OrdKpi label="售后" value={aftersaleOrders.length} danger={aftersaleOrders.length > 0} />
              <OrdKpi label="已完成" value={doneOrders.length} />
              <OrdKpi label="交易关闭" value={closedOrders.length} />
              <OrdKpi
                label="累计金额"
                value={<SensitiveValue kind="money" value={formatMoney(kpiGmv)} />}
              />
            </div>
          ) : null}
        </section>

        {loading && !groups.length ? (
          <div className="flex items-center justify-center py-24 gap-2" style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
            <RefreshCw className="w-4 h-4 animate-spin" />
            正在加载订单…
          </div>
        ) : !allOrders.length ? (
          <EmptyState icon={ShoppingCart} title="暂无订单" description="所有账号都没有订单数据" />
        ) : (
          <div className="space-y-4">
            <div className="flex items-center gap-2 flex-wrap">
              <div className="glass-control flex items-center gap-0.5 rounded-[12px] p-[3px]">
                <OrdStatusTab label="全部"    value="all"     count={boardOrders.length}     active={statusFilter === 'all'}     onChange={setStatusFilter} />
                <OrdStatusTab label="待付款"  value="pending" count={pendingOrders.length} active={statusFilter === 'pending'} onChange={setStatusFilter} tone={KANBAN_META.pending.tone} />
                <OrdStatusTab label="待发货"  value="tosend"  count={tosendOrders.length}  active={statusFilter === 'tosend'}  onChange={setStatusFilter} tone={KANBAN_META.tosend.tone} />
                <OrdStatusTab label="待收货"  value="sent"    count={sentOrders.length}    active={statusFilter === 'sent'}    onChange={setStatusFilter} tone={KANBAN_META.sent.tone} />
                <OrdStatusTab label="售后"    value="aftersale" count={aftersaleOrders.length} active={statusFilter === 'aftersale'} onChange={setStatusFilter} tone={KANBAN_META.aftersale.tone} />
                <OrdStatusTab label="已完成"  value="done"    count={doneOrders.length}    active={statusFilter === 'done'}    onChange={setStatusFilter} tone={KANBAN_META.done.tone} />
                <OrdStatusTab label="交易关闭" value="closed"  count={closedOrders.length}  active={statusFilter === 'closed'}  onChange={setStatusFilter} tone={KANBAN_META.closed.tone} />
              </div>

              <AccountMultiSelect
                accountStats={accountStats}
                selectedUids={selectedAccountUids}
                onChange={(uids) => setAccountSelection({ accountKey, uids })}
              />

              <div className="glass-control flex-1 min-w-[200px] max-w-[360px] flex items-center gap-1.5 rounded-[12px] px-3 py-[5px]">
                <ShoppingCart className="w-3.5 h-3.5" style={{ color: 'var(--text-muted)' }} />
                <input
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder="搜索商品标题 / 订单号 / 买家"
                  className="flex-1 bg-transparent outline-none text-[13px]"
                />
              </div>

              {tosendOrders.some((o) => getOverdueHours(o) > 0) ? (
                <span
                  className="inline-flex items-center gap-1 px-2.5 py-[5px] rounded-[8px] text-[12px] font-semibold cursor-pointer"
                  style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
                  onClick={() => setStatusFilter('tosend')}
                  title="跳到待发货"
                >
                  ⚠ {tosendOrders.filter((o) => getOverdueHours(o) > 0).length} 单超 24h
                </span>
              ) : null}
            </div>

            <OrderList
              orders={filterOrdersForList(boardOrders, statusFilter, searchQuery)}
              onShip={(uid, order) => setShipModal({ uid, order })}
              onAdjustPrice={handleAdjustPrice}
              onClose={handleClose}
              onDelete={handleDelete}
              onRequestRedFlower={handleRequestRedFlower}
              onSync={syncOne}
              syncingUids={syncingUids}
              requestingRedFlowerIds={requestingRedFlowerIds}
            />
          </div>
        )}
      </div>

      <ShippingModal
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
    </>
  )
}
