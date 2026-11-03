// @ts-nocheck
import { useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, Loader2, MapPin, Search, X } from 'lucide-react'
import { Modal } from '../../components/Modal'
import {
  getPublishLocation,
  locationByGps,
  locationDivisions,
  request,
} from '../../api/index'
import { getAuthToken } from '../../api/client'

const RECENT_REGION_KEY = 'xy.search.region.recent.v1'
const DEFAULT_NEARBY_RADIUS = '5000'
const NEARBY_RADIUS_OPTIONS = [
  { label: '1km', value: '1000' },
  { label: '5km', value: '5000' },
  { label: '10km', value: '10000' },
  { label: '15km', value: '15000' },
  { label: '50km', value: '50000' },
] as const

type RegionTabKey = 'division' | 'nearby'

type DivisionNode = {
  id?: string | number
  na?: string
  level?: string
  prov?: string
  city?: string
  area?: string
  divisionId?: string
  hasChild?: boolean
}

type OriginState = {
  latitude: string
  longitude: string
  label: string
  source: 'geolocation' | 'publish' | 'recent' | 'manual'
}

function trimText(value: unknown) {
  return String(value ?? '').trim()
}

function dedupeParts(parts: Array<string | undefined>) {
  const next: string[] = []
  for (const raw of parts) {
    const value = trimText(raw)
    if (!value) continue
    if (next[next.length - 1] !== value) next.push(value)
  }
  return next
}

function provinceDisplayName(value: string) {
  const text = trimText(value)
  return text
    .replace(/壮族自治区|回族自治区|维吾尔自治区|特别行政区|自治区$/u, '')
    .replace(/省$/u, '')
    .replace(/市$/u, '')
}

function normalizeDivisionNode(entry: any) {
  if (!entry || typeof entry !== 'object') return null
  return {
    ...entry,
    id: trimText(entry?.id || entry?.divisionId),
    divisionId: trimText(entry?.divisionId || entry?.id),
    na: trimText(entry?.na || entry?.name || entry?.prov || entry?.city || entry?.area),
    level: trimText(entry?.level),
    prov: trimText(entry?.prov || entry?.province || entry?.pname),
    city: trimText(entry?.city || entry?.cityName || entry?.cname),
    area: trimText(entry?.area || entry?.district || entry?.adname),
    hasChild: entry?.hasChild ?? entry?.haschild ?? false,
  }
}

function regionSummary(region: any | null) {
  if (!region) return '全国'
  if (trimText(region.distance)) {
    const parts = dedupeParts([region.province, region.city, region.name || region.district])
    return parts.join(' · ') || trimText(region.name) || '附近'
  }
  const parts = dedupeParts([region.province, region.city, region.district || region.name])
  return parts.join(' · ') || trimText(region.name) || '区域'
}

function regionMeta(region: any | null) {
  if (!region) return ''
  const distance = trimText(region.distance)
  if (!distance) return '当前区域'
  const km = Number(distance) / 1000
  return Number.isFinite(km) && km > 0 ? `附近 ${km}km` : '附近'
}

function parseGps(gps: string) {
  const [latitude = '', longitude = ''] = trimText(gps).split(',').map((part) => part.trim())
  return { latitude, longitude }
}

function regionKey(region: any | null) {
  if (!region) return 'nation'
  return [
    trimText(region?.name),
    trimText(region?.distance),
    trimText(region?.divisionId || region?.fishAdCode),
    trimText(region?.latitude),
    trimText(region?.longitude),
    trimText(region?.customGps),
  ].join('|')
}

function poiKey(poi: any | null) {
  if (!poi) return ''
  return [
    trimText(poi?.name),
    trimText(poi?.latitude),
    trimText(poi?.longitude),
  ].join('|')
}

function extractPoiFromRegion(region: any | null) {
  if (!region) return null
  try {
    const parsed = JSON.parse(trimText(region?.extraFilterValue) || '{}')
    const poi = Array.isArray(parsed?.poiList) ? parsed.poiList[0] : null
    if (poi?.gps) {
      const { latitude, longitude } = parseGps(trimText(poi.gps))
      if (latitude && longitude) {
        return {
          name: trimText(poi?.poiName || region?.name),
          latitude,
          longitude,
        }
      }
    }
  } catch {
    // noop
  }
  if (trimText(region?.distance) && trimText(region?.latitude) && trimText(region?.longitude)) {
    return {
      name: trimText(region?.name),
      latitude: trimText(region?.latitude),
      longitude: trimText(region?.longitude),
    }
  }
  return null
}

function recentRegionStorageKey() {
  const token = getAuthToken()
  if (!token) return `${RECENT_REGION_KEY}:guest`
  let hash = 5381
  for (let i = 0; i < token.length; i += 1) {
    hash = ((hash << 5) + hash) ^ token.charCodeAt(i)
  }
  return `${RECENT_REGION_KEY}:${(hash >>> 0).toString(36)}`
}

function loadRecentRegions() {
  try {
    const raw = localStorage.getItem(recentRegionStorageKey())
    const list = JSON.parse(raw || '[]')
    return Array.isArray(list) ? list.filter(Boolean) : []
  } catch {
    return []
  }
}

