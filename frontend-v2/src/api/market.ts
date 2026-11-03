import type {
  AiConnectionTestPayload,
  AiConnectionTestResult,
  AiModelsDiscoverPayload,
  AiModelsDiscoverResult,
  AiSettings,
  Card,
  DeliveryConfiguration,
  DeliveryRule,
  DeliveryRunSummary,
  Item,
  ItemReply,
  ItemReplyMode,
  OfflineReplyResult,
  OfflineReplySyncPayload,
  Keyword,
  Pagination,
  RequestOptions,
  SaveDeliveryConfigurationPayload,
  UpdateDeliveryConfigurationPayload,
} from './types'
import { request, getBaseURL } from './client'
import { emitActionFailure, emitActionStarted, emitActionSuccess } from '../bootstrap/actionLogging'

function qs(params: Record<string, string | number | boolean | null | undefined>) {
  const query = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === '') continue
    query.set(key, String(value))
  }
  return query.toString()
}

function looksLikeImportUrlText(value: unknown): boolean {
  if (typeof value !== 'string') return false
  const text = value.trim()
  if (!text) return false
  return /^https?:\/\/[^\s]+$/i.test(text)
}

function normalizeImportedItemPayload(payload: any, rawText: string) {
  const data = payload?.data ?? payload
  const title = typeof data?.title === 'string' ? data.title.trim() : ''
  const desc = typeof data?.desc === 'string' ? data.desc.trim() : ''
  const raw = rawText.trim()
  const suspiciousTitle = looksLikeImportUrlText(title)
  const suspiciousDesc = looksLikeImportUrlText(desc)
  const echoesRawLink = !!raw && (title === raw || desc === raw)

  if ((suspiciousTitle || suspiciousDesc) && echoesRawLink) {
    throw new Error('导入商品详情失败，请稍后重试')
  }
  return payload
}

function unwrapApiField<T>(payload: unknown, field: string): T {
  if (payload && typeof payload === 'object' && !Array.isArray(payload)) {
    const record = payload as Record<string, unknown>
    if (
      Object.prototype.hasOwnProperty.call(record, field)
      && (
        Object.prototype.hasOwnProperty.call(record, 'status')
        || Object.keys(record).length === 1
      )
    ) {
      return record[field] as T
    }
  }

  return payload as T
}

// ==================== 搜索 ====================

export const search = (
  uid: string,
  keyword: string,
  {
    page = 1,
    sourceMode = 'auto',
    pageSize = 50,
    sortField = '',
    sortValue = '',
    propValueStr = '',
    latitude = '',
    longitude = '',
    distance = '',
    regionFilter = null as Record<string, unknown> | null,
  }: {
    page?: number
    sourceMode?: string
    pageSize?: number
    sortField?: string
    sortValue?: string
    propValueStr?: string
    // 附近搜索：经纬度（高德 POI 返的字符串）+ 半径（米）。空 = 不限位置。
    latitude?: string
    longitude?: string
    distance?: string
    // 搜索页「区域」面板产出的真实地域筛选对象。会直接传给后端构造
    // APP search payload，避免后端只能靠 distance 猜字段结构。
    regionFilter?: Record<string, unknown> | null
  } = {},
) =>
  request('POST', '/api/search', {
    uid, keyword, page,
    page_size: pageSize,
    source_mode: sourceMode,
    sort_field: sortField,
    sort_value: sortValue,
    prop_value_str: propValueStr,
    latitude, longitude, distance,
    region_filter: regionFilter,
  })

// ==================== 商品 ====================

export const itemDetail = (uid: string, itemId: string) =>
  request('POST', '/api/item/detail', { uid, item_id: itemId })

export const commentList = (uid: string, itemId: string) =>
  request('POST', '/api/item/comment/list', { uid, item_id: itemId })

export const commentCapture = (uid: string, itemId: string) =>
  request('POST', '/api/item/comment/capture', { uid, item_id: itemId })

export const commentPublish = (uid: string, itemId: string, content: string) =>
  request('POST', '/api/item/comment/publish', { uid, item_id: itemId, content })

// ==================== 商品管理 ====================

export const getItems = (
  uid: string,
  page = 1,
  pageSize = 20,
  tab = 'ON_SALE',
  { force = false, requestOptions = {} }: { force?: boolean; requestOptions?: RequestOptions } = {},
) =>
  request<{ items: Item[]; pagination: Pagination }>(
    'POST',
    '/api/items',
    { uid, page, page_size: pageSize, tab, force },
    requestOptions,
  )

export const itemManagementLive = (uid: string, page = 1, pageSize = 20, onlineSubTab = 1) =>
  request('POST', '/api/items/management_live', { uid, page, page_size: pageSize, online_sub_tab: onlineSubTab })

export const polishItem = (uid: string, itemId = '') => request(
  'POST',
  `/api/item/polish?uid=${encodeURIComponent(uid)}${itemId ? `&item_id=${encodeURIComponent(itemId)}` : ''}`,
)
export const reducePrice = (uid: string, itemId: string, amount: number) =>
  request('POST', '/api/item/reduce_price', { uid, item_id: itemId, amount })
export const itemReducePriceFeeds = (uid: string, itemId: string, suggestPrice = '', exts: unknown = undefined, priceKey = '') =>
  request('POST', '/api/item/reduce_price_feeds', {
    uid,
    item_id: itemId,
    suggest_price: suggestPrice,
    price_key: priceKey,
    ...(exts === undefined ? {} : { exts }),
  })
export const itemGrouponStatus = (uid: string, itemId: string) =>
  request('POST', '/api/item/groupon_status', { uid, item_id: itemId })
