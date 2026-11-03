import { Plus, Trash2 } from 'lucide-react'
import { SafeRemoteImage } from '../../../messages/SafeRemoteImage'
import { SuperPolishBadge, SuperPolishSection } from '../../components/SuperPolishUi'
import {
  formatCentMoney,
  formatItemStats,
  MAX_CREATE_ITEMS,
  type SuperPolishSelectableItem,
} from './helpers'

export function CreatePlanSelectedItemsSection({
  items,
  onOpenPicker,
  onClear,
  onRemoveItem,
}: {
  items: SuperPolishSelectableItem[]
  onOpenPicker: () => void
  onClear: () => void
  onRemoveItem: (itemId: string) => void
}) {
  return (
    <SuperPolishSection
      title={`选择宝贝(${items.length}/${MAX_CREATE_ITEMS})`}
      subtitle="3个以上投放效果更优"
      actions={(
        <div className="flex flex-wrap items-center gap-2">
          <button type="button" onClick={onOpenPicker} className="btn-secondary btn-sm">
            <Plus className="h-3.5 w-3.5" />
            新增宝贝
          </button>
          <button type="button" onClick={onClear} className="btn-ghost btn-sm" disabled={items.length === 0}>
            <Trash2 className="h-3.5 w-3.5" />
            清空
          </button>
        </div>
      )}
    >
      {items.length === 0 ? (
        <div
          className="rounded-[18px] border px-4 py-5 text-[13px]"
          style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
        >
          当前还没有添加宝贝。
        </div>
      ) : (
        <div
          className="overflow-hidden rounded-[18px] border"
          style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
        >
          <div
            className="hidden grid-cols-[minmax(0,1.8fr)_120px_180px_120px_72px] gap-3 border-b px-4 py-3 text-[12px] font-medium md:grid"
            style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)', background: 'var(--surface-soft)' }}
          >
            <div>宝贝</div>
            <div>售价</div>
            <div>数据</div>
            <div>定向</div>
            <div className="text-right">操作</div>
          </div>
          {items.map((item) => (
            <div
              key={item.itemId}
              className="border-b px-4 py-4 last:border-b-0"
              style={{ borderColor: 'var(--control-border)' }}
            >
              <div className="grid gap-3 md:grid-cols-[minmax(0,1.8fr)_120px_180px_120px_72px] md:items-center">
                <div className="flex min-w-0 items-start gap-3">
                  <SafeRemoteImage
                    src={item.imageUrl}
                    alt={item.title}
                    className="h-14 w-14 rounded-[12px] object-cover"
                    fallback={(
                      <div
                        className="flex h-14 w-14 items-center justify-center rounded-[12px] text-[11px]"
                        style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
                      >
                        宝贝
                      </div>
                    )}
                  />
                  <div className="min-w-0">
                    <div className="text-[13px] font-semibold leading-5" style={{ color: 'var(--text-main)' }}>
                      {item.title}
                    </div>
                    <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      商品 ID {item.itemId}
                    </div>
                    <div className="mt-2 flex min-w-0 items-center gap-2 md:hidden">
                      <span className="shrink-0 text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                        {formatCentMoney(item.price)}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                        {formatItemStats(item)}
                      </span>
                      <span className="shrink-0">
                        {item.supportAreaTarget ? <SuperPolishBadge tone="info">城市定向</SuperPolishBadge> : <SuperPolishBadge tone="neutral">全部地域</SuperPolishBadge>}
                      </span>
                      <button
                        type="button"
                        onClick={() => onRemoveItem(item.itemId)}
                        className="btn-ghost btn-sm shrink-0 px-2"
                      >
                        取消添加
                      </button>
                    </div>
                  </div>
                </div>
                <div className="hidden text-[12px] md:block" style={{ color: 'var(--text-main)' }}>
                  {formatCentMoney(item.price)}
                </div>
                <div className="hidden text-[12px] md:block" style={{ color: 'var(--text-secondary)' }}>
                  {formatItemStats(item)}
                </div>
                <div className="hidden md:block">
                  {item.supportAreaTarget ? <SuperPolishBadge tone="info">支持城市定向</SuperPolishBadge> : <SuperPolishBadge tone="neutral">全部地域</SuperPolishBadge>}
                </div>
                <div className="hidden items-center justify-end md:flex">
                  <button
                    type="button"
                    onClick={() => onRemoveItem(item.itemId)}
                    className="btn-ghost btn-sm"
                  >
                    取消添加
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </SuperPolishSection>
  )
}
