// @ts-nocheck
// 搜索页业务状态 hook —— DesktopSearch / MobileSearch 共享同一份数据与 handler。
//
// 仅负责：账号 / 关键词 / 排序 / 通道 / 动态筛选 / 结果列表 / 多选 / 分页。
// UI 选项（filter panel 展开 / 排序栏样式 / batch modal 风格）由各端自行 useState。

import { useCallback, useEffect, useRef, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { useAccounts } from '../../../hooks/useAccounts'
import { search, accountsHealth, refreshAccount } from '../../../api/index'
import { useAuthScope } from '../../../hooks/useAuthScope'

/* ── Types ──────────────────────────────────────────────────────────── */

export interface SearchItem {
  itemId: string
  sellerId: string
  title: string
  price: string
  picUrl: string
  sellerNick: string
  area: string
  userAvatar: string
  wantNum: number
  wantText: string
  freeShip: boolean
  sellerBadgeUrl: string
  userExtraTags: string[]
  infoTags: string[]
  statTags: string[]
  serviceTags: string[]
}

export interface DynFilter {
  propId: string
  propName: string
  values: { valueId: string; valueName: string }[]
}

export interface SearchRegionFilter {
  name: string
  latitude: string
  longitude: string
  fishAdCode?: string
  province?: string
  city?: string
  district?: string
  divisionId?: string
  distance?: string
  customGps?: string
  customDistance?: string
  userPositionJson?: string
  extraFilterValue?: string
}

export type SourceMode = 'auto' | 'app' | 'pc' | 'wx'
export type PriceDir = 'asc' | 'desc'

export const SOURCE_MODES = [
  { label: '自动', value: 'auto' },
  { label: 'APP', value: 'app' },
  { label: 'PC', value: 'pc' },
  { label: '微信', value: 'wx' },
] as const

function isFixedAreaFilter(propId: unknown, propName?: unknown, valueName?: unknown) {
  const id = String(propId ?? '').trim().toLowerCase()
  const name = String(propName ?? '').trim()
  const value = String(valueName ?? '').trim()
  return name === '区域'
    || value === '区域'
    || id.includes('areafilter')
    || id.includes('client_area')
}

/* ── Parsers (lifted from SearchPage.tsx, behavior preserved) ───────── */

function parseFishTags(fishTags: any) {
  const infoTags: string[] = []
  const statTags: string[] = []
  const serviceTags: string[] = []
  if (!fishTags || typeof fishTags !== 'object') return { infoTags, statTags, serviceTags }
  for (const t of fishTags.r2?.tagList ?? []) {
    const c = t?.data?.content
    if (c && typeof c === 'string') infoTags.push(c)
  }
  for (const t of fishTags.r3?.tagList ?? []) {
    const c = t?.data?.content
    if (c && typeof c === 'string') statTags.push(c)
  }
  for (const rk of ['r4', 'r5']) {
    for (const t of fishTags[rk]?.tagList ?? []) {
      const c = t?.data?.content
      if (c && typeof c === 'string' && !c.startsWith('freeShipping')) serviceTags.push(c)
    }
  }
  return { infoTags, statTags, serviceTags }
}

export function parseResultList(rawList: any[]): SearchItem[] {
  return rawList.map((entry: any) => {
    const main = entry?.data?.item?.main
    const ex = main?.exContent ?? {}
    const dp = ex?.detailParams ?? {}
    const args = main?.clickParam?.args ?? {}
    const { infoTags, statTags, serviceTags } = parseFishTags(ex.fishTags)
    const wantNum = parseInt(args.wantNum, 10) || 0
    const tagname = (args.tagname ?? '') as string
    const freeShip = tagname.includes('包邮') || (dp.postInfo ?? '').includes('包邮')
      || ex.fishTags?.r1?.tagList?.some((t: any) => t?.data?.content === 'freeShippingIcon')

    let wantText = ''
    if (typeof ex.want === 'string' && ex.want) {
      wantText = ex.want
    } else if (Array.isArray(ex.priceTag)) {
      const pt = ex.priceTag.find((t: any) => t?.data?.text?.includes('想要'))
      if (pt) wantText = pt.data.text
    }
    if (!wantText && wantNum > 0) wantText = `${wantNum}人想要`

    let sellerBadgeUrl = ''
    if (Array.isArray(ex.richTitle)) {
      const img = ex.richTitle.find((t: any) => t.type === 'Image')
      if (img?.data?.url) sellerBadgeUrl = img.data.url
    }

    const userExtraTags: string[] = []
    if (Array.isArray(ex.userExtraTags)) {
      for (const t of ex.userExtraTags) {
        if (t?.text) userExtraTags.push(t.text)
      }
    }

    return {
      itemId: dp.itemId ?? ex.itemId ?? args.id ?? args.item_id ?? entry.itemId ?? entry.id,
      sellerId: dp.userId ?? dp.user_id ?? ex.userId ?? ex.user_id ?? args.userId ?? args.user_id ?? args.sellerId ?? args.seller_id ?? entry.sellerId ?? entry.seller_id ?? '',
      title: dp.title ?? ex.title ?? entry.title ?? '(无标题)',
      price: dp.soldPrice ?? args.price ?? args.displayPrice ?? entry.price ?? '',
      picUrl: ex?.dislikeFeedback?.itemPicUrl ?? dp.picUrl ?? ex.picUrl ?? entry.picUrl ?? entry.pic_url ?? '',
      sellerNick: dp.userNick ?? ex.userNickName ?? ex?.author?.nick ?? entry.sellerNick ?? '',
      area: ex.area ?? args.p_city ?? entry.location ?? '',
      userAvatar: ex.userAvatarUrl ?? '',
      wantNum,
      wantText,
      freeShip,
      sellerBadgeUrl,
      userExtraTags,
      infoTags,
      statTags,
      serviceTags,
    }
  }).filter((it: any) => it.itemId)
}

export function parseDynFilters(data: any): DynFilter[] {
  const panels: DynFilter[] = []
  const seen = new Set<string>()

  const ffGroups = data?.flexFilter?.data ?? []
  for (const group of ffGroups) {
    for (const el of group?.element ?? []) {
      const ft = el.filterType
      if (ft === 'SortStatusButton' || ft === 'FilterStatusButton') {
        for (const v of el.data?.valueList ?? []) {
          if (isFixedAreaFilter(v.propId, v.propName, v.valueName)) continue
          if (v.propId && v.valueId && !seen.has(v.valueId)) {
            seen.add(v.valueId)
            panels.push({ propId: v.propId, propName: v.valueName, values: [{ valueId: v.valueId, valueName: v.valueName }] })
          }
        }
      }
      if (ft === 'CpvNavFilter') {
        const card = el?.data
        const propId = String(card?.pid ?? '').trim()
        const propName = String(card?.pname ?? '').trim()
        const valueList = Array.isArray(card?.pvTermList) ? card.pvTermList : []
        if (!propId || !propName || valueList.length === 0 || seen.has(propId) || isFixedAreaFilter(propId, propName)) continue
        const values = valueList
          .map((v: any) => ({
            valueId: String(v?.vid ?? '').trim(),
            valueName: String(v?.vname ?? '').trim(),
          }))
          .filter((v: any) => v.valueId && v.valueName)
        if (values.length > 0) {
          seen.add(propId)
          panels.push({ propId, propName, values })
        }
      }
    }
  }

  const panelList = data?.smartUiFilterPanelDO?.filterPanelDOList ?? []
  for (const p of panelList) {
    const cd = p.cardData
    if (!cd?.propId || !cd.value?.length) continue
    if (isFixedAreaFilter(cd.propId, cd.propName)) continue
    if (cd.propId === 'priceRange' || cd.propId === 'otherFilter') continue
    if (cd.propId === 'quickFilter') {
      for (const v of cd.value) {
        if (isFixedAreaFilter(v.valueId, cd.propName, v.valueName)) continue
        if (v.valueId && v.valueName && !seen.has(v.valueId)) {
          seen.add(v.valueId)
          panels.push({ propId: 'quickFilter', propName: v.valueName, values: [{ valueId: v.valueId, valueName: v.valueName }] })
        }
      }
      continue
    }
    const values = cd.value
      .filter((v: any) => v.valueId && v.valueName && !seen.has(v.valueId))
      .map((v: any) => ({ valueId: v.valueId, valueName: v.valueName }))
    if (values.length > 0 && !seen.has(cd.propId)) {
      seen.add(cd.propId)
      panels.push({ propId: cd.propId, propName: cd.propName, values })
    }
  }
  return panels
}

/* ── Hook ───────────────────────────────────────────────────────────── */

export function useSearchPageState() {
  const { activeAccounts, loading: accountsLoading, loadAccounts } = useAccounts()
  useEffect(() => { loadAccounts() }, [loadAccounts])
  const [params, setParams] = useSearchParams()

  const [keyword, setKeyword] = useState(params.get('keyword') || '')
  const [loading, setLoading] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [results, setResults] = useState<SearchItem[]>([])
  const [searched, setSearched] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [page, setPage] = useState(1)
  const [hasMore, setHasMore] = useState(false)

  // 多选 + 一键上架
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [showBatchList, setShowBatchList] = useState(false)
  const toggleSelect = useCallback((itemId: string) => {
    setSelectedIds(prev => {
      const next = new Set(prev)
      if (next.has(itemId)) next.delete(itemId)
      else next.add(itemId)
      return next
    })
  }, [])
  const clearSelection = useCallback(() => setSelectedIds(new Set()), [])
  const selectedItems = results.filter(it => selectedIds.has(it.itemId))

  const [sourceMode, setSourceMode] = useState<SourceMode>('auto')
  const [sortField, setSortField] = useState('')
  const [sortValue, setSortValue] = useState('')
  const [priceDir, setPriceDir] = useState<PriceDir>('asc')

  const [dynFilters, setDynFilters] = useState<DynFilter[]>([])
  const [activeFilters, setActiveFilters] = useState<Map<string, string>>(new Map())
  // 附近搜索：用户选完 POI 后存这里，doSearch 自动带 lat/lng/distance 给后端。
  // null 表示「不限位置」（默认）。
  const [nearby, setNearby] = useState<SearchRegionFilter | null>(null)

  const reqIdRef = useRef(0)
  const account = activeAccounts[0] ?? null
  const activeAccountKey = activeAccounts.map((a: any) => a.uid).join('|')
  const activeAccountKeyRef = useRef(activeAccountKey)
  activeAccountKeyRef.current = activeAccountKey
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(() => {
    reqIdRef.current += 1
    setResults([])
    setDynFilters([])
    setSelectedIds(new Set())
    setHasMore(false)
    setPage(1)
    setSearched(false)
    setError(null)
    setLoading(false)
    setLoadingMore(false)
  })

  async function ensureAccountHealthy(uid: string): Promise<boolean> {
    const check = async () => {
      const list: any[] = await accountsHealth()
      const entry = Array.isArray(list) ? list.find((a: any) => a.uid === uid) : null
      return entry?.healthy === true
    }
    try {
      if (await check()) return true
      await refreshAccount(uid)
      return await check()
    } catch { return false }
  }

  const doSearch = useCallback(async (
    kwOverride?: string,
    overrides?: { sf?: string; sv?: string; src?: string; filters?: Map<string, string>; nextPage?: number; region?: SearchRegionFilter | null },
  ) => {
    const kw = (kwOverride ?? keyword).trim()
    if (!kw || !account) return

    const sf = overrides?.sf ?? sortField
    const sv = overrides?.sv ?? sortValue
    const src = overrides?.src ?? sourceMode
    const filters = overrides?.filters ?? activeFilters
    const pg = overrides?.nextPage ?? 1
    const hasRegionOverride = !!overrides && Object.prototype.hasOwnProperty.call(overrides, 'region')
    const region = hasRegionOverride ? (overrides?.region ?? null) : nearby
    const isAppend = pg > 1

    let propValueStr = ''
    if (filters.size > 0) {
      const obj: Record<string, string> = {}
      filters.forEach((vid, pid) => { obj[pid] = vid })
      propValueStr = JSON.stringify(obj)
    }

    setParams({ keyword: kw }, { replace: true })
    setSearched(true)
    if (isAppend) setLoadingMore(true)
    else { setLoading(true); setResults([]); setHasMore(false) }
    setError(null)
    const myReqId = ++reqIdRef.current
    const scope = captureScope(activeAccountKey)
    const isCurrent = () => myReqId === reqIdRef.current && isScopeCurrent(scope, activeAccountKeyRef.current)

    if (!isAppend) {
      const healthy = await ensureAccountHealthy(account.uid)
      if (!isCurrent()) return
      if (!healthy) { setError('账号失效，请重新登录'); setLoading(false); return }
    }

    try {
      const r: any = await search(account.uid, kw, {
        sourceMode: src, sortField: sf, sortValue: sv, propValueStr, page: pg,
        latitude: region?.latitude || '',
        longitude: region?.longitude || '',
        distance: region?.distance || '',
        regionFilter: region ? {
          fishAdCode: region.fishAdCode || region.divisionId || '',
          province: region.province || '',
          city: region.city || '',
          district: region.district || '',
          divisionId: region.divisionId || region.fishAdCode || '',
          latitude: region.latitude || '',
          longitude: region.longitude || '',
          distance: region.distance || '',
          customGps: region.customGps || '',
          customDistance: region.customDistance || '',
          userPositionJson: region.userPositionJson || '',
          extraFilterValue: region.extraFilterValue || '',
        } : null,
      })
      if (!isCurrent()) return
      const rawList: any[] = Array.isArray(r?.data?.resultList) ? r.data.resultList
        : Array.isArray(r?.resultList) ? r.resultList
        : Array.isArray(r?.data) ? r.data : []
      const parsed = parseResultList(rawList)
      if (isAppend) setResults(prev => [...prev, ...parsed])
      else {
        setResults(parsed)
        setDynFilters(parseDynFilters(r?.data ?? r))
      }
      setPage(pg)
      setHasMore(parsed.length >= 10)
    } catch (e: any) {
      if (isCurrent()) setError(e?.message || '搜索失败')
    }
    if (isCurrent()) { setLoading(false); setLoadingMore(false) }
  }, [keyword, account, activeAccountKey, sortField, sortValue, sourceMode, activeFilters, nearby, setParams])

  // 进入页面带 keyword 参数时自动搜索
  useEffect(() => {
    const kwParam = params.get('keyword') || ''
    if (kwParam && account && !searched) {
      setKeyword(kwParam)
      setTimeout(() => doSearch(kwParam), 0)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account?.uid, authToken])

  function handleSort(field: string, value: string) {
    if (field === 'price') {
      if (sortField === 'price') {
        const next = priceDir === 'asc' ? 'desc' : 'asc'
        setPriceDir(next)
        setSortField('price'); setSortValue(next)
        if (searched) doSearch(undefined, { sf: 'price', sv: next })
      } else {
        setSortField('price'); setSortValue(priceDir)
        if (searched) doSearch(undefined, { sf: 'price', sv: priceDir })
      }
    } else {
      const isActive = sortField === field && sortValue === value
      const sf = isActive ? '' : field
      const sv = isActive ? '' : value
      setSortField(sf); setSortValue(sv)
      if (searched) doSearch(undefined, { sf, sv })
    }
  }

  function toggleFilter(propId: string, valueId: string) {
    setActiveFilters(prev => {
      const next = new Map(prev)
      if (next.get(propId) === valueId) next.delete(propId)
      else next.set(propId, valueId)
      if (searched) doSearch(undefined, { filters: next })
      return next
    })
  }

  function clearFilters() {
    setActiveFilters(new Map())
    if (searched) doSearch(undefined, { filters: new Map() })
  }

  function changeSourceMode(mode: SourceMode) {
    setSourceMode(mode)
    if (searched) doSearch(undefined, { src: mode })
  }

  const sortActive = (f: string, v?: string) =>
    sortField === f && (v === undefined || sortValue === v)

  return {
    // 账号
    activeAccounts,
    accountsLoading,
    account,
    // 关键词 + 搜索
    keyword,
    setKeyword,
    doSearch,
    loading,
    loadingMore,
    error,
    searched,
    results,
    page,
    hasMore,
    // 通道
    sourceMode,
    changeSourceMode,
    // 排序
    sortField,
    sortValue,
    priceDir,
    handleSort,
    sortActive,
    // 动态筛选
    dynFilters,
    activeFilters,
    toggleFilter,
    clearFilters,
    // 多选
    selectedIds,
    selectedItems,
    toggleSelect,
    clearSelection,
    showBatchList,
    setShowBatchList,
    // 附近搜索
    nearby,
    setNearby,
  }
}
