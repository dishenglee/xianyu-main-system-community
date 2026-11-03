// @ts-nocheck
import { useState } from 'react'
import {
  Package, Trash2, ArrowDownToLine, UploadCloud, ExternalLink,
  Edit3, TrendingDown, CheckCheck, Eye, Heart, BarChart3, Coins, Share2, MapPin,
} from 'lucide-react'
import {
  coinDisplayRatio, grouponDisplayPrice, composeLocation, deepFindTime,
  formatItemPublishedAt, normalizeActionType, coinCanClose, coinCloseRemainingText,
} from './itemUtils'
import { SensitiveItemImage, SensitiveValue, useSensitiveAvatarInitial, useSensitiveText } from '../../components/SensitiveValue'

// ---------- GlassBtn ----------
// 移除 backdrop-filter blur —— 它会让按钮背后的图片局部模糊+提亮（特别是
// grayscale 的下架图片，看起来一片白），用户反馈"鼠标移到商品就变白色"
// 主要是这个原因。改用半透明深色实底 + 轻微 hover 加深，视觉更稳定。
function GlassBtn({ children, onClick, title, style }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className="inline-flex items-center gap-1 px-2 py-[3px] rounded-md text-[11px] font-medium text-white cursor-pointer transition-colors"
      style={{
        background: 'rgba(0,0,0,0.55)',
        border: 'none',
        ...style,
      }}
    >
      {children}
    </button>
  )
}

// ---------- indexActions ----------
// list 接口下发的 actions[] 仅用于"哪个按钮可用"的可见性判断。
// 抓包确认 actionType 4=编辑 / 11=下架 / 17=2人小刀 / 22=闲鱼币抵扣；
// 7=分享对应 share_link endpoint。13=智能回复目前仍未实现。
function indexActions(actions) {
  const map = {}
  if (!Array.isArray(actions)) return map
  for (const a of actions) {
    const t = normalizeActionType(a)
    if (!t) continue
    map[t] = a
  }
  return map
}