function saveRecentRegion(region: any) {
  if (!region || !trimText(region.name) && !trimText(region.district)) return
  const current = loadRecentRegions()
  const key = [
    trimText(region.name),
    trimText(region.district),
    trimText(region.fishAdCode || region.divisionId),
    trimText(region.distance),
    trimText(region.latitude),
    trimText(region.longitude),
  ].join('|')
  const next = [region, ...current.filter((entry: any) => {
    const entryKey = [
      trimText(entry?.name),
      trimText(entry?.district),
      trimText(entry?.fishAdCode || entry?.divisionId),
      trimText(entry?.distance),
      trimText(entry?.latitude),
      trimText(entry?.longitude),
    ].join('|')
    return entryKey !== key
  })].slice(0, 8)
  try {
    localStorage.setItem(recentRegionStorageKey(), JSON.stringify(next))
  } catch {
    // ignore storage failure in constrained webviews
  }
}

function buildDivisionRegion(entry: any) {
  const province = trimText(entry?.province || entry?.prov || entry?.pname)
  const city = trimText(entry?.city || entry?.cityName || entry?.cname)
  const district = trimText(entry?.district || entry?.area || entry?.adname || entry?.na)
  return {
    name: district || city || province || '区域',
    latitude: '',
    longitude: '',
    fishAdCode: trimText(entry?.fishAdCode || entry?.divisionId || entry?.id),
    province,
    city,
    district,
    divisionId: trimText(entry?.fishAdCode || entry?.divisionId || entry?.id),
    distance: '',
    customGps: '',
    customDistance: '',
    userPositionJson: JSON.stringify({
      province,
      city,
      district,
    }),
    extraFilterValue: JSON.stringify({
      divisionList: [{
        province,
        city,
        ...(district ? { area: district } : {}),
      }],
    }),
  }
}

async function enrichPoi(uid: string, poi: any) {
  const latitude = trimText(poi?.latitude)
  const longitude = trimText(poi?.longitude)
  let extra: any = null
  if (latitude && longitude) {
    try {
      extra = await locationByGps(uid, Number(latitude), Number(longitude))
    } catch {
      extra = null
    }
  }
  return {
    ...poi,
    latitude,
    longitude,
    province: trimText(poi?.province) || trimText(extra?.prov),
    city: trimText(poi?.city) || trimText(extra?.city),
    district: trimText(poi?.district) || trimText(extra?.area),
    divisionId: trimText(poi?.divisionId) || trimText(poi?.adcode) || trimText(extra?.divisionId),
  }
}

function buildPoiRegion(poi: any, radius: string, origin: OriginState | null) {
  const province = trimText(poi?.province)
  const city = trimText(poi?.city)
  const district = trimText(poi?.district)
  const latitude = trimText(origin?.latitude || poi?.latitude)
  const longitude = trimText(origin?.longitude || poi?.longitude)
  const customGps = dedupeParts([origin?.latitude, origin?.longitude]).length === 2
    ? `${trimText(origin?.latitude)},${trimText(origin?.longitude)}`
    : `${trimText(poi?.latitude)},${trimText(poi?.longitude)}`
  return {
    name: trimText(poi?.name) || district || city || province || '附近',
    latitude,
    longitude,
    fishAdCode: trimText(poi?.divisionId || poi?.fishAdCode),
    province,
    city,
    district,
    divisionId: trimText(poi?.divisionId || poi?.fishAdCode),
    distance: trimText(radius),
    customGps,
    customDistance: trimText(radius),
    userPositionJson: JSON.stringify({
      province,
      city,
      district,
    }),
    extraFilterValue: JSON.stringify({
      poiList: [{
        area: district,
        city,
        displayName: dedupeParts([city, poi?.name]).join('·'),
        divisionId: trimText(poi?.divisionId || poi?.fishAdCode),
        gps: `${trimText(poi?.latitude)},${trimText(poi?.longitude)}`,
        poiName: trimText(poi?.name),
        prov: province,
      }],
    }),
  }
}

async function fetchNearbyPois(origin: OriginState, radius: string) {
  const resp: any = await request('POST', '/api/messages/location/nearby', {
    latitude: Number(origin.latitude),
    longitude: Number(origin.longitude),
    radius: Number(radius),
    page_size: 30,
  })
  const list = Array.isArray(resp?.pois) ? resp.pois : []
  return list.map((poi: any) => ({
    name: trimText(poi?.name),
    address: trimText(poi?.address),
    latitude: trimText(poi?.latitude),
    longitude: trimText(poi?.longitude),
    province: trimText(poi?.province),
    city: trimText(poi?.city),
    district: trimText(poi?.district),
    divisionId: trimText(poi?.adcode),
  })).filter((poi: any) => poi.name && poi.latitude && poi.longitude)
}