export const itemGrouponGuide = (uid: string, itemId: string) =>
  request('POST', '/api/item/groupon_guide', { uid, item_id: itemId })
export const itemGrouponOpen = (uid: string, itemId: string, price: string, extra: { mode?: string; discount_rate?: string } = {}) =>
  request('POST', '/api/item/groupon_open', { uid, item_id: itemId, price, ...extra })
export const itemGrouponClose = (uid: string, itemId: string) =>
  request('POST', '/api/item/groupon_close', { uid, item_id: itemId })
export const superExposure = (uid: string) => request('POST', `/api/item/super_exposure?uid=${uid}`)
export const batchReducePrice = (uid: string, itemIds: string[], amount: number) =>
  request('POST', '/api/items/batch_reduce', { uid, item_ids: itemIds, amount })

// ==================== 商品操作 ====================

export const itemDelete = (uid: string, itemId: string) =>
  request('POST', '/api/item/delete', { uid, item_id: itemId })

// Batch delete in ONE round-trip (backend fans out upstream calls with
// bounded concurrency, see `routes/market/items_batch.rs`). Two body shapes:
//   - single-account: { uid, item_ids: [...] }
//   - cross-account:  { deletions: [{uid, item_id}, ...] }
export interface BatchDeleteResultEntry { uid: string; itemId: string; ok: boolean; message: string }
export interface BatchDeleteResponse {
  ret?: string[]
  total: number
  success_count: number
  fail_count: number
  results: BatchDeleteResultEntry[]
}
export const itemsBatchDelete = (uid: string, itemIds: string[]) =>
  request<BatchDeleteResponse>('POST', '/api/items/batch_delete', { uid, item_ids: itemIds })
export const itemsBatchDeleteCrossAccount = (deletions: Array<{ uid: string; item_id: string }>) =>
  request<BatchDeleteResponse>('POST', '/api/items/batch_delete', { deletions })

export const itemDownshelf = (uid: string, itemId: string) =>
  request('POST', '/api/item/downshelf', { uid, item_id: itemId })
export const itemEditDetail = (uid: string, itemId: string) =>
  request('POST', '/api/item/edit_detail', { uid, item_id: itemId })

// 闲鱼币抵扣开关（Python 版对齐：mtop coin.deduction.new.open / .close）
export const itemCoinDeductionOpen = (uid: string, itemId: string, ratio: string) =>
  request('POST', '/api/coin/deduction/open', { uid, item_ids: [itemId], ratio })
export const itemCoinDeductionClose = (uid: string, itemId: string) =>
  request('POST', '/api/coin/deduction/close', { uid, item_ids: [itemId] })
// 分享链接（Python 版对齐：mtop.taobao.idle.item.share）
export const itemShare = (uid: string, itemId: string) =>
  request('POST', '/api/item/share_link', { uid, item_id: itemId })

// 商品类目属性 schema（动态，按商品标题/类目返不同 propertyName/valuesList）。
// 上游：mtop.taobao.idlemtopsearch.search → smartUiFilterPanelDO.filterPanelDOList。
// 后端已过滤通用维度，只返真实属性卡片。
export const itemProperties = (uid: string, itemId: string, title = '', channelCatId = '') =>
  request<{ data?: { cards?: Array<{ propertyId: string; propertyName: string; valuesList: Array<{ valueId: string; valueName: string; selected?: boolean }> }> } }>(
    'POST', '/api/item/properties',
    { uid, item_id: itemId, title, channel_cat_id: channelCatId },
  )

export const itemEdit = (
  uid: string,
  itemId: string,
  payload: Record<string, unknown> = {},
) =>
  request('POST', '/api/item/edit', {
    uid,
    item_id: itemId,
    title: '',
    short_title: '',
    desc: '',
    price: '',
    orig_price: '',
    no_bargain: null,
    can_free_shipping: null,
    image_urls: null,
    detail: {},
    ...payload,
  })

export const itemKgraph = (
  uid: string,
  {
    title = '', desc = '', catId = '', channelCatId = '',
    imageUrls = [] as string[],
    currentCardList = [] as unknown[],
  } = {},
) =>
  request('POST', '/api/item/kgraph', {
    uid, title, desc, cat_id: catId, channel_cat_id: channelCatId,
    image_urls: imageUrls,
    current_card_list: currentCardList,
  })

export const itemAiDescription = (
  uid: string,
  {
    title = '', desc = '', category = null as unknown, imageUrls = [] as string[],
    cardList = [] as unknown[], propSelections = {} as Record<string, string>,
  } = {},
) =>
  request('POST', '/api/item/ai_description', {
    uid, title, desc, category, image_urls: imageUrls,
    card_list: cardList, prop_selections: propSelections,
  })

export const publishCheck = (uid: string) =>
  request('GET', `/api/publish/check?uid=${encodeURIComponent(uid)}`)

export const publishBadwords = (uid: string, title: string) =>
  request('GET', `/api/publish/badwords?uid=${encodeURIComponent(uid)}&title=${encodeURIComponent(title)}`)

export const itemPublish = (
  uid: string,
  {
    title = '', shortTitle = '', desc = '', price = '', origPrice = '',
    imageUrls = [] as string[], simpleItem = false, noBargain = false,
    canFreeShipping = true,
    shippingType = 'free', postPrice = '', freightTemplateId = '',
    quantity = 1,
    cardList = [] as unknown[],
    propSelections = {} as Record<string, string>,
    divisionId = '', city = '', province = '',
    area = '', latitude = '', longitude = '', fishAdCode = '',
  } = {},
) =>
  request('POST', '/api/item/publish', {
    uid, title, short_title: shortTitle, desc, price, orig_price: origPrice,
    image_urls: imageUrls, simple_item: simpleItem, no_bargain: noBargain,
    can_free_shipping: canFreeShipping,
    shipping_type: shippingType, post_price: postPrice,
    freight_template_id: freightTemplateId, quantity,
    card_list: cardList,
    prop_selections: propSelections,
    division_id: divisionId,
    city, province, area, latitude, longitude, fish_ad_code: fishAdCode,
  })

