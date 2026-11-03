// @ts-nocheck
// Single-item mobile action sheet. Business handlers come from useItemsPageState;
// this component only presents them in the shared mobile visual language.
import {
  TrendingDown,
  Coins,
  ArrowDownToLine,
  UploadCloud,
  Edit3,
  Share2,
  Trash2,
  ExternalLink,
  Sparkles,
  ShieldCheck,
} from 'lucide-react'
import { BottomSheet } from '../../../components/Modal'
import { SensitiveValue, useSensitiveText } from '../../../components/SensitiveValue'
import { statusOf } from '../itemActions'
import { coinDisplayRatio, grouponDisplayPrice, coinCloseRemainingText } from '../itemUtils'

interface Props {
  open: boolean
  item: any | null
  onClose: () => void
  onReduce: (uid: string, item: any) => void
  onDown: (uid: string, item: any) => void
  onUp: (uid: string, item: any) => void
  onEdit: (item: any) => void
  onShare: (uid: string, item: any) => void
  onDelete: (uid: string, item: any) => void
  onToggleCoin: (uid: string, item: any) => void
  onToggleGroupon: (uid: string, item: any) => void
  onPublishCheck?: (uid: string, item: any) => void
  onPolish?: (uid: string) => void
}

function statusMeta(status: string) {
  if (status === 'onsale') return { label: '在售', bg: 'var(--primary-light)', color: 'var(--primary)' }
  if (status === 'review') return { label: '审核中', bg: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }
  return { label: '已下架', bg: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' }
}

export function MobileItemActionSheet({
  open,
  item,
  onClose,
  onReduce,
  onDown,
  onUp,
  onEdit,
  onShare,
  onDelete,
  onToggleCoin,
  onToggleGroupon,
  onPublishCheck,
  onPolish,
}: Props) {
  const sheetTitle = useSensitiveText('item-title', item?.title || '商品操作')
  const maskedGrouponPrice = useSensitiveText('money', item ? grouponDisplayPrice(item) : '')
  if (!item) return null

  const status = statusOf(item)
  const meta = statusMeta(status)
  const coinRatio = coinDisplayRatio(item)
  const coinCloseHint = item.coinOpen ? coinCloseRemainingText(item) : ''
  const grouponPrice = grouponDisplayPrice(item)

  const wrap = (handler: () => void) => () => {
    onClose()
    handler()
  }

  const cells: Array<{
    icon: any
    label: string
    color: string
    bg: string
    onClick: () => void
    sub?: string
  } | null> = []

  if (status === 'onsale') {
    cells.push(
      {
        icon: TrendingDown,
        label: '降价',
        color: 'var(--primary)',
        bg: 'var(--primary-light)',
        onClick: wrap(() => onReduce(item._uid, item)),
        sub: '直接调整价格',
      },
      {
        icon: TrendingDown,
        label: item.grouponOpen ? '小刀已开' : '2人小刀',
        color: 'var(--primary)',
        bg: 'var(--primary-light)',
        onClick: wrap(() => onToggleGroupon(item._uid, item)),
        sub: grouponPrice ? `当前 ¥${maskedGrouponPrice}` : '团购价格',
      },
      {
        icon: Coins,
        label: item.coinOpen ? '闲鱼币已开' : '闲鱼币',
        color: 'var(--status-warning-text)',
        bg: 'var(--status-warning-bg)',
        onClick: wrap(() => onToggleCoin(item._uid, item)),
        sub: coinRatio ? `${coinRatio}% 抵扣` : (coinCloseHint || '配置抵扣'),
      },
      {
        icon: Edit3,
        label: '编辑',
        color: 'var(--primary)',
        bg: 'var(--primary-light)',
        onClick: wrap(() => onEdit(item)),
        sub: '修改文案图片',
      },
      {
        icon: Share2,
        label: '分享',
        color: 'var(--primary)',
        bg: 'var(--primary-light)',
        onClick: wrap(() => onShare(item._uid, item)),
        sub: '复制对外链接',
      },
      onPublishCheck
        ? {
            icon: ShieldCheck,
            label: '政策检查',
            color: 'var(--status-success-text)',
            bg: 'var(--status-success-bg)',
            onClick: wrap(() => onPublishCheck(item._uid, item)),
            sub: '标题和发布资格',
          }
        : null,
      {
        icon: ArrowDownToLine,
        label: '下架',
        color: 'var(--status-neutral-text)',
        bg: 'var(--status-neutral-bg)',
        onClick: wrap(() => onDown(item._uid, item)),
        sub: '移出在售货架',
      },
      onPolish
        ? {
            icon: Sparkles,
            label: '擦亮账号',
            color: 'var(--status-warning-text)',
            bg: 'var(--status-warning-bg)',
            onClick: wrap(() => onPolish!(item._uid)),
            sub: '刷新账号曝光',
          }
        : null,
    )
  } else if (status === 'review') {
    cells.push({
      icon: ArrowDownToLine,
      label: '下架',
      color: 'var(--status-neutral-text)',
      bg: 'var(--status-neutral-bg)',
      onClick: wrap(() => onDown(item._uid, item)),
      sub: '结束审核中的展示',
    })
  } else if (status === 'off') {
    cells.push({
      icon: UploadCloud,
      label: '上架/重发',
      color: 'var(--status-success-text)',
      bg: 'var(--status-success-bg)',
      onClick: wrap(() => onUp(item._uid, item)),
      sub: '重新回到货架',
    })
  }

  cells.push({
    icon: Trash2,
    label: '删除',
    color: 'var(--status-danger-text)',
    bg: 'var(--status-danger-bg)',
    onClick: wrap(() => onDelete(item._uid, item)),
    sub: '不可恢复',
  })

  return (
    <BottomSheet open={open} onClose={onClose} title={sheetTitle}>
      <div className="mobile-surface-card overflow-hidden">
        <div
          className="p-4"
          style={{
            background:
              'radial-gradient(circle at top right, var(--primary-light), transparent 34%),' +
              'linear-gradient(180deg, rgba(255,255,255,0.96), var(--surface-card-strong))',
          }}
        >
          <div className="flex items-start gap-3">
            {item.picUrl ? (
              <img
                src={item.picUrl}
                alt=""
                className="w-16 h-16 rounded-[18px] object-cover shrink-0"
                style={{ border: '1px solid var(--control-border)' }}
              />
            ) : (
              <div
                className="w-16 h-16 rounded-[18px] shrink-0"
                style={{ background: 'var(--surface-sunken)' }}
              />
            )}

            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 flex-wrap">
                <span
                  className="mobile-state-badge"
                  style={{ background: meta.bg, color: meta.color }}
                >
                  {meta.label}
                </span>
                {item.grouponOpen ? (
                  <span className="mobile-pill mobile-muted-pill text-[10px]" style={{ color: 'var(--primary)' }}>
                    小刀 {grouponPrice ? <>¥<SensitiveValue kind="money" value={grouponPrice} /></> : '已开启'}
                  </span>
                ) : null}
                {item.coinOpen ? (
                  <span className="mobile-pill mobile-muted-pill text-[10px]" style={{ color: 'var(--status-warning-text)' }}>
                    币 {coinRatio ? `${coinRatio}%` : '已开启'}
                  </span>
                ) : null}
              </div>

              <p className="mt-2 text-[15px] font-medium leading-snug line-clamp-2" style={{ color: 'var(--text-main)' }}>
                <SensitiveValue kind="item-title" value={item.title || '—'} />
              </p>

              <div className="mt-3 flex items-end justify-between gap-3">
                <div>
                  <div className="text-[10px] font-semibold uppercase tracking-[0.14em]" style={{ color: 'var(--primary)' }}>
                    Price
                  </div>
                  <div className="mt-1 text-[22px] font-semibold leading-none" style={{ color: 'var(--danger)' }}>
                    ¥<SensitiveValue kind="money" value={item.price || '—'} />
                  </div>
                </div>

                <a
                  href={`https://www.goofish.com/item?id=${item.itemId}`}
                  target="_blank"
                  rel="noreferrer"
                  onClick={onClose}
                  className="inline-flex items-center gap-1.5 px-3 py-2 rounded-full text-[12px] font-medium cursor-pointer"
                  style={{ background: 'var(--surface-control)', color: 'var(--text-secondary)', border: '1px solid var(--control-border)' }}
                >
                  <ExternalLink className="w-3.5 h-3.5" />
                  闲鱼查看
                </a>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="mobile-soft-card mt-4 p-3">
        <div className="mobile-section-head">
          <div className="mobile-section-copy">
            <span className="mobile-section-kicker">Actions</span>
            <h3 className="mobile-section-title">单商品操作</h3>
            <p className="mobile-section-note">沿用现有动作逻辑，只调整移动端呈现方式。</p>
          </div>
        </div>

        <div className="mobile-grid-2 mt-4">
          {cells.filter(Boolean).map((cell: any) => (
            <button
              key={cell.label}
              type="button"
              onClick={cell.onClick}
              className="mobile-soft-card flex items-start gap-3 p-3 text-left cursor-pointer active:opacity-90 transition-opacity"
              style={{ background: 'rgba(255,255,255,0.84)' }}
            >
              <div
                className="w-11 h-11 rounded-[16px] shrink-0 flex items-center justify-center"
                style={{ background: cell.bg }}
              >
                <cell.icon className="w-5 h-5" style={{ color: cell.color }} />
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                  {cell.label}
                </div>
                <div className="mt-1 text-[11px] leading-snug" style={{ color: 'var(--text-muted)' }}>
                  {cell.sub}
                </div>
              </div>
            </button>
          ))}
        </div>
      </div>
    </BottomSheet>
  )
}
