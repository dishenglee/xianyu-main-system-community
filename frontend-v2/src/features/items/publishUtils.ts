// @ts-nocheck
// Pure publish-only helpers shared between PublishItemModal and itemActions.
import { isSuccess, retMsg } from '../../api/index'
import { filterBusinessAccounts } from '../../utils/accountAccess'

export function pickKgraphCategory(resp: any) {
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  if (!data || typeof data !== 'object') return null
  if (data.category && typeof data.category === 'object') return data.category
  if (Array.isArray(data.categories) && data.categories.length) return data.categories[0]
  const cards = Array.isArray(data.cardList) ? data.cardList : []
  for (const card of cards) {
    const propertyId = String(card?.propertyId ?? card?.cardData?.propertyId ?? '')
    if (propertyId !== '-10000') continue
    const values = card?.valuesList ?? card?.cardData?.valuesList
    if (Array.isArray(values) && values.length) return values[0]
  }
  return null
}

export function pickKgraphCardList(resp: any): any[] {
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  if (!data || typeof data !== 'object') return []
  const cards = Array.isArray(data.cardList) ? data.cardList : []
  return cards.map((card: any) => {
    const cd = card?.cardData ?? card
    return {
      propertyId: String(cd?.propertyId ?? ''),
      propertyName: String(cd?.propertyName ?? ''),
      valuesList: Array.isArray(cd?.valuesList) ? cd.valuesList : [],
    }
  }).filter((c: any) => c.propertyId && c.valuesList.length > 0)
}

export function categoryLabel(cat: any) {
  return String(cat?.catName || cat?.channelCatName || cat?.valueName || cat?.text || cat?.name || '').trim()
}

const SIMPLE_ITEM_SERVICE_KEYWORDS = [
  '服务',
  '技能',
  '软件开发',
  '软件资源',
  '定制设计',
  '文案设计',
  '代写',
  '降重',
  '润色',
  '翻译',
  '咨询',
  '辅导',
  '系统安装升级',
]

const SIMPLE_ITEM_CONTENT_KEYWORDS = [
  '软件安装包',
  '安装包',
  '序列号',
  '激活码',
  '卡密',
  '兑换码',
  '注册码',
  '授权码',
  '激活服务',
  '软件授权',
  '会员代充',
  '网盘',
  '教程资料',
  '远程安装',
  '代安装',
  '代装',
  '代下',
  '素材包',
]

function normalizeCategoryText(cat: any) {
  if (!cat) return ''
  if (typeof cat === 'string') return cat.trim()
  return [
    cat?.catName,
    cat?.channelCatName,
    cat?.valueName,
    cat?.text,
    cat?.name,
    cat?.level1Name,
    cat?.level2Name,
    cat?.level3Name,
    cat?.level4Name,
  ]
    .map((value) => String(value || '').trim())
    .filter(Boolean)
    .join(' ')
}

export function inferSimpleItemFromCategory(cat: any) {
  const normalized = normalizeCategoryText(cat)
  if (!normalized) return false
  return SIMPLE_ITEM_SERVICE_KEYWORDS.some((keyword) => normalized.includes(keyword))
}

export function inferSimpleItemFromContent(input: any, fallback = '') {
  const normalized = [
    String(input || '').trim(),
    String(fallback || '').trim(),
  ]
    .filter(Boolean)
    .join(' ')
  if (!normalized) return false
  return SIMPLE_ITEM_CONTENT_KEYWORDS.some((keyword) => normalized.includes(keyword))
}

function normalizeSimpleItemContext(context: any) {
  if (!context) return { title: '', desc: '' }
  if (typeof context === 'string') return { title: context.trim(), desc: '' }
  return {
    title: String(context?.title || context?.name || '').trim(),
    desc: String(context?.desc || context?.description || context?.content || '').trim(),
  }
}

export function resolveSimpleItemFlag(explicitValue: any, cat: any, context: any = null) {
  if (typeof explicitValue === 'boolean') return explicitValue
  if (explicitValue === 1 || explicitValue === '1' || String(explicitValue || '').trim().toLowerCase() === 'true') {
    return true
  }
  if (explicitValue === 0 || explicitValue === '0' || String(explicitValue || '').trim().toLowerCase() === 'false') {
    return false
  }
  if (inferSimpleItemFromCategory(cat)) return true
  const normalizedContext = normalizeSimpleItemContext(context)
  return inferSimpleItemFromContent(normalizedContext.title, normalizedContext.desc)
}

export function extractAiDesc(resp: any) {
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  return String(
    data?.desc
    || data?.description
    || data?.text
    || data?.content
    || '',
  ).trim()
}

export function extractAiTitle(resp: any) {
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  return String(
    data?.title
    || data?.itemTitle
    || data?.shortTitle
    || data?.subject
    || '',
  ).trim()
}