export const itemOpen = (
  uid: string,
  itemId: string,
  inputJson = '',
  itemContext: Record<string, unknown> = {},
  extra: Record<string, unknown> = {},
) =>
  request('POST', '/api/item/open', {
    uid,
    item_id: itemId,
    input_json: inputJson,
    item_context: itemContext,
    ...extra,
  })

type RepublishContext = Record<string, unknown>

function republishText(value: unknown): string {
  return String(value ?? '').trim()
}

function republishBoolean(...values: unknown[]): boolean | undefined {
  for (const value of values) {
    if (value === true || value === false) return value
    if (value === 1 || value === '1') return true
    if (value === 0 || value === '0') return false
  }
  return undefined
}

function centsToPrice(value: unknown): string {
  const cents = Number(republishText(value))
  if (!Number.isFinite(cents) || cents <= 0) return ''
  return (cents / 100).toFixed(2).replace(/\.00$/, '').replace(/(\.\d)0$/, '$1')
}

/**
 * Keep re-list payload construction in one place. Both the edit dialog and
 * batch re-listing must use the complete edit-detail response rather than the
 * abbreviated sold-out list row.
 */
export function buildRepublishRequest(
  item: Record<string, any> = {},
  editDetail: any,
  overrides: Record<string, unknown> = {},
): { itemContext: RepublishContext; payload: Record<string, unknown> } {
  const detail = editDetail?.data ?? editDetail ?? {}
  const itemDO = detail?.itemDO ?? detail?.item ?? {}
  const text = detail?.itemTextDTO ?? {}
  const price = detail?.itemPriceDTO ?? {}
  const category = itemDO?.itemCatDTO ?? detail?.itemCatDTO ?? {}
  const listDetail = item?.detail && typeof item.detail === 'object' ? item.detail : {}
  const attributes = item?.attributesMap && typeof item.attributesMap === 'object' ? item.attributesMap : {}
  const imageSource = Array.isArray(itemDO?.imageInfos)
    ? itemDO.imageInfos
    : Array.isArray(detail?.imageInfoDOList)
      ? detail.imageInfoDOList
      : Array.isArray(itemDO?.imageUrls)
        ? itemDO.imageUrls
        : []
  const imageUrls = imageSource
    .map((image: any) => republishText(image?.url || image?.picUrl || image))
    .filter(Boolean)
  const pick = (...values: unknown[]) => values.map(republishText).find(Boolean) || ''
  const payload: Record<string, unknown> = {
    title: pick(itemDO?.title, text?.title, item?.title),
    desc: pick(itemDO?.desc, itemDO?.description, text?.desc, item?.desc, item?.content),
    price: pick(itemDO?.soldPrice, itemDO?.price, centsToPrice(price?.priceInCent), item?.price),
    orig_price: pick(itemDO?.originalPrice, itemDO?.origPrice, centsToPrice(price?.origPriceInCent), item?.origPrice, item?.originalPrice),
    image_urls: imageUrls,
    quantity: Number(itemDO?.quantity ?? detail?.quantity ?? item?.quantity ?? 1) || 1,
    detail,
  }
  const noBargain = republishBoolean(itemDO?.noBargain, detail?.noBargain, price?.noBargain, listDetail?.noBargain)
  const canFreeShipping = republishBoolean(itemDO?.canFreeShipping, detail?.canFreeShipping, itemDO?.freeShipping, listDetail?.freeShip)
  if (noBargain !== undefined) payload.no_bargain = noBargain
  if (canFreeShipping !== undefined) payload.can_free_shipping = canFreeShipping

  return {
    itemContext: {
      ...listDetail,
      tbCatId: pick(category?.tbCatId, category?.catId, listDetail?.tbCatId, item?.tbCatId, attributes?.tbCatId),
      channelCatId: pick(category?.channelCatId, listDetail?.channelCatId, item?.channelCatId, attributes?.cCatId),
      leafId: pick(category?.leafId, listDetail?.leafId, item?.leafId, attributes?.lfId),
      addressType: detail?.addressType ?? itemDO?.addressType ?? listDetail?.addressType ?? item?.addressType ?? attributes?.addressType,
      province: pick(itemDO?.province, detail?.province, item?.province, listDetail?.province),
      city: pick(itemDO?.city, detail?.city, item?.city, listDetail?.city),
      area: pick(itemDO?.area, detail?.area, item?.area, listDetail?.area),
      divisionId: pick(itemDO?.divisionId, detail?.divisionId, item?.divisionId, listDetail?.divisionId, attributes?.cainiaoDivision),
      gps: pick(itemDO?.gps, detail?.gps, item?.gps, listDetail?.gps),
      noBargain: noBargain ?? false,
      supportFreight: republishBoolean(itemDO?.supportFreight, detail?.supportFreight, listDetail?.supportFreight, attributes?.supportFreight) ?? false,
      freeShipping: canFreeShipping ?? republishBoolean(listDetail?.freeShip, item?.freeShip, attributes?.freeShipping) ?? false,
      simpleItem: republishBoolean(itemDO?.simpleItem, detail?.simpleItem, listDetail?.simpleItem, attributes?.simItem, item?.simpleItem) ?? false,
      publishScene: pick(detail?.publishScene, listDetail?.publishScene, attributes?.pScene, 'mainPublish'),
      sourceId: pick(detail?.sourceId, listDetail?.sourceId, attributes?.pSourceId, 'main_publish_item'),
    },
    payload: { ...payload, ...overrides },
  }
}