// ---------- ItemCard ----------
export function ItemCard({ it, selectMode, selected, onToggleSelect, onDown, onUp, onDel, onReduce, onEdit, onShare, onToggleCoin, onToggleGroupon }) {
  const [hover, setHover] = useState(false)
  const actionMap = indexActions(it?.actions)

  // 下架商品的真实详情（后端从闲鱼 SOLD_OUT 接口 cardData 解析）：
  //   headTitle:      闲鱼标的下架原因（"小二下架"/"主动下架"）
  //   gmtOffShelf:    下架毫秒时间戳（**真实可信**，比字符串 outStockTime 准）
  //   gmtShelf:       上架毫秒时间戳
  //   recentSoldTime: 最近成交毫秒时间戳，有则真已售
  //   instockByXiaoer/deleteByXiaoer: 小二处理标志
  //   rePublishText:  闲鱼"上架"按钮文案（"重新上架"/"优化宝贝"）
  // 注：`outStockTime` 字段闲鱼下发的是错的（看到过 2026-07 的未来时间），不用。
  const detail = (it?.detail && typeof it.detail === 'object') ? it.detail : {}

  // 视觉标签：区分"在售"/"审核中"/"已售"/"下架"。
  // detail.itemStatus：上游 management.query 返回的真实商品状态码，非 0 且
  // 主 status 仍为 ON_SALE 时，代表审核中/受限等特殊状态。
  const rawItemStatus = Number(detail.itemStatus ?? 0)
  const isReviewing = it.status === 'ON_SALE' && (rawItemStatus !== 0 || it.online === false) && !it._sold
  const visualTag = it._sold ? 'sold' : isReviewing ? 'review' : it.online === false ? 'off' : 'onsale'
  const status = it._sold ? 'off' : isReviewing ? 'review' : (it.online === false) ? 'off' : 'onsale'
  const views = Number(it.browseCount ?? it.viewCount ?? 0)
  const likes = Number(it.wantCount ?? it.favorCount ?? 0)
  const coinRatio = coinDisplayRatio(it)
  const coinCloseHint = !coinCanClose(it) ? coinCloseRemainingText(it) : ''
  const grouponPrice = grouponDisplayPrice(it)
  const maskedGrouponPrice = useSensitiveText('money', grouponPrice)
  const maskedConfiguredGrouponPrice = useSensitiveText('money', it.grouponPrice)
  const maskedAccountNick = useSensitiveText('account', it._nick)
  const accountAvatarInitial = useSensitiveAvatarInitial(it._nick || '?')
  // 统一卡片尺寸：所有图片走 1:1 正方形比例，避免不同商品图片把卡片撑成
  // 不同高度（旧 masonry 行为）。原始宽高数据仍保留在 it.width/it.height，
  // 仅在 object-cover 内裁剪展示。
  const offReason: string = String(detail.offReason || detail.headTitle || '').trim()
  const headIconUrl: string = String(detail.headIconUrl || '').trim()
  // 把毫秒/秒时间戳格式化成本地日期时间字符串
  const fmtTs = (raw: unknown): string => {
    if (raw == null) return ''
    const n = Number(String(raw).trim())
    if (!n || Number.isNaN(n)) return ''
    const ms = n > 1e12 ? n : n * 1000
    return new Date(ms).toLocaleString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
  }
  const offShelfAt = fmtTs(detail.gmtOffShelf)
  const recentSoldTime = fmtTs(detail.recentSoldTime)
  const firstModifiedDiff: string = String(detail.firstModifiedDiff || '').trim()
  const rePublishText: string = String(detail.rePublishText || '').trim() || '上架'
  const isSold = !!detail.sold
  // 红色警示仅给"删除"类（不可恢复）。`instockByXiaoer=true` 不必然是违规，
  // 可能是系统/政策/活动结束 — 闲鱼自己的 headTitle 只说"小二下架"未定性违规，
  // 这里就不再用红色给用户造成"违规"误解。
  const isDangerous = !!detail.itemDeleted || !!detail.deleteByXiaoer
  return (
    <div
      className="rounded-[12px] overflow-hidden flex flex-col w-full h-full"
      style={{
        background: 'var(--surface-card)',
        border: selected ? '2px solid var(--primary)' : '1px solid var(--glass-border)',
        boxShadow: hover ? 'var(--glass-shadow-soft)' : 'none',
        transition: 'box-shadow 160ms ease, border 100ms ease',
        cursor: selectMode ? 'pointer' : undefined,
      }}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onClick={selectMode ? onToggleSelect : undefined}
    >
      <div className="relative overflow-hidden shrink-0" style={{ background: 'var(--surface-sunken)', aspectRatio: '1 / 1' }}>
        {selectMode ? (
          <button
            className="absolute left-2 bottom-2 z-10 w-6 h-6 rounded-md flex items-center justify-center transition-colors"
            style={{
              background: selected ? 'var(--primary)' : 'rgba(255,255,255,0.85)',
              border: selected ? 'none' : '1.5px solid var(--control-border)',
              boxShadow: '0 6px 14px rgba(25,35,55,0.12)',
            }}
            onClick={(e) => { e.stopPropagation(); onToggleSelect() }}
          >
            {selected ? <CheckCheck className="w-4 h-4 text-white" /> : null}
          </button>
        ) : null}
        {it.picUrl ? (
          <SensitiveItemImage
            src={it.picUrl}
            alt=""
            className="w-full h-full object-cover"
            style={{
              // 下架商品图片置灰显示；审核中不置灰只降低一点透明度提示状态。
              opacity: status === 'off' ? 0.6 : status === 'review' ? 0.85 : 1,
              filter: status === 'off' ? 'grayscale(0.85)' : 'none',
            }}
            loading="lazy"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center">
            <Package className="w-8 h-8" style={{ color: 'var(--text-muted)' }} />
          </div>
        )}

        {/* 状态角标：审核中 / 下架 / 已售 */}
        {visualTag === 'review' ? (
          <span
            className="absolute left-2 top-2 inline-flex items-center gap-1 text-[10px] font-semibold px-1.5 py-[2px] rounded"
            style={{ background: 'var(--warning)', color: 'var(--liquid-highlight)' }}
            title="商品审核中，暂不对买家展示"
          >
            审核中
          </span>
        ) : (visualTag === 'sold' || visualTag === 'off') ? (
          <span
            className="absolute left-2 top-2 inline-flex items-center gap-1 text-[10px] font-semibold px-1.5 py-[2px] rounded"
            style={{
              background: isDangerous ? 'var(--danger)' : (isSold ? 'var(--text-main)' : 'var(--status-neutral-text)'),
              color: 'var(--liquid-highlight)',
            }}
            title={offReason}
          >
            {headIconUrl ? (
              <img src={headIconUrl} alt="" className="w-3 h-3 rounded-sm object-cover" onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none' }} />
            ) : null}
            {offReason || (isSold ? '已售' : '下架')}
          </span>
        ) : null}

        {/* 账号头像（真实头像优先，fallback 彩色首字母） */}
        <div
          className="absolute right-2 top-2 w-7 h-7 rounded-full overflow-hidden flex items-center justify-center text-[10px] font-bold text-white"
          style={{
            background: it._avatar ? 'var(--surface-card)' : it._color,
            boxShadow: `0 0 0 2px ${it._color}, 0 0 0 4px var(--surface-card-strong)`,
          }}
          title={maskedAccountNick}
        >
          {it._avatar ? (
            <img src={it._avatar} alt="" className="w-full h-full object-cover" />
          ) : (
            accountAvatarInitial
          )}
        </div>

        {/* hover 毛玻璃操作栏 — 按后端 actions[] 动态渲染。
            actions 来自 list 接口下发的 server-driven UI 协议，每个 action 含
            actionType + actionUrl 预签名 token + enable 开关。enable=0 的不渲染。
            旧数据库行没有 actions（migration 前）时，回退为静态按钮（降价/编辑/下架/上架）。 */}
        {hover && !selectMode ? (
          <div
            className="absolute inset-x-0 bottom-0 p-2 flex items-center gap-1 flex-wrap"
            style={{ background: 'linear-gradient(to top, rgba(0,0,0,0.55), transparent)' }}
          >
            {(() => {
              const has = (t) => actionMap[String(t)] && String(actionMap[String(t)].enable ?? '1') !== '0'
              const fallback = !it?.actions || (Array.isArray(it.actions) && it.actions.length === 0)
              const buttons = []

              if (status === 'review') {
                // 审核中的商品：只能下架或删除
                if (has(11) || fallback) buttons.push(
                  <GlassBtn key="down" onClick={(e) => { e.stopPropagation(); onDown(it._uid, it) }} title="下架">
                    <ArrowDownToLine className="w-3 h-3" /> 下架
                  </GlassBtn>,
                )
              } else if (status === 'onsale') {
                if (has(3) || fallback) buttons.push(
                  <GlassBtn key="reduce" onClick={(e) => { e.stopPropagation(); onReduce(it._uid, it) }} title="降价">
                    <TrendingDown className="w-3 h-3" /> 降价
                  </GlassBtn>,
                )
                if (has(17) || fallback) buttons.push(
                  <GlassBtn
                    key="groupon"
                    onClick={(e) => { e.stopPropagation(); onToggleGroupon(it._uid, it) }}
                    title={it.grouponOpen ? (it.grouponPrice ? `关闭 2人小刀（当前 ¥${maskedConfiguredGrouponPrice}）` : '关闭 2人小刀') : '开启 2人小刀'}
                    style={it.grouponOpen ? { background: 'color-mix(in srgb, var(--primary) 70%, transparent)' } : undefined}
                  >
                    <TrendingDown className="w-3 h-3" /> {it.grouponOpen ? '小刀·开' : '2人小刀'}
                  </GlassBtn>,
                )
                if (has(22) || fallback) buttons.push(
                  <GlassBtn
                    key="coin"
                    onClick={(e) => { e.stopPropagation(); onToggleCoin(it._uid, it) }}
                    title={it.coinOpen
                      ? (coinRatio
                        ? `修改闲鱼币抵扣（当前 ${coinRatio}%${coinCloseHint ? `，${coinCloseHint}` : ''}）`
                        : `修改闲鱼币抵扣${coinCloseHint ? `（${coinCloseHint}）` : ''}`)
                      : '开启闲鱼币抵扣（按商品可用比例选择）'}
                    style={it.coinOpen ? { background: 'color-mix(in srgb, var(--warning) 65%, transparent)' } : undefined}
                  >
                    <Coins className="w-3 h-3" />
                    {it.coinOpen ? (coinRatio ? `币·${coinRatio}%` : '币') : '币'}
                  </GlassBtn>,
                )
                if (has(4) || fallback) buttons.push(
                  <GlassBtn key="edit" onClick={(e) => { e.stopPropagation(); onEdit(it) }} title="编辑">
                    <Edit3 className="w-3 h-3" /> 编辑
                  </GlassBtn>,
                )
                if (has(7) || fallback) buttons.push(
                  <GlassBtn key="share" onClick={(e) => { e.stopPropagation(); onShare(it._uid, it) }} title="分享链接">
                    <Share2 className="w-3 h-3" /> 分享
                  </GlassBtn>,
                )
                if (has(11) || fallback) buttons.push(
                  <GlassBtn key="down" onClick={(e) => { e.stopPropagation(); onDown(it._uid, it) }} title="下架">
                    <ArrowDownToLine className="w-3 h-3" /> 下架
                  </GlassBtn>,
                )
              } else if (status === 'off') {
                buttons.push(
                  <GlassBtn
                    key="up"
                    onClick={(e) => { e.stopPropagation(); onUp(it._uid, it) }}
                    style={{ background: 'var(--primary)' }}
                    title={rePublishText}
                  >
                    <UploadCloud className="w-3 h-3" /> {rePublishText}
                  </GlassBtn>,
                )
              }
              if (status !== 'sold') {
                buttons.push(
                  <GlassBtn
                    key="del"
                    onClick={(e) => { e.stopPropagation(); onDel(it._uid, it) }}
                    style={{ background: 'rgba(255,59,48,0.55)' }}
                    title="删除"
                  >
                    <Trash2 className="w-3 h-3" />
                  </GlassBtn>,
                )
              }
              return buttons
            })()}
            <a
              href={`https://www.goofish.com/item?id=${it.itemId}`}
              target="_blank"
              rel="noreferrer"
              onClick={(e) => e.stopPropagation()}
              className="ml-auto w-7 h-7 rounded-md flex items-center justify-center text-white"
              style={{ background: 'rgba(255,255,255,0.18)', backdropFilter: 'blur(8px)' }}
              title="闲鱼查看"
            >
              <ExternalLink className="w-3 h-3" />
            </a>
          </div>
        ) : null}
      </div>

      {/* 内容区固定为 flex-1，min-h-0 + overflow-hidden 保证内部条目即便很多
          也不会把卡片撑高 —— 网格里同一行的卡片必须等高。 */}
      <div className="p-2 flex-1 min-h-0 overflow-hidden flex flex-col gap-1">
        <p
          className="text-[12px] leading-snug line-clamp-2 shrink-0"
          style={{ color: 'var(--text-main)', minHeight: '2.6em' }}
        >
          <SensitiveValue kind="item-title" value={it.title || '无标题'} />
        </p>

        {/* 下架商品的完整下架信息行：原因 + 下架时间 + 成交/最近修改时间。
            在售商品不渲染。所有数据都来自后端从闲鱼 cardData 解析的真实字段。 */}
        {status === 'off' ? (
          <div
            className="px-2 py-1.5 rounded-[8px] text-[11px] leading-relaxed shrink-0 overflow-hidden"
            style={{
              background: isDangerous ? 'var(--status-danger-bg)' : 'var(--surface-soft)',
              border: `1px solid ${isDangerous ? 'color-mix(in srgb, var(--danger) 20%, transparent)' : 'var(--control-border)'}`,
              color: 'var(--text-main)',
            }}
          >
            <div className="flex items-center gap-1.5">
              {headIconUrl ? (
                <img src={headIconUrl} alt="" className="w-3.5 h-3.5 object-cover rounded-sm shrink-0" onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none' }} />
              ) : null}
              <span className="font-semibold" style={{ color: isDangerous ? 'var(--danger)' : 'var(--text-main)' }}>
                {offReason || '已下架'}
              </span>
              {isSold ? (
                <span className="ml-auto rounded px-1 py-[1px] text-[10px]" style={{ background: 'var(--text-main)', color: 'var(--liquid-highlight)' }}>已售出</span>
              ) : null}
            </div>
            <div className="mt-0.5 flex flex-wrap gap-x-3 gap-y-0.5" style={{ color: 'var(--text-secondary)' }}>
              {offShelfAt ? <span>下架：{offShelfAt}</span> : null}
              {recentSoldTime ? <span>成交：{recentSoldTime}</span> : null}
              {firstModifiedDiff && !offShelfAt ? <span>最近修改：{firstModifiedDiff}</span> : null}
            </div>
          </div>
        ) : null}

        {/* 价格行：当前价 + 划线原价（仅 origPrice 是有效数字且明显大于 price 时才展示，
            后端有时存 "0.00" 这种噪音值，过滤掉避免误展示。 */}
        {(() => {
          const cur = parseFloat(String(it.price ?? '0')) || 0
          const orig = parseFloat(String(it.originalPrice ?? it.origPrice ?? '0')) || 0
          const showOrig = orig > 0 && orig > cur
          return (
            <div className="flex items-end justify-between gap-2 shrink-0">
              <div className="flex items-baseline gap-1 min-w-0">
                <span className="text-[14px] font-semibold truncate" style={{ color: 'var(--danger)' }}>¥<SensitiveValue kind="money" value={it.price || '—'} /></span>
                {showOrig ? (
                  <span className="text-[10px] line-through truncate" style={{ color: 'var(--text-tertiary)' }}>¥<SensitiveValue kind="money" value={it.originalPrice} /></span>
                ) : null}
              </div>
            </div>
          )
        })()}

        {(it.grouponOpen || it.coinOpen) ? (
          <div className="flex items-center gap-1 flex-wrap shrink-0">
            {it.grouponOpen ? (
              <span
                className="inline-flex items-center gap-0.5 rounded px-1.5 py-[1px] text-[10px] font-medium shrink-0"
                style={{ background: 'var(--primary-light)', color: 'var(--primary)', border: '1px solid color-mix(in srgb, var(--primary) 22%, transparent)' }}
                title={grouponPrice ? `2人小刀 ¥${maskedGrouponPrice}` : '2人小刀已开启'}
              >
                <TrendingDown className="w-3 h-3" /> {grouponPrice ? <>小刀·¥<SensitiveValue kind="money" value={grouponPrice} /></> : '小刀·开'}
              </span>
            ) : null}
            {it.coinOpen ? (
              <span
                className="inline-flex items-center gap-0.5 rounded px-1.5 py-[1px] text-[10px] font-medium shrink-0"
                style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)', border: '1px solid color-mix(in srgb, var(--warning) 22%, transparent)' }}
                title={coinRatio ? `闲鱼币抵扣 ${coinRatio}%` : '闲鱼币抵扣'}
              >
                <Coins className="w-3 h-3" /> {coinRatio ? `币·${coinRatio}%` : '币'}
              </span>
            ) : null}
          </div>
        ) : null}

        {/* 数据指标合并一行：曝光 / 浏览 / 想要 — 任一 > 0 才渲染整行 */}
        {(() => {
          const exposure = Number(it.exposureNum ?? 0)
          if (exposure <= 0 && views <= 0 && likes <= 0) return null
          return (
            <div className="flex items-center gap-2 text-[10px] shrink-0" style={{ color: 'var(--text-muted)' }}>
              {exposure > 0 ? (
                <span className="inline-flex items-center gap-1"><BarChart3 className="w-3 h-3" /> {exposure}</span>
              ) : null}
              {views > 0 ? (
                <span className="inline-flex items-center gap-1"><Eye className="w-3 h-3" /> {views}</span>
              ) : null}
              {likes > 0 ? (
                <span className="inline-flex items-center gap-1"><Heart className="w-3 h-3" /> {likes}</span>
              ) : null}
            </div>
          )
        })()}

        {/* 位置：composeLocation 已剥掉行政区编码 */}
        {(() => {
          const location = composeLocation(it)
          if (!location) return null
          return (
            <div className="flex items-center gap-1 text-[10px] shrink-0 min-w-0" style={{ color: 'var(--text-muted)' }}>
              <MapPin className="w-3 h-3 shrink-0" />
              <span className="truncate">{location}</span>
            </div>
          )
        })()}

        {/* 账号归属 + 发布时间 —— 始终在卡片底部，mt-auto 推到尾部 */}
        <div className="flex items-center gap-1 mt-auto text-[10px] shrink-0 min-w-0" style={{ color: 'var(--text-muted)' }}>
          <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: it._color }} />
          <SensitiveValue kind="account" className="truncate" value={it._nick} />
          {(() => {
            const publishedRaw = deepFindTime(it)
            const published = formatItemPublishedAt(publishedRaw)
            return published ? (
              <span className="ml-auto truncate" title={`发布于 ${published}`}>{published}</span>
            ) : null
          })()}
        </div>
      </div>
    </div>
  )
}
