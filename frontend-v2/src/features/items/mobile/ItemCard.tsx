// @ts-nocheck
import { ChevronRight, Coins, MapPin, Eye, Heart } from 'lucide-react'
import { colorForAccount, composeLocation, formatItemPublishedAt, coinDisplayRatio } from '../itemUtils'
import { SensitiveItemImage, SensitiveValue, useSensitiveText } from '../../../components/SensitiveValue'

interface ItemCardProps {
  item: any
  accountNick?: string
  selectMode?: boolean
  selected?: boolean
  onTap?: () => void
  onToggleSelect?: () => void
}

// Move to top-level so React render reuses one Intl instance.
const COMPACT_NUMBER = (() => {
  try {
    return new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 })
  } catch {
    return null
  }
})()

function formatCount(n: number | string | undefined): string {
  if (n == null || n === '') return ''
  const num = Number(n)
  if (!Number.isFinite(num) || num <= 0) return ''
  if (COMPACT_NUMBER) return COMPACT_NUMBER.format(num)
  if (num >= 10000) return `${(num / 10000).toFixed(1)}w`
  if (num >= 1000) return `${(num / 1000).toFixed(1)}k`
  return String(num)
}

export function ItemCard({ item, accountNick, selectMode, selected, onTap, onToggleSelect }: ItemCardProps) {
  const accentColor = colorForAccount(item._uid)
  const location = composeLocation(item)
  const published = formatItemPublishedAt(item.publishTime || item.gmtCreate || item.createTime)
  const coinRatio = coinDisplayRatio(item)
  const cover = item.picUrl || item.imageUrl || item.mainImg || (Array.isArray(item.images) ? item.images[0] : '')
  const views = formatCount(item.browseCount || item.pv || item.viewCount)
  const wants = formatCount(item.wantCount || item.collect || item.likeCount)
  const isSoldOut = item._listType === 'soldOut' || item.status === 'soldOut' || item.status === 'SOLD_OUT'
  const detail = (item?.detail && typeof item.detail === 'object') ? item.detail : {}
  const rawItemStatus = Number(detail.itemStatus ?? 0)
  const isReviewing = item.status === 'ON_SALE' && (rawItemStatus !== 0 || item.online === false) && !isSoldOut
  const accountLabel = useSensitiveText('account', accountNick)

  return (
    <div
      onClick={selectMode ? onToggleSelect : onTap}
      className="relative flex gap-3 px-4 py-3 active:bg-[var(--surface-soft)]/60 cursor-pointer"
      style={{ backgroundColor: 'var(--bg-card)' }}
    >
      {/* 账号色条（左侧 3px 竖线） */}
      <div
        className="absolute left-0 top-3 bottom-3 w-[3px] rounded-r-full"
        style={{ backgroundColor: accentColor }}
      />

      {/* 选择框 */}
      {selectMode && (
        <div className="shrink-0 flex items-start pt-1">
          <div
            className="w-5 h-5 rounded-full border-2 flex items-center justify-center transition-colors"
            style={{
              borderColor: selected ? 'var(--primary)' : 'var(--control-border)',
              backgroundColor: selected ? 'var(--primary)' : 'transparent',
            }}
          >
            {selected && <div className="w-2 h-2 rounded-full" style={{ backgroundColor: 'var(--liquid-highlight)' }} />}
          </div>
        </div>
      )}

      {/* 商品图 */}
      <div className="shrink-0 overflow-hidden rounded-[10px]">
        {cover ? (
          <SensitiveItemImage
            src={cover}
            alt=""
            className="w-[88px] h-[88px] object-cover"
            style={{ backgroundColor: 'var(--surface-soft)' }}
          />
        ) : (
          <div className="w-[88px] h-[88px] rounded-[10px] flex items-center justify-center text-[12px]" style={{ backgroundColor: 'var(--surface-soft)', color: 'var(--text-muted)' }}>
            无图
          </div>
        )}
      </div>

      {/* 主信息区 */}
      <div className="flex-1 min-w-0 flex flex-col">
        <p className="text-[14px] font-medium leading-snug line-clamp-2" style={{ color: 'var(--text-main)' }}>
          <SensitiveValue kind="item-title" value={item.title || item.itemTitle || '—'} />
        </p>

        <div className="flex items-baseline gap-1 mt-1">
          <span className="text-[16px] font-semibold" style={{ color: 'var(--primary)' }}>
            ¥<SensitiveValue kind="money" value={item.price || item.priceText || '—'} />
          </span>
          {coinRatio && (
            <span className="text-[10px] px-1.5 py-0.5 rounded font-medium" style={{ backgroundColor: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}>
              <Coins className="w-2.5 h-2.5 inline -mt-0.5" /> {coinRatio}%
            </span>
          )}
          {isReviewing && (
            <span className="text-[10px] px-1.5 py-0.5 rounded font-medium" style={{ backgroundColor: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}>
              审核中
            </span>
          )}
          {isSoldOut && !isReviewing && (
            <span className="text-[10px] px-1.5 py-0.5 rounded font-medium" style={{ backgroundColor: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' }}>
              已下架
            </span>
          )}
        </div>

        <div className="flex items-center gap-2 mt-auto pt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
          {accountNick && (
            <SensitiveValue kind="account" className="truncate max-w-[100px]" title={accountLabel} value={accountNick} />
          )}
          {location && (
            <>
              <span>·</span>
              <span className="flex items-center gap-0.5 truncate min-w-0">
                <MapPin className="w-3 h-3 shrink-0" />
                <span className="truncate">{location}</span>
              </span>
            </>
          )}
        </div>

        {(views || wants || published) && (
          <div className="flex items-center gap-3 mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
            {views && <span className="flex items-center gap-0.5"><Eye className="w-3 h-3" />{views}</span>}
            {wants && <span className="flex items-center gap-0.5"><Heart className="w-3 h-3" />{wants}</span>}
            {published && <span>{published}</span>}
          </div>
        )}
      </div>

      {/* 右侧 chevron */}
      {!selectMode && (
        <ChevronRight className="w-4 h-4 shrink-0 self-center" style={{ color: 'var(--text-muted)' }} />
      )}
    </div>
  )
}