export function republishItemWithDetail(
  uid: string,
  itemId: string,
  item: Record<string, any>,
  editDetail: any,
  overrides: Record<string, unknown> = {},
) {
  const { itemContext, payload } = buildRepublishRequest(item, editDetail, overrides)
  return itemOpen(uid, itemId, '', itemContext, payload)
}

export async function republishItem(
  uid: string,
  itemId: string,
  item: Record<string, any> = {},
) {
  const detail = await itemEditDetail(uid, itemId)
  return republishItemWithDetail(uid, itemId, item, detail)
}

export const itemSellerRecommend = (uid: string, page = 1, pageSize = 20) =>
  request('GET', `/api/item/seller_recommend?uid=${uid}&page=${page}&page_size=${pageSize}`)

export const itemResellFeeds = (uid: string, page = 1, pageSize = 20) =>
  request('GET', `/api/item/resell_feeds?uid=${uid}&page=${page}&page_size=${pageSize}`)

export const itemAwesomeDetail = (uid: string, itemId: string) =>
  request('POST', '/api/item/awesome_detail', { uid, item_id: itemId })

export const itemAwesomeDetailUnit = (uid: string, itemId: string) =>
  request('POST', '/api/item/awesome_detail_unit', { uid, item_id: itemId })

export const publishCapabilities = (uid: string, itemId = '', title = '') =>
  request('GET', `/api/publish/capabilities?uid=${uid}&item_id=${encodeURIComponent(itemId)}&title=${encodeURIComponent(title)}`)

export const itemImportLink = async (uid: string, text: string) =>
  normalizeImportedItemPayload(
    await request('POST', '/api/item/import_link', { uid, text }),
    text,
  )

export const itemShareLink = (uid: string, itemId: string) =>
  request('POST', '/api/item/share_link', { uid, item_id: itemId })

export const uploadItemImage = async (uid: string, file: File) => {
  const fd = new FormData()
  fd.append('uid', uid)
  fd.append('file', file)
  emitActionStarted({
    module: 'market-items',
    action: 'upload_item_image',
    message: '开始上传商品图片',
    context: { uid, fileSize: file.size || 0, fileType: file.type || '' },
  })
  const baseURL = await getBaseURL()
  try {
    const r = await fetch(`${baseURL}/api/item/upload_image`, { method: 'POST', body: fd })
    const payload = await r.json().catch(() => ({}))
    if (!r.ok) {
      throw new Error(payload?.detail || payload?.message || `HTTP ${r.status}`)
    }
    emitActionSuccess({
      module: 'market-items',
      action: 'upload_item_image',
      message: '商品图片上传成功',
      context: { uid, fileSize: file.size || 0, hasUrl: Boolean(payload?.url) },
    })
    return payload
  } catch (error) {
    emitActionFailure({
      module: 'market-items',
      action: 'upload_item_image',
      message: '商品图片上传失败',
      error,
      context: { uid, fileSize: file.size || 0, fileType: file.type || '' },
    })
    throw error
  }
}

export const uploadImageUrl = (uid: string, url: string) =>
  request<{ success: boolean; url: string }>('POST', '/api/item/upload_image_url', { uid, url })

export const poiSearch = (uid: string, keyword: string, options: RequestOptions = {}) =>
  request('GET', `/api/location/poi_search?uid=${uid}&keyword=${encodeURIComponent(keyword)}`, undefined, options)

export const locationDivisions = (uid: string) =>
  request('GET', `/api/location/divisions?uid=${uid}`)

export const locationDivisionChildren = (uid: string, parentId: string | number) =>
  request('GET', `/api/location/division_children?uid=${uid}&parent_id=${encodeURIComponent(String(parentId))}`)

export const locationByGps = (uid: string, lat: number, lng: number) =>
  request('GET', `/api/location/by_gps?uid=${uid}&lat=${lat}&lng=${lng}`)

export const itemCollect = (uid: string, itemId: string) =>
  request('POST', '/api/item/collect', { uid, item_id: itemId })

export const commentDelete = (uid: string, itemId: string, commentId: string) =>
  request('POST', '/api/item/comment/delete', { uid, item_id: itemId, comment_id: commentId })

// ==================== M015 业务配置 ====================

export const getKeywords = (cookieId: string, options: RequestOptions = {}) =>
  request<Keyword[]>('GET', `/api/keywords/${cookieId}`, undefined, options)
export const createKeyword = (cookieId: string, payload: Partial<Keyword>, options: RequestOptions = {}) =>
  request<{ status?: string; keyword: Keyword } | Keyword>('POST', `/api/keywords/${cookieId}`, payload, options)
    .then((result) => unwrapApiField<Keyword>(result, 'keyword'))
export const updateKeyword = (keywordId: number, payload: Partial<Keyword>, options: RequestOptions = {}) =>
  request<{ status?: string; keyword: Keyword } | Keyword>('PUT', `/api/keywords/${keywordId}`, payload, options)
    .then((result) => unwrapApiField<Keyword>(result, 'keyword'))
export const deleteKeyword = (keywordId: number, options: RequestOptions = {}) =>
  request('DELETE', `/api/keywords/${keywordId}`, undefined, options)

export const getItemReplies = (cookieId: string, options: RequestOptions = {}) =>
  request<ItemReply[]>('GET', `/api/item-replies/${cookieId}`, undefined, options)
export const createItemReply = (cookieId: string, payload: Partial<ItemReply>, options: RequestOptions = {}) =>
  request<{ status?: string; itemReply: ItemReply } | ItemReply>('POST', `/api/item-replies/${cookieId}`, payload, options)
    .then((result) => unwrapApiField<ItemReply>(result, 'itemReply'))