async function searchPois(keyword: string, origin: OriginState | null) {
  const resp: any = await request('POST', '/api/messages/location/search', {
    keyword,
    origin_latitude: origin ? Number(origin.latitude) : undefined,
    origin_longitude: origin ? Number(origin.longitude) : undefined,
  })
  const list = Array.isArray(resp?.tips) ? resp.tips : []
  return list.map((poi: any) => ({
    name: trimText(poi?.name),
    address: trimText(poi?.address),
    latitude: trimText(poi?.latitude),
    longitude: trimText(poi?.longitude),
    district: trimText(poi?.district),
    divisionId: trimText(poi?.adcode),
  })).filter((poi: any) => poi.name && poi.latitude && poi.longitude)
}

async function fetchDivisionChildren(uid: string, parentId: string | number) {
  const resp: any = await request(
    'GET',
    `/api/location/division_children?uid=${encodeURIComponent(uid)}&parent_id=${encodeURIComponent(String(parentId || 0))}`,
  )
  return Array.isArray(resp?.items) ? resp.items : []
}

export function RegionFilterControl({
  uid,
  region,
  onApply,
}: {
  uid: string
  region: any | null
  onApply: (region: any | null) => void
}) {
  const [open, setOpen] = useState(false)
  const [tab, setTab] = useState<RegionTabKey>('division')
  const [divisionLoading, setDivisionLoading] = useState(false)
  const [divisionError, setDivisionError] = useState('')
  const [provinceNodes, setProvinceNodes] = useState<DivisionNode[]>([])
  const [selectedProvinceNode, setSelectedProvinceNode] = useState<DivisionNode | null>(null)
  const [cityNodes, setCityNodes] = useState<DivisionNode[]>([])
  const [selectedCityNode, setSelectedCityNode] = useState<DivisionNode | null>(null)
  const [areaNodes, setAreaNodes] = useState<DivisionNode[]>([])
  const [publishLocation, setPublishLocation] = useState<any | null>(null)
  const [recents, setRecents] = useState<any[]>([])
  const [radius, setRadius] = useState(trimText(region?.distance) || DEFAULT_NEARBY_RADIUS)
  const [origin, setOrigin] = useState<OriginState | null>(null)
  const [originLoading, setOriginLoading] = useState(false)
  const [originNotice, setOriginNotice] = useState('')
  const [originError, setOriginError] = useState('')
  const [nearbyLoading, setNearbyLoading] = useState(false)
  const [nearbyError, setNearbyError] = useState('')
  const [nearbyList, setNearbyList] = useState<any[]>([])
  const [query, setQuery] = useState('')
  const [searching, setSearching] = useState(false)
  const [searchResults, setSearchResults] = useState<any[]>([])
  const [draftRegion, setDraftRegion] = useState<any | null>(region ? { ...region } : null)
  const [selectedNearbyPoi, setSelectedNearbyPoi] = useState<any | null>(null)
  const [selectingPoiKey, setSelectingPoiKey] = useState('')
  const [locateVersion, setLocateVersion] = useState(0)
  const searchReqIdRef = useRef(0)

  const currentSummary = regionSummary(region)
  const draftSummary = regionSummary(draftRegion)
  const draftMeta = regionMeta(draftRegion)
  const currentRegionId = regionKey(region)
  const draftRegionId = regionKey(draftRegion)

  const draftPoi = useMemo(() => {
    if (selectedNearbyPoi) return selectedNearbyPoi
    return extractPoiFromRegion(draftRegion)
  }, [selectedNearbyPoi, draftRegionId])

  const draftPoiId = poiKey(draftPoi)

  const nearbyCenterLabel = useMemo(() => {
    const lat = trimText(origin?.latitude || draftPoi?.latitude || draftRegion?.latitude)
    const lng = trimText(origin?.longitude || draftPoi?.longitude || draftRegion?.longitude)
    return lat && lng ? `${lat}, ${lng}` : ''
  }, [origin, draftPoi, draftRegionId])

  useEffect(() => {
    if (!open) return
      setRecents(loadRecentRegions())
      setDraftRegion(region ? { ...region } : null)
      setSelectedNearbyPoi(null)
      setSelectingPoiKey('')
    setRadius(trimText(region?.distance) || DEFAULT_NEARBY_RADIUS)
    setQuery('')
    setSearchResults([])
    setNearbyError('')
    setNearbyList([])
    setOrigin(null)
    setOriginNotice('')
    setOriginError('')
    setTab(trimText(region?.distance) ? 'nearby' : 'division')
    let cancelled = false
    setDivisionLoading(true)
    setDivisionError('')
    Promise.all([
      locationDivisions(uid).catch(() => ({ divisionArea: [] })),
      getPublishLocation(uid).catch(() => null),
    ]).then(([divisionResp, publishResp]: any[]) => {
      if (cancelled) return
      const items = Array.isArray(divisionResp?.divisionArea) ? divisionResp.divisionArea : []
      const normalized = items
        .map((entry: any) => normalizeDivisionNode(entry))
        .filter((entry: any) => entry && trimText(entry?.divisionId))
      setProvinceNodes(normalized)
      const preferredProvince = normalized.find((entry: any) => trimText(entry?.prov) === trimText(region?.province))
      setSelectedProvinceNode(preferredProvince || null)
      setSelectedCityNode(null)
      setCityNodes([])
      setAreaNodes([])
      setPublishLocation(publishResp)
    }).catch((error: any) => {
      if (cancelled) return
      setDivisionError(error?.message || '加载地区失败')
    }).finally(() => {
      if (!cancelled) setDivisionLoading(false)
    })
    return () => { cancelled = true }
  }, [open, uid, region])

  useEffect(() => {
    if (!selectedNearbyPoi) return
    setDraftRegion(buildPoiRegion(selectedNearbyPoi, radius, origin))
  }, [selectedNearbyPoi, radius, origin])

  useEffect(() => {
    if (!open || !selectedProvinceNode || tab !== 'division') return
    let cancelled = false
    setDivisionLoading(true)
    setDivisionError('')
    setSelectedCityNode(null)
    setAreaNodes([])
    fetchDivisionChildren(uid, trimText(selectedProvinceNode?.divisionId || selectedProvinceNode?.id))
      .then((items) => {
        if (cancelled) return
        const normalized = items
          .map((entry: any) => normalizeDivisionNode(entry))
          .filter((entry: any) => entry && trimText(entry?.divisionId))
        setCityNodes(normalized)
        const preferredCity = normalized.find((entry: any) => trimText(entry?.city || entry?.na) === trimText(region?.city))
        setSelectedCityNode(preferredCity || normalized[0] || null)
      })
      .catch((error: any) => {
        if (cancelled) return
        setDivisionError(error?.message || '加载城市失败')
        setCityNodes([])
      })
      .finally(() => {
        if (!cancelled) setDivisionLoading(false)
      })
    return () => { cancelled = true }
  }, [open, selectedProvinceNode, tab, uid, region])

  useEffect(() => {
    if (!open || !selectedCityNode || tab !== 'division') return
    let cancelled = false
    setDivisionLoading(true)
    setDivisionError('')
    fetchDivisionChildren(uid, trimText(selectedCityNode?.divisionId || selectedCityNode?.id))
      .then((items) => {
        if (cancelled) return
        const normalized = items
          .map((entry: any) => normalizeDivisionNode(entry))
          .filter((entry: any) => entry && trimText(entry?.divisionId))
        setAreaNodes(normalized)
      })
      .catch((error: any) => {
        if (cancelled) return
        setDivisionError(error?.message || '加载区域失败')
        setAreaNodes([])
      })
      .finally(() => {
        if (!cancelled) setDivisionLoading(false)
      })
    return () => { cancelled = true }
  }, [open, selectedCityNode, tab, uid])

  useEffect(() => {
    if (!open || tab !== 'nearby') return
    let cancelled = false
    async function resolveOrigin() {
      setOriginLoading(true)
      setOriginNotice('')
      setOriginError('')

      const fromPublishRecord = () => {
        const record = publishLocation?.record
        if (!record) return null
        const latitude = trimText(record?.latitude)
        const longitude = trimText(record?.longitude)
        if (!latitude || !longitude) return null
        return {
          latitude,
          longitude,
          label: dedupeParts([record?.province, record?.city, record?.name || record?.district]).join(' · ') || '账号发布地',
          source: 'publish' as const,
        }
      }

      const fromCurrentRegion = () => {
        if (!region) return null
        const customGps = parseGps(trimText(region?.customGps))
        const latitude = customGps.latitude || trimText(region?.latitude)
        const longitude = customGps.longitude || trimText(region?.longitude)
        if (!latitude || !longitude) return null
        return {
          latitude,
          longitude,
          label: currentSummary,
          source: 'recent' as const,
        }
      }

      const fromRecent = () => {
        const first = loadRecentRegions().find((entry: any) => {
          const customGps = parseGps(trimText(entry?.customGps))
          return trimText(customGps.latitude || entry?.latitude) && trimText(customGps.longitude || entry?.longitude)
        })
        if (!first) return null
        const customGps = parseGps(trimText(first?.customGps))
        return {
          latitude: customGps.latitude || trimText(first?.latitude),
          longitude: customGps.longitude || trimText(first?.longitude),
          label: regionSummary(first),
          source: 'recent' as const,
        }
      }

      const applyFallback = () => {
        const fallback = fromPublishRecord() || fromCurrentRegion() || fromRecent()
        if (fallback) {
          setOrigin(fallback)
          setOriginNotice(
            fallback.source === 'publish'
              ? '定位不可用，已改用账号发布地作为附近搜索中心'
              : '定位不可用，已改用最近一次位置作为附近搜索中心',
          )
        } else {
          setOrigin(null)
          setOriginNotice('当前无法自动定位。你仍可手动搜索学校、小区、商圈后按附近范围筛选。')
        }
      }

      if (!('geolocation' in navigator)) {
        setOriginError('当前运行环境不支持浏览器定位')
        applyFallback()
        setOriginLoading(false)
        return
      }

      navigator.geolocation.getCurrentPosition(async (pos) => {
        if (cancelled) return
        try {
          const area: any = await locationByGps(uid, pos.coords.latitude, pos.coords.longitude).catch(() => null)
          if (cancelled) return
          setOrigin({
            latitude: String(pos.coords.latitude),
            longitude: String(pos.coords.longitude),
            label: dedupeParts([area?.prov, area?.city, area?.area]).join(' · ') || '当前位置',
            source: 'geolocation',
          })
          setOriginNotice('')
        } finally {
          if (!cancelled) setOriginLoading(false)
        }
      }, (error) => {
        if (cancelled) return
        const reason = error?.code === 1
          ? '定位权限被拒绝'
          : error?.code === 2
            ? '当前位置不可用'
            : error?.code === 3
              ? '定位超时'
              : (error?.message || '定位失败')
        setOriginError(reason)
        applyFallback()
        setOriginLoading(false)
      }, {
        enableHighAccuracy: false,
        timeout: 6000,
        maximumAge: 300000,
      })
    }
    resolveOrigin()
    return () => { cancelled = true }
  }, [open, tab, uid, publishLocation, currentSummary, region, locateVersion])

  useEffect(() => {
    if (!open || tab !== 'nearby' || query.trim()) return
    if (!origin) {
      setNearbyList([])
      setNearbyError(originNotice ? '' : '暂无可用搜索中心')
      return
    }
    let cancelled = false
    setNearbyLoading(true)
    setNearbyError('')
    fetchNearbyPois(origin, radius)
      .then((rows) => {
        if (cancelled) return
        setNearbyList(rows)
      })
      .catch((e: any) => {
        if (cancelled) return
        setNearbyError(e?.message || '加载附近位置失败')
      })
      .finally(() => {
        if (!cancelled) setNearbyLoading(false)
      })
    return () => { cancelled = true }
  }, [open, tab, origin, radius, query, originNotice])

  useEffect(() => {
    if (!open || tab !== 'nearby') return
    const keyword = query.trim()
    if (!keyword) {
      setSearchResults([])
      setSearching(false)
      return
    }
    const reqId = ++searchReqIdRef.current
    setSearching(true)
    const timer = window.setTimeout(async () => {
      try {
        const rows = await searchPois(keyword, origin)
        if (searchReqIdRef.current !== reqId) return
        setSearchResults(rows)
      } catch {
        if (searchReqIdRef.current !== reqId) return
        setSearchResults([])
      } finally {
        if (searchReqIdRef.current === reqId) setSearching(false)
      }
    }, 250)
    return () => window.clearTimeout(timer)
  }, [open, tab, query, origin])

  function close() {
    setOpen(false)
  }

  function applyRegion(nextRegion: any | null) {
    if (nextRegion) saveRecentRegion(nextRegion)
    setRecents(loadRecentRegions())
    onApply(nextRegion)
    close()
  }

  async function selectNearbyPoiCandidate(poi: any) {
    const key = poiKey(poi)
    setSelectingPoiKey(key)
    try {
      const enriched = await enrichPoi(uid, poi)
      setSelectedNearbyPoi(enriched)
      setDraftRegion(buildPoiRegion(enriched, radius, origin))
    } finally {
      setSelectingPoiKey('')
    }
  }

  function retryLocate() {
    setOrigin(null)
    setOriginNotice('')
    setOriginError('')
    setNearbyError('')
    setNearbyList([])
    setSelectedNearbyPoi(null)
    setLocateVersion((value) => value + 1)
  }

  function confirmDraftRegion() {
    applyRegion(draftRegion)
  }

  const confirmDisabled = !!selectingPoiKey || (tab === 'nearby' && !trimText(draftRegion?.distance))
  const confirmLabel = trimText(draftRegion?.distance)
    ? '查看附近结果'
    : draftRegion
      ? '查看区域结果'
      : '查看全部结果'

  return (
    <>
      <div className="w-full sm:w-auto">
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="w-full sm:w-auto px-3 py-1.5 rounded-lg text-[12px] font-medium flex items-center justify-between sm:justify-start gap-1.5 cursor-pointer transition-colors"
          style={{
            background: region ? 'var(--primary-light)' : 'var(--surface-soft)',
            color: region ? 'var(--primary)' : 'var(--text-main)',
            border: `1px solid ${region ? 'var(--primary)' : 'var(--control-border)'}`,
          }}
        >
          <span className="truncate">
            {region ? `区域: ${currentSummary}` : '区域'}
          </span>
          <ChevronDown className="w-3.5 h-3.5" />
        </button>
      </div>

      <Modal
        open={open}
        onClose={close}
        width={620}
        footer={(
          <div className="flex w-full flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div className="min-w-0">
              <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                已选：{draftSummary}
              </div>
              <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>
                {tab === 'nearby' && !trimText(draftRegion?.distance)
                  ? '先选一个附近地点，再按查看应用筛选'
                  : draftMeta || '点击查看后应用筛选'}
              </div>
            </div>
            <div className="flex w-full items-center gap-2 md:w-auto">
              {draftRegionId !== currentRegionId ? (
                <button
                  type="button"
                  onClick={() => {
                    setDraftRegion(region ? { ...region } : null)
                    setSelectedNearbyPoi(null)
                  }}
                  className="h-[38px] px-4 rounded-[10px] text-[13px] font-medium cursor-pointer"
                  style={{ background: 'var(--surface-soft)', color: 'var(--text-main)' }}
                >
                  重置
                </button>
              ) : null}
              <button
                type="button"
                onClick={confirmDraftRegion}
                disabled={confirmDisabled}
                className="h-[38px] flex-1 md:min-w-[200px] rounded-[10px] text-[13px] font-semibold cursor-pointer transition-opacity"
                style={{
                  background: confirmDisabled ? 'var(--surface-soft)' : 'var(--primary)',
                  color: confirmDisabled ? 'var(--text-muted)' : 'var(--text-main)',
                }}
              >
                {confirmLabel}
              </button>
            </div>
          </div>
        )}
      >
        <div className="space-y-3">
          <div
            className="-mx-6 -mt-6 px-5 pt-4 pb-3 flex items-start justify-between"
            style={{ borderBottom: '1px solid var(--control-border)' }}
          >
            <div className="flex items-center gap-6">
              {[
                { key: 'division', label: '选地区' },
                { key: 'nearby', label: '搜附近' },
              ].map((entry) => {
                const active = tab === entry.key
                return (
                  <button
                    key={entry.key}
                    type="button"
                    onClick={() => setTab(entry.key as RegionTabKey)}
                    className="relative pb-2 text-[14px] md:text-[16px] font-semibold cursor-pointer transition-colors"
                    style={{ color: active ? 'var(--text-main)' : 'var(--text-secondary)' }}
                  >
                    {entry.label}
                    {active ? (
                      <span
                        className="absolute left-0 right-0 -bottom-[1px] h-[4px] rounded-full"
                        style={{ background: 'var(--primary)' }}
                      />
                    ) : null}
                  </button>
                )
              })}
            </div>
            <button
              type="button"
              onClick={close}
              className="w-9 h-9 -mr-1 rounded-full flex items-center justify-center cursor-pointer transition-colors"
              style={{ color: 'var(--text-main)' }}
              aria-label="关闭"
            >
              <X className="w-4.5 h-4.5" />
            </button>
          </div>

          {tab === 'division' ? (
            <div className="rounded-[12px] overflow-hidden" style={{ border: '1px solid var(--control-border)' }}>
              <div className="grid grid-cols-[112px_132px_minmax(0,1fr)] md:grid-cols-[124px_148px_minmax(0,1fr)]">
                <div className="max-h-[340px] overflow-y-auto" style={{ background: 'var(--surface-soft)', borderRight: '1px solid var(--control-border)' }}>
                  <button
                    type="button"
                    onClick={() => {
                      setSelectedProvinceNode(null)
                      setSelectedCityNode(null)
                      setCityNodes([])
                      setAreaNodes([])
                      setSelectedNearbyPoi(null)
                      setDraftRegion(null)
                    }}
                    className="w-full px-3 py-2.5 text-left text-[12px] font-medium cursor-pointer"
                    style={{
                      background: !selectedProvinceNode ? 'var(--control-active)' : 'transparent',
                      color: !selectedProvinceNode ? 'var(--primary)' : 'var(--text-main)',
                    }}
                  >
                    全国
                  </button>
                  {provinceNodes.map((node) => {
                    const active = trimText(selectedProvinceNode?.divisionId || selectedProvinceNode?.id) === trimText(node?.divisionId || node?.id)
                    return (
                      <button
                        key={trimText(node?.divisionId || node?.id || node?.na)}
                        type="button"
                        onClick={() => setSelectedProvinceNode(node)}
                        className="w-full px-3 py-2.5 text-left text-[12px] font-medium cursor-pointer"
                        style={{
                          background: active ? 'var(--control-active)' : 'transparent',
                          color: active ? 'var(--primary)' : 'var(--text-main)',
                        }}
                      >
                        {provinceDisplayName(trimText(node?.na || node?.prov))}
                      </button>
                    )
                  })}
                </div>

                <div className="max-h-[340px] overflow-y-auto" style={{ borderRight: '1px solid var(--control-border)', background: 'var(--glass-bg-soft)' }}>
                  {divisionLoading && cityNodes.length === 0 ? (
                    <div className="py-12 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>
                      <Loader2 className="w-4 h-4 animate-spin inline-block mr-1" />
                      加载城市中…
                    </div>
                  ) : divisionError ? (
                    <div className="py-12 px-4 text-center text-[13px]" style={{ color: 'var(--danger)' }}>{divisionError}</div>
                  ) : !selectedProvinceNode ? (
                    <button
                      type="button"
                      onClick={() => {
                        setSelectedNearbyPoi(null)
                        setDraftRegion(null)
                      }}
                      className="w-full px-4 py-3 text-left cursor-pointer flex items-center justify-between gap-3"
                      style={{ color: 'var(--primary)' }}
                    >
                      <div>
                        <div className="text-[16px] font-semibold">全国</div>
                        <div className="mt-1 text-[12px]" style={{ color: 'var(--text-muted)' }}>清除区域筛选，查看全部结果</div>
                      </div>
                      {draftRegion === null ? (
                        <span className="text-[20px] font-bold" style={{ color: 'var(--primary)' }}>✓</span>
                      ) : null}
                    </button>
                  ) : cityNodes.length === 0 ? (
                    <div className="py-12 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>先选省份</div>
                  ) : (
                    cityNodes.map((entry, index) => {
                      const active = trimText(selectedCityNode?.divisionId || selectedCityNode?.id) === trimText(entry?.divisionId || entry?.id)
                      return (
                        <button
                          key={`${trimText(entry?.divisionId || entry?.id)}-${index}`}
                          type="button"
                          onClick={() => setSelectedCityNode(entry)}
                          className="w-full px-4 py-3 text-left cursor-pointer flex items-center justify-between gap-3"
                          style={{
                            borderTop: index === 0 ? 'none' : '1px solid var(--control-border)',
                            background: active ? 'var(--control-active)' : 'var(--surface-control)',
                          }}
                        >
                          <div className="min-w-0">
                            <div className="text-[15px] font-semibold" style={{ color: active ? 'var(--primary)' : 'var(--text-main)' }}>
                              {trimText(entry?.na || entry?.city)}
                            </div>
                            <div className="mt-1 text-[12px] truncate" style={{ color: 'var(--text-muted)' }}>
                              {dedupeParts([entry?.prov, entry?.city]).join(' · ')}
                            </div>
                          </div>
                          {active ? (
                            <span className="text-[18px] font-bold shrink-0" style={{ color: 'var(--primary)' }}>✓</span>
                          ) : null}
                        </button>
                      )
                    })
                  )}
                </div>

                <div className="max-h-[340px] overflow-y-auto" style={{ background: 'var(--glass-bg-soft)' }}>
                  {divisionLoading && areaNodes.length === 0 ? (
                    <div className="py-12 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>
                      <Loader2 className="w-4 h-4 animate-spin inline-block mr-1" />
                      加载区域中…
                    </div>
                  ) : !selectedCityNode ? (
                    <div className="py-12 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>先选城市</div>
                  ) : areaNodes.length === 0 ? (
                    <div className="py-12 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>暂无区域数据</div>
                  ) : (
                    areaNodes.map((entry, index) => {
                      const nextRegion = buildDivisionRegion(entry)
                      const active = !trimText(draftRegion?.distance)
                        && trimText(draftRegion?.divisionId || draftRegion?.fishAdCode) === trimText(nextRegion?.divisionId)
                      return (
                        <button
                          key={`${trimText(entry?.divisionId || entry?.id)}-${index}`}
                          type="button"
                          onClick={() => {
                            setSelectedNearbyPoi(null)
                            setDraftRegion(nextRegion)
                          }}
                          className="w-full px-4 py-3 text-left cursor-pointer flex items-center justify-between gap-3"
                          style={{
                            borderTop: index === 0 ? 'none' : '1px solid var(--control-border)',
                            background: active ? 'var(--control-active)' : 'var(--surface-control)',
                          }}
                        >
                          <div className="min-w-0">
                            <div className="text-[15px] font-semibold" style={{ color: active ? 'var(--primary)' : 'var(--text-main)' }}>
                              {trimText(entry?.na || entry?.area)}
                            </div>
                            <div className="mt-1 text-[12px] truncate" style={{ color: 'var(--text-muted)' }}>
                              {dedupeParts([entry?.prov, entry?.city, entry?.area]).join(' · ')}
                            </div>
                          </div>
                          {active ? (
                            <span className="text-[18px] font-bold shrink-0" style={{ color: 'var(--primary)' }}>✓</span>
                          ) : null}
                        </button>
                      )
                    })
                  )}
                </div>
              </div>
            </div>
          ) : null}

          {tab === 'nearby' ? (
            <div className="space-y-3">
              <div className="rounded-[12px] p-3" style={{ border: '1px solid var(--control-border)', background: 'var(--surface-control)' }}>
                <div className="flex items-center gap-2">
                  <div className="relative flex-1">
                    <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2" style={{ color: 'var(--text-muted)' }} />
                    <input
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                      placeholder="搜索学校 / 小区 / 商圈 / 地标"
                      className="input pl-9"
                    />
                  </div>
                  <button
                    type="button"
                    onClick={retryLocate}
                    className="h-[36px] px-3 rounded-[10px] text-[12px] font-medium cursor-pointer"
                    style={{ background: 'var(--surface-soft)', color: 'var(--text-main)', border: '1px solid var(--control-border)' }}
                  >
                    重新定位
                  </button>
                </div>
              </div>

              <div className="rounded-[12px] overflow-hidden" style={{ border: '1px solid var(--control-border)', background: 'var(--surface-control)' }}>
                <div className="px-4 py-3 border-b" style={{ borderColor: 'var(--control-border)', background: 'var(--glass-bg-soft)' }}>
                  <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                    {originLoading ? '正在获取附近搜索中心…' : '附近搜索中心'}
                  </div>
                  <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                    {origin ? origin.label : '未获取定位，可直接搜索学校、小区、商圈后按附近范围筛选。'}
                  </div>
                  {originError ? (
                    <div className="mt-1 text-[11px]" style={{ color: 'var(--danger)' }}>{originError}</div>
                  ) : null}
                  {originNotice ? (
                    <div className="mt-1 text-[11px]" style={{ color: 'var(--status-warning-text)' }}>{originNotice}</div>
                  ) : null}
                  {nearbyCenterLabel ? (
                    <div className="mt-1 text-[10px]" style={{ color: 'var(--text-muted)' }}>
                      中心坐标 {nearbyCenterLabel}
                    </div>
                  ) : null}
                  {draftPoi?.name ? (
                    <div className="mt-2 inline-flex max-w-full items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px]" style={{ background: 'var(--primary-light)', color: 'var(--primary)' }}>
                      <MapPin className="w-3.5 h-3.5 shrink-0" />
                      <span className="truncate">{draftPoi.name}</span>
                    </div>
                  ) : null}
                </div>

                <div className="border-t px-4 py-3" style={{ borderColor: 'var(--control-border)' }}>
                  <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>搜索范围</div>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                  {NEARBY_RADIUS_OPTIONS.map((option) => {
                    const active = radius === option.value
                    return (
                      <button
                        key={option.value}
                        type="button"
                        onClick={() => setRadius(option.value)}
                        className="px-3 py-1.5 rounded-full text-[12px] font-medium cursor-pointer"
                        style={{
                          background: active ? 'var(--primary-light)' : 'var(--surface-soft)',
                          color: active ? 'var(--primary)' : 'var(--text-main)',
                          border: `1px solid ${active ? 'var(--primary)' : 'transparent'}`,
                        }}
                      >
                        {option.label}
                      </button>
                    )
                  })}
                </div>
                </div>

                <div className="max-h-[240px] overflow-y-auto border-t" style={{ borderColor: 'var(--control-border)' }}>
                {query.trim() ? (
                  searching ? (
                    <div className="py-10 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>
                      <Loader2 className="w-4 h-4 animate-spin inline-block mr-1" />
                      搜索附近位置中…
                    </div>
                  ) : searchResults.length === 0 ? (
                    <div className="py-10 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>无匹配位置</div>
                  ) : (
                    searchResults.map((poi, index) => (
                      (() => {
                        const active = poiKey(poi) === draftPoiId
                        const selecting = selectingPoiKey === poiKey(poi)
                        return (
                      <button
                        key={`${poi.name}-${poi.latitude}-${index}`}
                        type="button"
                        onClick={() => selectNearbyPoiCandidate(poi)}
                        className="w-full px-4 py-3 text-left cursor-pointer"
                        style={{
                          borderTop: index === 0 ? 'none' : '1px solid var(--control-border)',
                          background: active ? 'var(--control-active)' : 'var(--surface-control)',
                        }}
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="text-[14px] font-semibold" style={{ color: active ? 'var(--primary)' : 'var(--text-main)' }}>{poi.name}</div>
                            <div className="mt-1 text-[12px]" style={{ color: 'var(--text-muted)' }}>{poi.address || poi.district || '—'}</div>
                          </div>
                          {selecting ? (
                            <Loader2 className="w-4 h-4 animate-spin shrink-0 mt-0.5" style={{ color: 'var(--primary)' }} />
                          ) : active ? (
                            <span className="text-[18px] font-bold shrink-0" style={{ color: 'var(--primary)' }}>✓</span>
                          ) : null}
                        </div>
                      </button>
                        )
                      })()
                    ))
                  )
                ) : nearbyLoading ? (
                  <div className="py-10 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>
                    <Loader2 className="w-4 h-4 animate-spin inline-block mr-1" />
                    正在加载附近位置…
                  </div>
                ) : nearbyError ? (
                  <div className="py-10 text-center text-[13px]" style={{ color: 'var(--danger)' }}>{nearbyError}</div>
                ) : nearbyList.length === 0 ? (
                  <div className="py-10 text-center text-[13px]" style={{ color: 'var(--text-muted)' }}>暂无附近位置，试试上方搜索</div>
                ) : (
                  nearbyList.map((poi, index) => (
                    (() => {
                      const active = poiKey(poi) === draftPoiId
                      const selecting = selectingPoiKey === poiKey(poi)
                      return (
                    <button
                      key={`${poi.name}-${poi.latitude}-${index}`}
                      type="button"
                      onClick={() => selectNearbyPoiCandidate(poi)}
                      className="w-full px-4 py-3 text-left cursor-pointer"
                      style={{
                        borderTop: index === 0 ? 'none' : '1px solid var(--control-border)',
                        background: active ? 'var(--control-active)' : 'var(--surface-control)',
                      }}
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <div className="text-[14px] font-semibold" style={{ color: active ? 'var(--primary)' : 'var(--text-main)' }}>{poi.name}</div>
                          <div className="mt-1 text-[12px]" style={{ color: 'var(--text-muted)' }}>{poi.address || dedupeParts([poi.city, poi.district]).join(' · ') || '—'}</div>
                        </div>
                        {selecting ? (
                          <Loader2 className="w-4 h-4 animate-spin shrink-0 mt-0.5" style={{ color: 'var(--primary)' }} />
                        ) : active ? (
                          <span className="text-[18px] font-bold shrink-0" style={{ color: 'var(--primary)' }}>✓</span>
                        ) : null}
                      </div>
                    </button>
                      )
                    })()
                  ))
                )}
                </div>
              </div>
            </div>
          ) : null}
        </div>
      </Modal>
    </>
  )
}
