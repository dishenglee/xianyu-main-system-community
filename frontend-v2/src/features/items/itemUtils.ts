// @ts-nocheck
// 商品页面共享工具函数 — 桌面 / 移动两端复用
// 8 色账号调色板，对齐 orders/DesktopOrders.tsx — 不复用 messages/accountColor
// 那份（消息聚合明确要求账号统一灰色），这里需要每账号一个独立颜色。
export const ACCOUNT_COLORS = ['#FF5000', '#0EBE7C', '#A24DFF', '#0095FF', '#FFC900', '#FF2E63', '#5E5CE6', '#14B8A6']

export function colorForAccount(uid: string): string {
  if (!uid) return ACCOUNT_COLORS[0]
  let seed = 0
  for (const ch of String(uid)) seed = (seed * 31 + ch.charCodeAt(0)) % ACCOUNT_COLORS.length
  return ACCOUNT_COLORS[seed]
}

export function extractItems(r) {
  if (Array.isArray(r?.items)) return r.items
  if (Array.isArray(r?.data?.itemList)) return r.data.itemList
  if (Array.isArray(r?.data)) return r.data
  return []
}

// 拼接商品地域：省 + 市 + 区，空字段跳过。后端 item_row_to_value 会给
// city/province/area 三个独立字段，不合并，这里做展示层拼接。
export function composeLocation(it) {
  // 后端有时把 area 存成 "350121:闽侯县" 这种 divisionCode:areaName 形式，
  // 数字编码前端无需展示，剥掉前导 "数字:" 前缀。
  const stripCode = (v) => String(v ?? '').trim().replace(/^\d+\s*[:：]\s*/, '')
  const parts = [it.province, it.city, it.area].map(stripCode).filter(Boolean)
  if (parts.length > 0) {
    // 相邻重复去掉（有些账号 province === city 时后端会一起塞）
    const dedup = []
    for (const p of parts) if (dedup[dedup.length - 1] !== p) dedup.push(p)
    return dedup.join(' · ')
  }
  return it.divisionName || it.location || ''
}

export function composeAccountLocation(location) {
  if (!location || typeof location !== 'object') return ''
  const rawName = String(location.name ?? '').trim()
  const rawAddress = String(location.address ?? '').trim()
  const province = String(location.province ?? '').trim()
  const city = String(location.city ?? '').trim()
  const area = String(location.area ?? location.district ?? '').trim()
  if (rawName) {
    const parts = [province, city, rawName].filter(Boolean)
    const dedup = []
    for (const p of parts) if (dedup[dedup.length - 1] !== p) dedup.push(p)
    return dedup.join(' · ')
  }
  if (rawAddress) {
    const parts = [province, city, area, rawAddress].filter(Boolean)
    const dedup = []
    for (const p of parts) if (dedup[dedup.length - 1] !== p) dedup.push(p)
    return dedup.join(' · ')
  }
  return composeLocation({
    province,
    city,
    area,
    divisionName: location.divisionName,
    location: location.location,
  })
}

// 从 item 的嵌套 detail / itemContext 里兜底找发布时间字段。
export function deepFindTime(obj, depth = 0) {
  if (!obj || typeof obj !== 'object' || depth > 3) return null
  const TIME_KEYS = ['gmtCreate', 'gmtCreated', 'createTime', 'publishTime', 'postTime', 'onShelfTime', 'onSellTime', 'onlineTime']
  for (const k of TIME_KEYS) {
    if (obj[k]) return obj[k]
  }
  for (const v of Object.values(obj)) {
    if (v && typeof v === 'object') {
      const found = deepFindTime(v, depth + 1)
      if (found) return found
    }
  }
  return null
}

export const ITEM_SORT_OPTIONS = [
  { value: 'published_desc', label: '按发布时间', shortLabel: '最新' },
  { value: 'views', label: '按浏览量', shortLabel: '浏览' },
  { value: 'likes', label: '按收藏数', shortLabel: '收藏' },
  { value: 'price_desc', label: '价格高到低', shortLabel: '价高' },
  { value: 'price_asc', label: '价格低到高', shortLabel: '价低' },
]

export function itemPublishTimeMs(it) {
  const raw = deepFindTime(it)
  if (raw == null || raw === '' || raw === 0) return 0
  if (typeof raw === 'number') {
    return raw > 1_000_000_000_000 ? raw : raw * 1000
  }
  const text = String(raw).trim()
  if (!text) return 0
  const n = Number(text)
  if (!Number.isNaN(n) && n > 0) {
    return n > 1_000_000_000_000 ? n : n * 1000
  }
  const normalized = /^\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}$/.test(text)
    ? `${text.replace(/\s+/, 'T')}+08:00`
    : text
  const ms = new Date(normalized).getTime()
  return Number.isFinite(ms) ? ms : 0
}