export const updateItemReply = (itemReplyId: number, payload: Partial<ItemReply>, options: RequestOptions = {}) =>
  request<{ status?: string; itemReply: ItemReply } | ItemReply>('PUT', `/api/item-replies/${itemReplyId}`, payload, options)
    .then((result) => unwrapApiField<ItemReply>(result, 'itemReply'))
export const deleteItemReply = (itemReplyId: number, options: RequestOptions = {}) =>
  request('DELETE', `/api/item-replies/${itemReplyId}`, undefined, options)

export const getItemReplyModes = (cookieId: string, options: RequestOptions = {}) =>
  request<ItemReplyMode[]>('GET', `/api/item-reply-modes/${cookieId}`, undefined, options)
export const upsertItemReplyMode = (cookieId: string, payload: Partial<ItemReplyMode>, options: RequestOptions = {}) =>
  request<{ status?: string; itemReplyMode: ItemReplyMode } | ItemReplyMode>('PUT', `/api/item-reply-modes/${cookieId}`, payload, options)
    .then((result) => unwrapApiField<ItemReplyMode>(result, 'itemReplyMode'))
export const deleteItemReplyMode = (cookieId: string, itemId: string, options: RequestOptions = {}) =>
  request('DELETE', `/api/item-reply-modes/${cookieId}`, { itemId }, options)

export const getOfflineReply = (cookieId: string, itemId: string, options: RequestOptions = {}) =>
  request<OfflineReplyResult>(
    'GET',
    `/api/offline-reply/${cookieId}?itemId=${encodeURIComponent(itemId)}`,
    undefined,
    options,
  )
export const syncOfflineReply = (cookieId: string, payload: OfflineReplySyncPayload, options: RequestOptions = {}) =>
  request<OfflineReplyResult>('PUT', `/api/offline-reply/${cookieId}`, payload, options)

export const getCards = (cookieId: string, options: RequestOptions = {}) =>
  request<Card[]>('GET', `/api/cards/${cookieId}`, undefined, options)
export const bulkCreateCards = <T extends Partial<Card> = Partial<Card>>(cookieId: string, cards: T[], options: RequestOptions = {}) =>
  request<{ status?: string; cards: Card[] } | Card[]>('POST', '/api/cards/bulk', { cookieId, cards }, options)
    .then((result) => unwrapApiField<Card[]>(result, 'cards'))
export const deleteCard = (cardId: number, options: RequestOptions = {}) =>
  request('DELETE', `/api/cards/${cardId}`, undefined, options)
export const updateCard = (cardId: number, payload: Pick<Card, 'cardContent' | 'cardPoolTag'>, options: RequestOptions = {}) =>
  request<{ status?: string; card: Card } | Card>('PUT', `/api/cards/${cardId}`, payload, options)
    .then((result) => unwrapApiField<Card>(result, 'card'))

export const getDeliveryRules = (cookieId: string, options: RequestOptions = {}) =>
  request<DeliveryRule[]>('GET', `/api/delivery-rules/${cookieId}`, undefined, options)
export const createDeliveryRule = (cookieId: string, payload: Partial<DeliveryRule>, options: RequestOptions = {}) =>
  request<{ status?: string; deliveryRule: DeliveryRule } | DeliveryRule>('POST', `/api/delivery-rules/${cookieId}`, payload, options)
    .then((result) => unwrapApiField<DeliveryRule>(result, 'deliveryRule'))
export const updateDeliveryRule = (ruleId: number, payload: Partial<DeliveryRule>, options: RequestOptions = {}) =>
  request<{ status?: string; deliveryRule: DeliveryRule } | DeliveryRule>('PUT', `/api/delivery-rules/${ruleId}`, payload, options)
    .then((result) => unwrapApiField<DeliveryRule>(result, 'deliveryRule'))
export const deleteDeliveryRule = (ruleId: number, options: RequestOptions = {}) =>
  request('DELETE', `/api/delivery-rules/${ruleId}`, undefined, options)

export const listDeliveryConfigurations = (uid: string, options: RequestOptions = {}) =>
  request<DeliveryConfiguration[]>('GET', `/api/delivery/configurations?${qs({ uid })}`, undefined, options)
export const createDeliveryConfiguration = (payload: SaveDeliveryConfigurationPayload, options: RequestOptions = {}) =>
  request<{ status?: string; configuration: DeliveryConfiguration }>(
    'POST',
    '/api/delivery/configurations',
    payload,
    options,
  ).then((result) => result.configuration)
export const updateDeliveryConfiguration = (
  id: number,
  payload: UpdateDeliveryConfigurationPayload,
  options: RequestOptions = {},
) =>
  request<{ status?: string; configuration: DeliveryConfiguration }>(
    'PUT',
    `/api/delivery/configurations/${id}`,
    payload,
    options,
  ).then((result) => result.configuration)
export const deleteDeliveryConfiguration = (id: number, options: RequestOptions = {}) =>
  request('DELETE', `/api/delivery/configurations/${id}`, undefined, options)
export const addDeliveryConfigurationCards = (
  id: number,
  cards: string[],
  options: RequestOptions = {},
) =>
  request<{ status?: string; insertedCount: number; configuration: DeliveryConfiguration }>(
    'POST',
    `/api/delivery/configurations/${id}/cards`,
    { cards },
    options,
  )
export const listDeliveryConfigurationCards = (id: number, options: RequestOptions = {}) =>
  request<Card[]>('GET', `/api/delivery/configurations/${id}/cards`, undefined, options)
