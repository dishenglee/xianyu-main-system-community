// @ts-nocheck
// Toolbar strip: status tabs + search + sort + action buttons + batch action bar.
// Pure presentational — all state and callbacks come from DesktopItems as props.
import type { ReactNode } from 'react'
import {
  RefreshCw, Sparkles, Plus, CheckSquare, Square, CheckCheck,
  Search as SearchIcon, TrendingDown, Coins, ArrowDownToLine, UploadCloud, Trash2,
} from 'lucide-react'
import { StatusTab } from './ItemKpi'
import { ITEM_SORT_OPTIONS } from './itemUtils'

interface ItemToolbarProps {
  counts: { onsale: number; off: number }
  statusFilter: string
  searchQuery: string
  sortKey: string
  selectMode: boolean
  syncingSelection: boolean
  selectedAccountCount: number
  accountSelector: ReactNode
  polishingUids: Record<string, boolean>
  activeAccountsLen: number
  onStatusChange: (v: string) => void
  onSearchChange: (v: string) => void
  onSortChange: (v: string) => void
  onPublish: () => void
  onPolishAll: () => void
  onToggleSelect: () => void
  onSync: () => void
}

export function ItemToolbar({
  counts, statusFilter, searchQuery, sortKey,
  selectMode, syncingSelection, selectedAccountCount, accountSelector, polishingUids,
  activeAccountsLen,
  onStatusChange, onSearchChange, onSortChange,
  onPublish, onPolishAll, onToggleSelect, onSync,
}: ItemToolbarProps) {
  const anyPolishing = Object.values(polishingUids).some(Boolean)
  return (
    <div className="flex items-center gap-2 flex-wrap mb-3">
      <div className="glass-control flex items-center gap-0.5 rounded-[12px] p-[3px]">
        <StatusTab value="onsale" label="在售"  count={counts.onsale} active={statusFilter === 'onsale'} onChange={onStatusChange} />
        <StatusTab value="off"    label="已下架" count={counts.off}   active={statusFilter === 'off'}    onChange={onStatusChange} />
      </div>

      {accountSelector}

      <div className="glass-control flex-1 min-w-[200px] max-w-[360px] flex items-center gap-1.5 rounded-[12px] px-3 py-[5px]">
        <SearchIcon className="w-3.5 h-3.5" style={{ color: 'var(--text-muted)' }} />
        <input
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder="搜索商品标题 / ID"
          className="flex-1 bg-transparent outline-none text-[13px]"
        />
      </div>

      <select
        value={sortKey}
        onChange={(e) => onSortChange(e.target.value)}
        className="px-2.5 py-[6px] rounded-[8px] text-[13px] border cursor-pointer"
        style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}
      >
        {ITEM_SORT_OPTIONS.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>

      <button
        onClick={onPublish}
        disabled={!activeAccountsLen}
        className="btn-primary btn-sm disabled:opacity-40"
        title="发布新商品"
      >
        <Plus className="w-3.5 h-3.5" /> 发布
      </button>
      <button
        onClick={onPolishAll}
        disabled={anyPolishing}
        className="btn-ghost btn-sm disabled:opacity-40"
        title={selectedAccountCount ? '对已选账号全部商品擦亮' : '一键对所有账号全部商品擦亮'}
      >
        <Sparkles className={`w-3.5 h-3.5 ${anyPolishing ? 'animate-spin' : ''}`} />
        {selectedAccountCount ? '擦亮' : '一键擦亮'}
      </button>
      <button
        onClick={onToggleSelect}
        className="btn-ghost btn-sm"
        style={selectMode ? { boxShadow: '0 0 0 1px var(--primary)' } : undefined}
        title={selectMode ? '退出选择' : '批量选择'}
      >
        <CheckSquare className="w-3.5 h-3.5" /> {selectMode ? '取消' : '选择'}
      </button>
      {selectedAccountCount ? (
        <button
          onClick={onSync}
          disabled={syncingSelection}
          className="btn-ghost btn-sm disabled:opacity-40"
          title="同步已选账号"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${syncingSelection ? 'animate-spin' : ''}`} /> 同步
        </button>
      ) : null}
    </div>
  )
}

interface BatchBarProps {
  filtered: any[]
  selectedKeys: Set<string>
  statusFilter: string
  batchRunning: boolean
  onToggleSelectAll: () => void
  onBatchDownshelf: () => void
  onBatchReducePrice: () => void
  onBatchGrouponOpen: () => void
  onBatchGrouponClose: () => void
  onBatchCoinOpen: () => void
  onBatchCoinClose: () => void
  onBatchUpshelf: () => void
  onBatchDelete: () => void
}

export function BatchBar({
  filtered, selectedKeys, statusFilter, batchRunning,
  onToggleSelectAll, onBatchDownshelf, onBatchReducePrice,
  onBatchGrouponOpen, onBatchGrouponClose,
  onBatchCoinOpen, onBatchCoinClose,
  onBatchUpshelf, onBatchDelete,
}: BatchBarProps) {
  const allSelected = selectedKeys.size === filtered.length
  return (
    <div
      className="flex items-center gap-2 mb-3 px-3 py-2 rounded-[10px]"
      style={{
        background: 'linear-gradient(135deg, var(--primary-light), rgba(255,255,255,0.62))',
        border: '1px solid color-mix(in srgb, var(--primary) 18%, transparent)',
        boxShadow: 'var(--glass-shadow-soft)',
      }}
    >
      <button onClick={onToggleSelectAll} className="flex items-center gap-1.5 text-[13px] font-medium cursor-pointer hover:opacity-70" style={{ color: 'var(--primary)' }}>
        {allSelected ? <CheckCheck className="w-4 h-4" /> : <Square className="w-4 h-4" />}
        {allSelected ? '取消全选' : '全选'}
      </button>
      <span className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>已选 {selectedKeys.size} / {filtered.length}</span>
      <div className="flex-1" />
      {selectedKeys.size > 0 ? (
        <>
          {statusFilter === 'onsale' ? (
            <>
              <button onClick={onBatchDownshelf} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40">
                <ArrowDownToLine className="w-3.5 h-3.5" /> 下架
              </button>
              <button onClick={onBatchReducePrice} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40">
                <TrendingDown className="w-3.5 h-3.5" /> 降价
              </button>
              <button onClick={onBatchGrouponOpen} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40">
                <TrendingDown className="w-3.5 h-3.5" /> 开启小刀
              </button>
              <button onClick={onBatchGrouponClose} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40">
                <TrendingDown className="w-3.5 h-3.5" /> 关闭小刀
              </button>
              <button onClick={onBatchCoinOpen} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40">
                <Coins className="w-3.5 h-3.5" /> 开启闲鱼币
              </button>
              <button onClick={onBatchCoinClose} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40">
                <Coins className="w-3.5 h-3.5" /> 关闭闲鱼币
              </button>
            </>
          ) : (
            <button onClick={onBatchUpshelf} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40">
              <UploadCloud className="w-3.5 h-3.5" /> 上架
            </button>
          )}
          <button onClick={onBatchDelete} disabled={batchRunning} className="btn-ghost btn-sm text-[12px] disabled:opacity-40" style={{ color: 'var(--danger)' }}>
            <Trash2 className="w-3.5 h-3.5" /> 删除
          </button>
          {batchRunning ? <RefreshCw className="w-3.5 h-3.5 animate-spin" style={{ color: 'var(--primary)' }} /> : null}
        </>
      ) : null}
    </div>
  )
}