// 商品发布时间格式化 — 兼容秒/毫秒/ISO。超过一年显示年份，同年显示月-日。
export function formatItemPublishedAt(raw) {
  if (raw == null || raw === '' || raw === 0) return ''
  const ms = itemPublishTimeMs({ publishTime: raw })
  if (!ms || Number.isNaN(ms)) return ''
  const d = new Date(ms)
  const now = new Date()
  const sameYear = d.getFullYear() === now.getFullYear()
  const mo = String(d.getMonth() + 1).padStart(2, '0')
  const da = String(d.getDate()).padStart(2, '0')
  return sameYear ? `${mo}-${da}` : `${d.getFullYear()}-${mo}-${da}`
}

export function normalizeCoinRatio(raw) {
  if (raw == null) return ''
  const text = String(raw).trim()
  if (!text) return ''
  const percent = text.match(/(\d+(?:\.\d+)?)\s*%/)
  const direct = text.match(/^\d+(?:\.\d+)?$/)
  const loose = /币|抵扣/.test(text) ? text.match(/(\d+(?:\.\d+)?)/) : null
  const value = percent?.[1] || direct?.[0] || loose?.[1] || ''
  if (!value) return ''
  const n = Number(value)
  if (!n || Number.isNaN(n) || n <= 0 || n > 100) return ''
  return Number.isInteger(n) ? String(n) : String(n).replace(/0+$/, '').replace(/\.$/, '')
}

export function coinDisplayRatio(it) {
  const direct = normalizeCoinRatio(it?.coinRatio)
    || normalizeCoinRatio(it?.coinDeductionRatio)
    || normalizeCoinRatio(it?.detail?.coinRatio)
    || normalizeCoinRatio(it?.coinSuffix)
    || normalizeCoinRatio(it?.detail?.coinSuffix)
  if (direct) return direct
  const ratioList = [
    ...(Array.isArray(it?.coinRatioList) ? it.coinRatioList : []),
    ...(Array.isArray(it?.detail?.coinRatioList) ? it.detail.coinRatioList : []),
  ]
  const opened = ratioList.find((entry) => entry?.open)
  return normalizeCoinRatio(opened?.ratio)
}

export function coinOpenedAtValue(it) {
  const raw = it?.coinOpenedAt ?? it?.detail?.coinOpenedAt ?? 0
  const n = Number(raw)
  return Number.isFinite(n) && n > 0 ? n : 0
}

export function coinCanClose(it, nowMs = Date.now()) {
  const openedAt = coinOpenedAtValue(it)
  if (!openedAt) return true
  return nowMs / 1000 - openedAt >= 24 * 3600
}

export function coinCloseRemainingText(it, nowMs = Date.now()) {
  const openedAt = coinOpenedAtValue(it)
  if (!openedAt) return ''
  const remainSec = Math.max(0, openedAt + 24 * 3600 - nowMs / 1000)
  if (remainSec <= 0) return ''
  const hours = Math.ceil(remainSec / 3600)
  if (hours >= 24) return `${hours} 小时后可关闭`
  const mins = Math.ceil(remainSec / 60)
  return mins > 60 ? `${hours} 小时后可关闭` : `${mins} 分钟后可关闭`
}

export function coinRatioOptionsFrom(...sources) {
  const byRatio = new Map()
  const push = (entry) => {
    if (entry == null) return
    const obj = typeof entry === 'object' ? entry : { ratio: entry }
    const ratio = normalizeCoinRatio(obj.ratio ?? obj.deductionRatio ?? obj.value)
    if (!ratio) return
    const canOpen = obj.canOpen !== false && obj.disabled !== true
    const descParts = []
    const description = String(obj.description || obj.title || obj.name || '').trim()
    const price = String(obj.price || obj.discountPrice || obj.deductionPrice || '').trim()
    const coin = String(obj.coin || obj.costCoin || obj.coinAmount || '').trim()
    if (description) descParts.push(description)
    if (price) descParts.push(`到手 ¥${price}`)
    if (coin) descParts.push(`消耗 ${coin} 币`)
    const next = {
      value: ratio,
      label: `${ratio}%`,
      description: descParts.join(' · ') || `抵扣 ${ratio}%`,
      disabled: !canOpen,
    }
    const prev = byRatio.get(ratio)
    if (!prev || (prev.disabled && !next.disabled) || (!prev.description && next.description)) {
      byRatio.set(ratio, next)
    }
  }
  const scan = (source) => {
    if (!source) return
    const data = source?.data && typeof source.data === 'object' ? source.data : source
    for (const list of [
      data?.coinDeductionInfoList,
      data?.coinRatioList,
      source?.coinRatioList,
      source?.detail?.coinRatioList,
    ]) {
      if (Array.isArray(list)) list.forEach(push)
    }
  }
  sources.forEach(scan)
  return [...byRatio.values()].sort((a, b) => Number(a.value) - Number(b.value))
}