export const refreshDeliveryProductStock = (
  uid: string,
  itemId: string,
  options: RequestOptions = {},
) =>
  request<{ status?: string; itemId: string; quantity: number | null }>(
    'POST',
    `/api/delivery/products/${encodeURIComponent(itemId)}/refresh-stock?${qs({ uid })}`,
    undefined,
    options,
  )
export const listDeliveryRuns = (
  params: {
    uid?: string;
    configurationId?: number;
    status?: string;
    page?: number;
    pageSize?: number;
  } = {},
  options: RequestOptions = {},
) => request<DeliveryRunSummary[]>('GET', `/api/delivery/runs?${qs(params)}`, undefined, options)
export const retryDeliveryRun = (id: number, options: RequestOptions = {}) =>
  request<{ status?: string; run: DeliveryRunSummary }>('POST', `/api/delivery/runs/${id}/retry`, undefined, options)
    .then((result) => result.run)

export const getAiSettings = (cookieId: string, options: RequestOptions = {}) =>
  request<AiSettings>('GET', `/api/ai-settings/${cookieId}`, undefined, options)
export const updateAiSettings = (cookieId: string, payload: Partial<AiSettings>, options: RequestOptions = {}) =>
  request<{ status?: string; settings: AiSettings } | AiSettings>('PUT', `/api/ai-settings/${cookieId}`, payload, options)
    .then((result) => unwrapApiField<AiSettings>(result, 'settings'))
export const discoverAiModels = (
  payload: Partial<AiModelsDiscoverPayload>,
  options: RequestOptions = {},
) =>
  request<{ status?: string; models?: string[]; endpoint?: string } | AiModelsDiscoverResult>(
    'POST',
    '/api/ai-settings/models',
    payload,
    options,
  ).then((result) => ({
    models: Array.isArray((result as any)?.models) ? ((result as any).models as string[]) : [],
    endpoint: (result as any)?.endpoint,
  }))
export const testAiSettingsConnection = (
  payload: Partial<AiConnectionTestPayload>,
  options: RequestOptions = {},
) =>
  request<{ status?: string } & AiConnectionTestResult>(
    'POST',
    '/api/ai-settings/test',
    payload,
    options,
  )

// ==================== S04 item aliases ====================

export const itemList = (uid: string, page = 1, pageSize = 50) =>
  getItems(uid, page, pageSize, 'ON_SALE', { force: false })
export const itemSoldOutList = (uid: string, page = 1, pageSize = 50) =>
  getItems(uid, page, pageSize, 'SOLD_OUT', { force: false })
export const itemAllList = (uid: string, page = 1, pageSize = 100) =>
  getItems(uid, page, pageSize, 'ALL', { force: false })
export const itemPolish = (uid: string, itemId = '') => polishItem(uid, itemId)
// 上架要带 item_context 给后端 build_item_open_payload 用 —— 没 tbCatId/
// channelCatId/leafId 闲鱼会返 FAIL_BIZ_CHANNEL_CAT_ID_PATH_QUERY_ERROR。
// 字段 lookup 顺序：item.detail.*（落库后的权威源） > item.* 顶级 > attributesMap。
export const itemUpshelf = (uid: string, itemId: string, item: any = {}) => {
  const detail = (item && typeof item === 'object' && item.detail && typeof item.detail === 'object') ? item.detail : {}
  const attrs = (item && typeof item === 'object' && item.attributesMap) || {}
  const pick = (...keys: any[]): string => {
    for (const v of keys) {
      const s = String(v ?? '').trim()
      if (s) return s
    }
    return ''
  }
  const pickBool = (...keys: any[]): boolean => {
    for (const v of keys) {
      if (v === true) return true
      if (v === '1' || v === 1) return true
      if (v === false || v === '0' || v === 0) return false
    }
    return false
  }
  const ctx = {
    tbCatId: pick(detail.tbCatId, item.tbCatId, attrs.tbCatId),
    channelCatId: pick(detail.channelCatId, item.channelCatId, attrs.cCatId),
    leafId: pick(detail.leafId, item.leafId, attrs.lfId),
    addressType: detail.addressType ?? item.addressType ?? attrs.addressType,
    province: pick(item.province, detail.province),
    city: pick(item.city, detail.city),
    area: pick(item.area, detail.area),
    divisionId: pick(item.divisionId, detail.divisionId, attrs.cainiaoDivision),
    gps: pick(item.gps, detail.gps),
    noBargain: pickBool(detail.noBargain, attrs.noBargain, item.noBargain),
    supportFreight: pickBool(detail.supportFreight, attrs.supportFreight, item.supportFreight),
    freeShipping: pickBool(detail.freeShip, item.freeShip, attrs.freeShipping),
    simpleItem: pickBool(detail.simpleItem, attrs.simItem, item.simpleItem),
    publishScene: pick(detail.publishScene, attrs.pScene, 'mainPublish'),
    sourceId: pick(detail.sourceId, attrs.pSourceId, 'main_publish_item'),
  }
  return itemOpen(uid, itemId, '', ctx)
}
export const syncItems = (uid: string) =>
  request<{ ok: boolean; total: number; synced_at: number; detail?: string }>(
    'POST', '/api/items/sync', { uid },
  )

/**
 * 一键同步所有 active 账号的商品。后端会以受限并发（默认 4）扇出每个账号的
 * sync_items_to_db，全部完成后一次性返回每个账号的结果。前端只发一个请求。
 */
export interface SyncAllAccountResult {
  uid: string
  ok: boolean
  item_count: number
  synced_at: number
  detail?: string
}
export interface SyncAllResponse {
  status: 'ok' | 'error'
  total: number
  success_count: number
  fail_count: number
  total_items: number
  accounts: SyncAllAccountResult[]
}
export const syncAllItems = () =>
  request<SyncAllResponse>('POST', '/api/items/sync_all')

