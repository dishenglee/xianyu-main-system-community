import { CheckCircle2, Package, RefreshCw } from 'lucide-react'
import type { DeliveryItemSelectionByUid } from './deliveryMultiAccountCreate'
import type { DeliveryAccountProductState } from './deliveryMultiAccountProducts'

type AccountOption = { uid: string; nick?: string }
type ConfigurationLike = { uid: string; itemId: string }

type Props = {
  accounts: AccountOption[]
  statesByUid: Record<string, DeliveryAccountProductState>
  selectedItemIdsByUid: DeliveryItemSelectionByUid
  configurations: ConfigurationLike[]
  disabled?: boolean
  onToggleItem: (uid: string, itemId: string) => void
  onRefresh: (uid: string) => void
}

function stockText(quantity: number | null | undefined) {
  if (quantity === undefined || quantity === null) return '库存待同步'
  return `库存 ${quantity}`
}

export default function DeliveryProductAccountGroups({
  accounts,
  statesByUid,
  selectedItemIdsByUid,
  configurations,
  disabled = false,
  onToggleItem,
  onRefresh,
}: Props) {
  if (!accounts.length) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 py-10 text-center">
        <Package className="h-7 w-7" style={{ color: 'var(--text-tertiary)' }} />
        <div className="text-[13px] font-semibold">请先选择账号</div>
        <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>选择后会按账号分别加载在售商品和真实库存</div>
      </div>
    )
  }

  return (
    <div className="max-h-[440px] space-y-3 overflow-y-auto pr-1">
      {accounts.map((account) => {
        const state = statesByUid[account.uid] ?? { items: [], loading: true, error: '', stockSyncing: 0, stockFailures: 0 }
        const selectedIds = selectedItemIdsByUid[account.uid] ?? []
        const configuredIds = new Set(
          configurations.filter((configuration) => configuration.uid === account.uid).map((configuration) => configuration.itemId),
        )
        const label = account.nick || account.uid
        return (
          <section
            key={account.uid}
            aria-label={`${label}商品组`}
            className="overflow-hidden rounded-[14px] border"
            style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}
          >
            <div className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-2.5" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-card-strong)' }}>
              <div className="min-w-0">
                <div className="truncate text-[13px] font-semibold" title={label}>{label}</div>
                <div className="mt-0.5 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                  已选 {selectedIds.length} 个
                  {state.loading ? ' · 加载中' : ''}
                  {state.error ? ' · 加载失败' : ''}
                  {state.stockSyncing ? ` · 正在同步 ${state.stockSyncing} 个库存` : ''}
                  {state.stockFailures ? ` · ${state.stockFailures} 个库存同步失败` : ''}
                </div>
              </div>
              <button
                type="button"
                onClick={() => onRefresh(account.uid)}
                disabled={disabled || state.loading}
                className="btn-ghost btn-sm shrink-0 disabled:opacity-40"
                aria-label={`刷新 ${label} 商品和库存`}
              >
                <RefreshCw className={`h-3.5 w-3.5 ${state.loading ? 'animate-spin' : ''}`} />
                刷新
              </button>
            </div>

            {state.error ? (
              <div className="liquid-notice liquid-notice-danger m-3 rounded-xl px-3 py-2 text-[12px]">
                {state.error}
              </div>
            ) : null}
            {!state.error && state.stockFailures ? (
              <div className="liquid-notice liquid-notice-warning mx-3 mt-3 rounded-xl px-3 py-2 text-[12px]">
                有 {state.stockFailures} 个商品未同步到真实库存，暂不可选择，可单独刷新本账号重试。
              </div>
            ) : null}

            <div className="max-h-[260px] space-y-2 overflow-y-auto p-3">
              {state.items.map((item) => {
                const selected = selectedIds.includes(item.id)
                const alreadyConfigured = configuredIds.has(item.id)
                const stockUnavailable = item.quantity === null || item.quantity === undefined || Number(item.quantity) <= 0
                const itemDisabled = disabled || alreadyConfigured || stockUnavailable
                return (
                  <button
                    key={`${account.uid}:${item.id}`}
                    type="button"
                    disabled={itemDisabled}
                    aria-pressed={selected}
                    aria-label={`${label} / ${item.title}`}
                    onClick={() => onToggleItem(account.uid, item.id)}
                    className="w-full rounded-[12px] border p-3 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-55"
                    style={{
                      borderColor: selected ? 'var(--primary)' : 'var(--border-light)',
                      background: selected ? 'var(--primary-light)' : 'var(--surface-card-strong)',
                    }}
                  >
                    <div className="flex items-center gap-3">
                      <div className="h-11 w-11 shrink-0 overflow-hidden rounded-[10px]" style={{ background: 'var(--surface-sunken)' }}>
                        {item.picUrl
                          ? <img src={item.picUrl} alt="" className="h-full w-full object-cover" />
                          : <Package className="m-2.5 h-6 w-6" style={{ color: 'var(--text-tertiary)' }} />}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-[13px] font-semibold" title={item.title}>{item.title}</span>
                          {selected ? <CheckCircle2 className="h-4 w-4 shrink-0" style={{ color: 'var(--primary)' }} /> : null}
                        </div>
                        <div className="mt-1 flex flex-wrap gap-2 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                          <span>{item.price || '价格 —'}</span>
                          <span>{stockText(item.quantity)}</span>
                          <span>{alreadyConfigured ? '已配置' : stockUnavailable ? '暂不可选' : selected ? '已选择' : '可配置'}</span>
                        </div>
                      </div>
                    </div>
                  </button>
                )
              })}
              {state.loading ? (
                <div className="flex items-center justify-center gap-2 py-8 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  <RefreshCw className="h-4 w-4 animate-spin" /> 正在读取商品和库存…
                </div>
              ) : null}
              {!state.loading && !state.error && !state.items.length ? (
                <div className="py-8 text-center text-[12px]" style={{ color: 'var(--text-secondary)' }}>暂无在售商品</div>
              ) : null}
            </div>
          </section>
        )
      })}
    </div>
  )
}