export function normalizePriceText(raw) {
  const n = Number(String(raw ?? '').replace(/[^\d.]/g, ''))
  if (!Number.isFinite(n) || n <= 0) return ''
  return Number.isInteger(n) ? `${n}.00` : n.toFixed(2).replace(/0+$/, '').replace(/\.$/, '')
}

export function formatMoneyDisplay(raw, fallback = '--') {
  const text = normalizePriceText(raw)
  if (!text) return fallback
  const n = Number(text)
  return Number.isFinite(n) ? n.toFixed(2) : fallback
}

export function priceToNumber(raw) {
  const text = normalizePriceText(raw)
  if (!text) return null
  const n = Number(text)
  return Number.isFinite(n) ? n : null
}

export function resolveGrouponItemPrice(...candidates) {
  let best = null
  for (const candidate of candidates) {
    const n = priceToNumber(candidate)
    if (n == null || n <= 0) continue
    if (best == null || n < best) best = n
  }
  if (best != null) return normalizePriceText(best)
  for (const candidate of candidates) {
    const text = normalizePriceText(candidate)
    if (text) return text
  }
  return ''
}

export function discountAmountFromFinalPrice(itemPriceRaw, finalPriceRaw) {
  const itemPrice = priceToNumber(itemPriceRaw)
  const finalPrice = priceToNumber(finalPriceRaw)
  if (itemPrice == null || finalPrice == null) return ''
  const discount = itemPrice - finalPrice
  if (!Number.isFinite(discount) || discount <= 0) return ''
  return normalizePriceText(discount)
}

export const GROUPON_FIXED_AMOUNTS = ['6.66', '15.00', '50.00']
export const GROUPON_DISCOUNT_OPTIONS = ['80', '85', '90', '95']

export function normalizeGrouponSubmitPrice(raw) {
  const text = normalizePriceText(raw)
  return text ? formatMoneyDisplay(text, '') : ''
}

export function grouponDisplayPrice(it) {
  return normalizeGrouponSubmitPrice(it?.grouponPrice)
}

export function grouponOptionsFrom(...sources) {
  return GROUPON_FIXED_AMOUNTS.map((price) => ({
    mode: 'amount',
    value: normalizePriceText(price),
    label: `¥${formatMoneyDisplay(price)}`,
    description: `固定小刀金额 ¥${formatMoneyDisplay(price)}`,
    disabled: false,
  }))
}

export function grouponDiscountOptionsFrom(itemPriceRaw, ...sources) {
  const itemPrice = priceToNumber(itemPriceRaw)
  const buildDefault = () => GROUPON_DISCOUNT_OPTIONS.map((rate) => {
    const ratio = Number(rate) / 100
    const finalPrice = itemPrice != null ? normalizePriceText((itemPrice * ratio).toFixed(2)) : ''
    return {
      mode: 'discount',
      value: rate,
      label: rate === '80' ? '8折' : rate === '90' ? '9折' : `${rate}折`,
      finalPrice,
      description: finalPrice ? `折后价 ¥${formatMoneyDisplay(finalPrice)}` : '',
      disabled: itemPrice == null,
    }
  })
  for (const source of sources) {
    const data = source?.data && typeof source.data === 'object' ? source.data : source
    const list = Array.isArray(data?.drawer?.options) ? data.drawer.options : Array.isArray(data?.options) ? data.options : null
    if (!list?.length) continue
    const mapped = list.map((entry) => {
      const rate = String(entry?.discountRate ?? entry?.value ?? '').trim()
      const finalPrice = normalizePriceText(entry?.price || entry?.grouponPrice || entry?.finalPrice)
      const label = String(entry?.label || '').trim() || (rate === '80' ? '8折' : rate === '90' ? '9折' : `${rate}折`)
      return {
        mode: 'discount',
        value: rate,
        label,
        finalPrice,
        description: String(entry?.description || '').trim() || (finalPrice ? `折后价 ¥${formatMoneyDisplay(finalPrice)}` : ''),
        disabled: !!entry?.disabled || !rate,
      }
    }).filter((entry) => entry.value)
    if (mapped.length) return mapped
  }
  return buildDefault()
}