// ==================== 发布后通用能力：闲鱼币 / 2人小刀 ====================

/** 查询闲鱼币抵扣可用比例（同 orders.ts 里的 coinDeductionRatio，这里本地定义避免跨模块依赖） */
const queryCoinRatio = (uid: string, price = '100', itemId = '') =>
  request(
    'GET',
    `/api/coin/deduction/ratio?uid=${encodeURIComponent(uid)}&price=${encodeURIComponent(price)}${itemId ? `&item_id=${encodeURIComponent(itemId)}` : ''}`,
  )

export interface PostPublishExtra {
  label: string
  ok: boolean
  detail?: string
  appliedPrice?: string
  mode?: string
  discountRate?: string
}

export interface PostPublishExtraRunResult {
  itemId: string
  results: PostPublishExtra[]
}

const POST_PUBLISH_ITEM_NOT_FOUND_RE = /商品不存在/
const POST_PUBLISH_RETRY_DELAYS_MS = [1500, 2500]

export function extractPublishedItemId(resp: any) {
  return String(
    resp?.data?.itemId
    || resp?.data?.itemDO?.id
    || resp?.data?.itemDO?.itemId
    || resp?.data?.itemInfo?.itemId
    || resp?.data?.model?.itemId
    || resp?.data?.id
    || resp?.itemId
    || resp?.id
    || '',
  ).trim()
}

