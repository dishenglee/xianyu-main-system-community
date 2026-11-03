import { formatCentMoney } from '../create/helpers'
import { pickNumberString } from '../../model'
import { SuperPolishBadge, SuperPolishSection } from '../../components/SuperPolishUi'
import type { SuperPolishCampaignDetailItem } from './types'

export function CampaignDetailCurrentItemsSection({
  items,
  loading,
  onDelete,
  onOpenEditor,
}: {
  items: SuperPolishCampaignDetailItem[]
  loading: boolean
  onDelete: (item: SuperPolishCampaignDetailItem) => void
  onOpenEditor: () => void
}) {
  return (
    <SuperPolishSection
      title={`当前宝贝 (${items.length}/10)`}
      subtitle="这里对应当前计划内正在维护的宝贝。删除会从本次计划草稿里移出，编辑宝贝会进入补充与调整弹层。"
      actions={(
        <button type="button" onClick={onOpenEditor} className="btn-secondary btn-sm">
          编辑宝贝
        </button>
      )}
    >
      {items.length === 0 ? (
        <div
          className="rounded-[18px] border px-4 py-6 text-[13px]"
          style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
        >
          {loading ? '正在加载当前宝贝…' : '当前计划还没有可维护的宝贝。'}
        </div>
      ) : (
        <div
          className="overflow-hidden rounded-[18px] border"
          style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
        >
          <div
            className="hidden grid-cols-[minmax(0,1.8fr)_120px_180px_140px_90px] gap-3 border-b px-4 py-3 text-[12px] font-medium md:grid"
            style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)', background: 'var(--surface-soft)' }}
          >
            <div>宝贝</div>
            <div>售价</div>
            <div>效果</div>
            <div>状态</div>
            <div className="text-right">操作</div>
          </div>
          {items.map((item) => (
            <div
              key={item.itemId}
              className="border-b px-4 py-4 last:border-b-0"
              style={{ borderColor: 'var(--control-border)' }}
            >
              <div className="grid gap-3 md:grid-cols-[minmax(0,1.8fr)_120px_180px_140px_90px] md:items-center">
                <div className="flex min-w-0 items-start gap-3">
                  {item.imageUrl ? (
                    <img src={item.imageUrl} alt={item.title} className="h-14 w-14 rounded-[12px] object-cover" />
                  ) : (
                    <div
                      className="flex h-14 w-14 items-center justify-center rounded-[12px] text-[11px]"
                      style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
                    >
                      宝贝
                    </div>
                  )}

                  <div className="min-w-0">
                    <div className="text-[13px] font-semibold leading-5" style={{ color: 'var(--text-main)' }}>
                      {item.title}
                    </div>
                    <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      商品 ID {item.itemId}
                    </div>
                    <div className="mt-2 flex flex-wrap items-center gap-2">
                      {item.supportAreaTarget ? <SuperPolishBadge tone="info">可城市定向</SuperPolishBadge> : null}
                      {item.notSupportReasons.map((reason) => (
                        <SuperPolishBadge key={`${item.itemId}-${reason}`} tone="warning">
                          {reason}
                        </SuperPolishBadge>
                      ))}
                    </div>
                  </div>
                </div>

                <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
                  {formatCentMoney(item.price)}
                </div>

                <div className="space-y-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  <div>点击：{pickNumberString(item.raw, [['adData', 'clickPv']], '0')}</div>
                  <div>咨询：{pickNumberString(item.raw, [['adData', 'chatPv']], '0')}</div>
                  <div>消耗：{formatCentMoney((item.raw.adData as { cost?: number } | undefined)?.cost ?? 0)}</div>
                </div>

                <div className="space-y-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  <div>{item.areaTarget ? '已开城市定向' : '全部地域'}</div>
                  <div>销量 {item.saleCount}</div>
                </div>

                <div className="flex justify-end">
                  <button type="button" onClick={() => onDelete(item)} className="btn-ghost btn-sm">
                    删除
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