export function grouponGuideSummary(...sources) {
  for (const source of sources) {
    if (!source) continue
    const data = source?.data && typeof source.data === 'object' ? source.data : source
    const guide = data?.guide && typeof data.guide === 'object' ? data.guide : data
    const lines = []
    if (Array.isArray(guide?.descText)) {
      for (const text of guide.descText) {
        const normalized = String(text || '').trim()
        if (normalized) lines.push(normalized)
      }
    }
    if (!lines.length && Array.isArray(guide?.descRichText)) {
      for (const entry of guide.descRichText) {
        const normalized = String(entry?.text || '').trim()
        if (normalized) lines.push(normalized)
      }
    }
    if (!lines.length) {
      const fallback = String(guide?.buttonText || data?.errorDescription || '').trim()
      if (fallback) lines.push(fallback)
    }
    if (lines.length) return lines.join('\n')
  }
  return ''
}

export function grouponPriceValidation(priceRaw, itemPriceRaw) {
  const price = priceToNumber(priceRaw)
  if (price == null) return { ok: false, message: '请输入有效的小刀金额' }
  if (price <= 0) return { ok: false, message: '小刀金额必须大于 0' }
  const itemPrice = priceToNumber(itemPriceRaw)
  if (itemPrice != null && price >= itemPrice) {
    return {
      ok: false,
      message: `小刀金额不能大于或等于售价，当前售价 ¥${formatMoneyDisplay(itemPriceRaw)}`,
    }
  }
  return { ok: true, message: '' }
}

export function markdownAmountValidation(amountRaw, itemPriceRaw, grouponPriceRaw) {
  const amount = priceToNumber(amountRaw)
  if (amount == null) return { ok: false, message: '请输入有效的降价金额' }
  if (amount <= 0) return { ok: false, message: '降价金额必须大于 0' }
  const itemPrice = priceToNumber(itemPriceRaw)
  if (itemPrice != null && amount >= itemPrice) {
    return {
      ok: false,
      message: `降价金额不能大于或等于售价，当前售价 ¥${formatMoneyDisplay(itemPriceRaw)}`,
    }
  }
  const grouponPrice = priceToNumber(grouponPriceRaw)
  if (itemPrice != null && grouponPrice != null) {
    const nextPrice = itemPrice - amount
    if (!Number.isFinite(nextPrice) || nextPrice <= grouponPrice) {
      return {
        ok: false,
        message: `降价后价格不能低于或等于 2人小刀价格 ¥${formatMoneyDisplay(grouponPriceRaw)}`,
      }
    }
  }
  return { ok: true, message: '' }
}

export function normalizeActionType(action) {
  const raw = String(action?.actionType ?? '').trim()
  if (raw === '16') return '17'
  return raw
}

export function buildGrouponOptions(itemPrice, ...sources) {
  const drawerMode = sources
    .map((source) => source?.data?.drawer?.mode || source?.drawer?.mode || source?.data?.mode || source?.mode)
    .find(Boolean)
  if (drawerMode === 'discount') {
    return grouponDiscountOptionsFrom(itemPrice, ...sources)
  }
  return grouponOptionsFrom(...sources).map((option) => {
    const validation = grouponPriceValidation(option.value, itemPrice)
    return validation.ok
      ? option
      : {
          ...option,
          disabled: true,
          description: '高于售价，不可用',
        }
  })
}

export function normalizeGrouponDrawer(source, fallbackItem) {
  const data = source?.data && typeof source.data === 'object' ? source.data : source
  const drawer = data?.drawer && typeof data.drawer === 'object' ? data.drawer : {}
  const selectedPrice = normalizePriceText(drawer.selectedPrice || data?.grouponPrice || data?.guidePrice)
  const recommendedPrice = normalizePriceText(drawer.recommendedPrice || data?.guide?.grouponPrice || data?.grouponPrice)
  const itemPrice = normalizePriceText(drawer.itemPrice || data?.price || fallbackItem?.price)
  const selectedDiscount = normalizePriceText(drawer.selectedDiscount || discountAmountFromFinalPrice(itemPrice, selectedPrice))
  const recommendedDiscount = normalizePriceText(drawer.recommendedDiscount || discountAmountFromFinalPrice(itemPrice, recommendedPrice))
  const descLines = Array.isArray(drawer.descLines) ? drawer.descLines.map((line) => String(line || '').trim()).filter(Boolean) : []
  const headerTips = Array.isArray(drawer.headerTips) ? drawer.headerTips : []
  return {
    title: String(drawer.title || '设置小刀优惠'),
    mode: String(drawer.mode || data?.mode || 'amount'),
    itemPrice,
    selectedPrice,
    recommendedPrice,
    selectedDiscount,
    recommendedDiscount,
    helperText: String(drawer.helperText || '').trim(),
    closeOptionLabel: String(drawer.closeOptionLabel || '不开'),
    buttonText: String(drawer.buttonText || '确定'),
    backgroundImageUrl: String(drawer.backgroundImageUrl || '').trim(),
    descLines,
    headerTips,
    freeShipping: !!drawer.freeShipping,
    historyGroupon: !!drawer.historyGroupon,
  }
}