function waitMs(ms: number) {
  if (!Number.isFinite(ms) || ms <= 0) return Promise.resolve()
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

function isPostPublishItemNotReady(result: PostPublishExtra | null | undefined) {
  if (!result || result.ok) return false
  return POST_PUBLISH_ITEM_NOT_FOUND_RE.test(`${result.label} ${result.detail || ''}`)
}

async function syncFreshlyPublishedItem(uid: string, delayMs: number) {
  await waitMs(delayMs)
  await syncItems(uid).catch(() => null)
}

function normalizeMoneyText(raw: unknown) {
  const n = Number(String(raw ?? '').replace(/[^\d.]/g, ''))
  if (!Number.isFinite(n) || n <= 0) return ''
  return n.toFixed(2)
}

function buildDiscountFallbackOptions(itemPriceRaw: unknown) {
  const itemPrice = Number(normalizeMoneyText(itemPriceRaw))
  if (!Number.isFinite(itemPrice) || itemPrice <= 0) return []
  return ['80', '85', '90', '95'].map((rate) => {
    const finalPrice = (itemPrice * Number(rate) / 100).toFixed(2)
    return {
      value: rate,
      discountRate: rate,
      price: finalPrice,
      grouponPrice: finalPrice,
      disabled: false,
    }
  })
}

function resolveDiscountOpenChoice(statusResp: any, guideResp: any) {
  const drawer = statusResp?.data?.drawer || {}
  const options = Array.isArray(drawer?.options) && drawer.options.length
    ? drawer.options
    : buildDiscountFallbackOptions(drawer?.itemPrice || statusResp?.data?.price || guideResp?.data?.price)
  const available = options
    .map((entry: any) => {
      const discountRate = String(entry?.discountRate ?? entry?.value ?? '').trim()
      const price = normalizeMoneyText(entry?.price || entry?.grouponPrice || entry?.finalPrice)
      return {
        discountRate,
        price,
        disabled: !!entry?.disabled || !discountRate || !price,
      }
    })
    .filter((entry: any) => !entry.disabled)
    .sort((a: any, b: any) => Number(a.discountRate) - Number(b.discountRate))
  return available[0] || null
}

function resolveAmountOpenChoice(statusResp: any, guideResp: any) {
  const drawer = statusResp?.data?.drawer || {}
  const options = Array.isArray(drawer?.options) ? drawer.options : []
  const available = options
    .map((entry: any) => normalizeMoneyText(entry?.price || entry?.grouponPrice || entry?.finalPrice))
    .filter(Boolean)
    .sort((a: any, b: any) => Number(a) - Number(b))
  if (available.length) return available[0]
  return normalizeMoneyText(
    drawer?.recommendedPrice
    || statusResp?.data?.guidePrice
    || guideResp?.data?.guide?.grouponPrice
    || guideResp?.data?.grouponPrice
    || guideResp?.grouponPrice
    || '',
  )
}

/**
 * 通用闲鱼币抵扣：查比例 → 开启。
 * @returns { label, ok, detail } — 用于 UI 展示结果
 */
export async function enableCoinDeduction(
  uid: string,
  itemId: string,
  price: string,
  preferredPercent: number = 5,
): Promise<PostPublishExtra> {
  try {
    const ratioResp: any = await queryCoinRatio(uid, price || '100', itemId)
    const ratios = ratioResp?.data?.deductionRatioList
      || ratioResp?.data?.coinDeductionInfoList
      || ratioResp?.deductionRatioList || []
    const matched = ratios.find(
      (r: any) => parseInt(String(r?.ratio || r), 10) === preferredPercent,
    )
    const ratio = matched?.ratio || matched || ratios[0]?.ratio || ratios[0] || ''
    if (!ratio) return { label: '币:无可用比例', ok: false, detail: `ratios=${JSON.stringify(ratios)}` }
    const coinResp: any = await itemCoinDeductionOpen(uid, itemId, String(ratio))
    const coinRet = String(coinResp?.ret?.[0] || '')
    if (coinRet.toUpperCase().startsWith('SUCCESS')) {
      return { label: `闲鱼币${ratio}%`, ok: true }
    }
    // 空 ret 不算成功
    const errMsg = coinRet.replace(/^[A-Z_]+::/, '').slice(0, 20) || '未知错误'
    return { label: `币:${errMsg}`, ok: false, detail: coinRet }
  } catch (e: any) {
    return { label: `币:${e?.message?.slice?.(0, 20) || '失败'}`, ok: false, detail: String(e) }
  }
}

/**
 * 通用2人小刀：查guide → 开启。
 * @param delayMs 发布后等待时间(ms)，让闲鱼服务索引新商品。对已有商品传 0。
 * @returns { label, ok, detail } — 用于 UI 展示结果
 */
export async function enableGroupon(
  uid: string,
  itemId: string,
  customPrice: string = '',
  delayMs: number = 0,
): Promise<PostPublishExtra> {
  try {
    if (delayMs > 0) await new Promise(r => setTimeout(r, delayMs))
    const statusResp: any = await itemGrouponStatus(uid, itemId).catch(() => null)
    const guideResp: any = await itemGrouponGuide(uid, itemId)
    const mode = String(statusResp?.data?.drawer?.mode || '').trim() || 'amount'
    if (mode === 'discount') {
      const choice = resolveDiscountOpenChoice(statusResp, guideResp)
      if (!choice) {
        return {
          label: '小刀:无可用折扣档位',
          ok: false,
          detail: JSON.stringify(statusResp?.data?.drawer || guideResp?.data || {}),
          mode: 'discount',
        }
      }
      const gpResp: any = await itemGrouponOpen(uid, itemId, choice.price, {
        mode: 'discount',
        discount_rate: choice.discountRate,
      })
      const gpRet = String(gpResp?.ret?.[0] || gpResp?.detail || '')
      if (gpRet.toUpperCase().includes('SUCCESS') || gpRet.includes('已开启')) {
        const discountLabel = choice.discountRate === '80'
          ? '8折'
          : choice.discountRate === '90'
          ? '9折'
          : `${choice.discountRate}折`
        return {
          label: `小刀${discountLabel}`,
          ok: true,
          appliedPrice: choice.price,
          mode: 'discount',
          discountRate: choice.discountRate,
        }
      }
      const errMsg = gpRet.replace(/^[A-Z_]+::/, '').slice(0, 20) || '失败'
      return { label: `小刀:${errMsg}`, ok: false, detail: gpRet, mode: 'discount' }
    }
    const autoPrice = resolveAmountOpenChoice(statusResp, guideResp)
    const finalPrice = normalizeMoneyText(customPrice.trim() || autoPrice)
    if (!finalPrice) return { label: '小刀:无可用价格', ok: false, detail: JSON.stringify(guideResp?.data), mode: 'amount' }
    const gpResp: any = await itemGrouponOpen(uid, itemId, finalPrice)
    const gpRet = String(gpResp?.ret?.[0] || gpResp?.detail || '')
    if (gpRet.toUpperCase().includes('SUCCESS') || gpRet.includes('已开启')) {
      return { label: `小刀¥${finalPrice}`, ok: true, appliedPrice: finalPrice, mode: 'amount' }
    }
    const errMsg = gpRet.replace(/^[A-Z_]+::/, '').slice(0, 20) || '失败'
    return { label: `小刀:${errMsg}`, ok: false, detail: gpRet, mode: 'amount' }
  } catch (e: any) {
    const msg = String(e?.message || '').slice(0, 30)
    return { label: `小刀:${msg || '失败'}`, ok: false, detail: String(e) }
  }
}

export async function applyPostPublishExtras({
  uid,
  publishResponse = null,
  itemId = '',
  price = '100',
  coinEnabled = false,
  preferredCoinPercent = 5,
  grouponEnabled = false,
  grouponAmount = '',
  grouponDelayMs = 0,
}: {
  uid: string
  publishResponse?: any
  itemId?: string
  price?: string
  coinEnabled?: boolean
  preferredCoinPercent?: number
  grouponEnabled?: boolean
  grouponAmount?: string
  grouponDelayMs?: number
}): Promise<PostPublishExtraRunResult> {
  const resolvedItemId = String(itemId || extractPublishedItemId(publishResponse)).trim()
  if (!resolvedItemId) return { itemId: '', results: [] }

  const fromFreshPublish = !!publishResponse
  if (fromFreshPublish && (coinEnabled || grouponEnabled)) {
    const initialSyncDelayMs = Math.min(Math.max(grouponDelayMs || 0, 1200), 2500)
    await syncFreshlyPublishedItem(uid, initialSyncDelayMs)
  }

  let coinResult: PostPublishExtra | null = null
  let grouponResult: PostPublishExtra | null = null
  if (coinEnabled) {
    coinResult = await enableCoinDeduction(uid, resolvedItemId, price, preferredCoinPercent)
  }
  if (grouponEnabled) {
    grouponResult = await enableGroupon(
      uid,
      resolvedItemId,
      grouponAmount,
      fromFreshPublish ? 0 : grouponDelayMs,
    )
  }
  if (fromFreshPublish) {
    for (const retryDelayMs of POST_PUBLISH_RETRY_DELAYS_MS) {
      const needsRetry = isPostPublishItemNotReady(coinResult) || isPostPublishItemNotReady(grouponResult)
      if (!needsRetry) break
      await syncFreshlyPublishedItem(uid, retryDelayMs)
      if (isPostPublishItemNotReady(coinResult)) {
        coinResult = await enableCoinDeduction(uid, resolvedItemId, price, preferredCoinPercent)
      }
      if (isPostPublishItemNotReady(grouponResult)) {
        grouponResult = await enableGroupon(uid, resolvedItemId, grouponAmount, 0)
      }
    }
  }
  const results = [coinResult, grouponResult].filter(Boolean) as PostPublishExtra[]
  return { itemId: resolvedItemId, results }
}