function isGenericDescTitleSegment(text: string) {
  const normalized = String(text || '').trim()
  if (!normalized) return true
  return [
    '图片实拍',
    '成色如图',
    '整体状态良好',
    '适合日常使用',
    '细节可沟通确认',
    '确认无误后再拍',
  ].some((item) => normalized === item || normalized.startsWith(item))
}

export function deriveTitleFromDescription(desc: string) {
  const firstLine = String(desc || '')
    .split(/\n+/)
    .map((line) => line.trim())
    .find(Boolean) || ''
  if (!firstLine) return ''

  const normalized = firstLine
    .replace(/^(图片版描述|AI描述|商品描述)\s*[:：]\s*/i, '')
    .trim()
  if (!normalized) return ''

  const sentence = normalized.split(/[。！？!?]/)[0]?.trim() || normalized
  const segments = sentence
    .split(/[，,；;]/)
    .map((segment) => segment.trim())
    .filter(Boolean)
  const preferred = segments.find((segment) => segment.length >= 4 && !isGenericDescTitleSegment(segment))
    || segments.find((segment) => !isGenericDescTitleSegment(segment))
    || segments[0]
    || sentence
  const title = preferred.slice(0, 60).trim()
  return isGenericDescTitleSegment(title) ? '' : title
}

function normalizeDescriptionHeadline(desc: string) {
  const firstLine = String(desc || '')
    .split(/\n+/)
    .map((line) => line.trim())
    .find(Boolean) || ''
  if (!firstLine) return ''
  return firstLine
    .replace(/^(图片版描述|AI描述|商品描述)\s*[:：]\s*/i, '')
    .trim()
}

export function derivePublishTitle(title: string, desc: string, fallbackCategory = '') {
  const explicit = String(title || '').trim()
  if (explicit) return explicit.slice(0, 60)

  const derived = deriveTitleFromDescription(desc)
  if (derived) return derived.slice(0, 60)

  const normalizedHeadline = normalizeDescriptionHeadline(desc)
  if (normalizedHeadline) {
    return normalizedHeadline
      .replace(/[。！？!?]/g, ' ')
      .replace(/\s+/g, ' ')
      .trim()
      .slice(0, 60)
  }

  const categoryTitle = String(fallbackCategory || '').trim()
  if (categoryTitle) return categoryTitle.slice(0, 60)
  return ''
}

export function extractBadwordsMatches(resp: any) {
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  const sources = [
    data?.matched,
    data?.badWords,
    data?.forbiddenWords,
    data?.hitWords,
    data?.words,
  ]
  const out = new Set<string>()
  for (const source of sources) {
    if (!Array.isArray(source)) continue
    for (const value of source) {
      const text = String(
        value?.word
        || value?.text
        || value?.value
        || value,
      ).trim()
      if (text) out.add(text)
    }
  }
  return Array.from(out)
}

export function isBadwordsSafe(resp: any) {
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  if (data?.safe === false || data?.pass === false) return false
  return extractBadwordsMatches(resp).length === 0
}

export function publishCheckMessage(resp: any, fallback: string) {
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  return String(
    data?.message
    || data?.detail
    || data?.tips
    || data?.text
    || retMsg(resp, fallback)
    || fallback,
  ).trim()
}

export function isPublishAllowed(resp: any) {
  if (!isSuccess(resp)) return false
  const data = resp?.data && typeof resp.data === 'object' ? resp.data : resp
  if (data?.canPublish === false || data?.enablePublish === false || data?.pass === false) return false
  return true
}

export function pickPreferredPublishAccount(accounts: any[]) {
  if (!Array.isArray(accounts) || !accounts.length) return null
  const usableAccounts = filterBusinessAccounts(accounts)
  if (!usableAccounts.length) return null
  const score = (account: any) => {
    const status = String(account?.last_session_status || '').trim()
    const reason = String(account?.last_session_reason || '').trim()
    const okAt = Number(account?.last_session_ok_at || 0)
    const checkedAt = Number(account?.last_session_check_at || 0)
    const hasH5Tk = account?.has_h5_tk ? 1 : 0
    const active = String(account?.status || '').trim() === 'active' ? 1 : 0
    return [
      status === 'ok' ? 1 : 0,
      reason === 'hasLogin_resume' ? 1 : 0,
      okAt,
      checkedAt,
      hasH5Tk,
      active,
    ]
  }
  return [...usableAccounts].sort((a, b) => {
    const sa = score(a)
    const sb = score(b)
    for (let i = 0; i < sa.length; i += 1) {
      if (sb[i] !== sa[i]) return sb[i] - sa[i]
    }
    return String(a?.uid || '').localeCompare(String(b?.uid || ''))
  })[0] || null
}
