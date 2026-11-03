// @ts-nocheck
import { useEffect, useMemo, useState } from 'react'
import { Loader2, MapPin, Search, Trash2 } from 'lucide-react'
import { Modal } from '../../components/Modal'
import { useSensitiveText } from '../../components/SensitiveValue'
import {
  deletePublishLocation,
  getPublishLocation,
  locationByGps,
  putPublishLocation,
  request,
  type PublishLocationRecord,
} from '../../api/index'
import { emitAccountsSynced } from '../runtimeEvents'

interface PublishLocationModalProps {
  uid: string
  nick?: string
  open: boolean
  onClose: () => void
  onSaved?: (record: PublishLocationRecord | null) => void | Promise<void>
}

interface PublishLocationCandidate {
  name: string
  address: string
  latitude: string
  longitude: string
  province: string
  city: string
  district: string
  divisionId: string
}

export function PublishLocationModal({
  uid,
  nick,
  open,
  onClose,
  onSaved,
}: PublishLocationModalProps) {
  const maskedNick = useSensitiveText('account', nick)
  const [current, setCurrent] = useState<PublishLocationRecord | null>(null)
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [clearing, setClearing] = useState(false)
  const [searching, setSearching] = useState(false)
  const [error, setError] = useState('')
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState<PublishLocationCandidate | null>(null)
  const [results, setResults] = useState<PublishLocationCandidate[]>([])
  const [searchOrigin, setSearchOrigin] = useState<{ latitude: number; longitude: number } | null>(null)

  useEffect(() => {
    if (!open) return
    let cancelled = false
    setLoading(true)
    setError('')
    setSearch('')
    setSelected(null)
    setResults([])
    getPublishLocation(uid)
      .then((locResp) => {
        if (cancelled) return
        setCurrent(locResp?.record ?? null)
      })
      .catch((e) => {
        if (!cancelled) setError(e?.message || '加载失败')
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => { cancelled = true }
  }, [open, uid])

  useEffect(() => {
    if (!open || searchOrigin || !('geolocation' in navigator)) return
    navigator.geolocation.getCurrentPosition(
      (pos) => {
        setSearchOrigin({
          latitude: pos.coords.latitude,
          longitude: pos.coords.longitude,
        })
      },
      () => {},
      { enableHighAccuracy: false, timeout: 8000, maximumAge: 300000 },
    )
  }, [open, searchOrigin])

  useEffect(() => {
    if (!open) return
    const q = search.trim()
    if (!q) {
      setResults([])
      setSearching(false)
      return
    }
    let cancelled = false
    setSearching(true)
    const timer = window.setTimeout(async () => {
      try {
        const resp: any = await request('POST', '/api/messages/location/search', {
          keyword: q,
          origin_latitude: searchOrigin?.latitude,
          origin_longitude: searchOrigin?.longitude,
        })
        if (cancelled) return
        const tips = Array.isArray(resp?.tips) ? resp.tips : []
        setResults(tips.map((tip: any) => ({
          name: String(tip?.name || ''),
          address: String(tip?.address || ''),
          latitude: String(tip?.latitude || ''),
          longitude: String(tip?.longitude || ''),
          province: String(tip?.province || ''),
          city: String(tip?.city || ''),
          district: String(tip?.district || ''),
          divisionId: String(tip?.adcode || tip?.divisionId || ''),
        })).filter((tip: PublishLocationCandidate) => tip.name && tip.latitude && tip.longitude))
      } catch (e: any) {
        if (!cancelled) {
          setResults([])
          setError(e?.message || '搜索地点失败')
        }
      } finally {
        if (!cancelled) setSearching(false)
      }
    }, 300)

    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [open, search, searchOrigin])

  const selectedSummary = useMemo(() => {
    if (!selected) return ''
    return [selected.province, selected.city, selected.district].filter(Boolean).join(' · ')
  }, [selected])

  async function chooseCandidate(candidate: PublishLocationCandidate) {
    setError('')
    try {
      const enriched: any = await locationByGps(uid, Number(candidate.latitude), Number(candidate.longitude)).catch(() => null)
      setSelected({
        ...candidate,
        province: String(enriched?.prov || candidate.province || ''),
        city: String(enriched?.city || candidate.city || ''),
        district: String(enriched?.area || candidate.district || ''),
        divisionId: String(enriched?.divisionId || candidate.divisionId || ''),
      })
    } catch (e: any) {
      setError(e?.message || '选择发布地失败')
    }
  }

  async function handleSave() {
    if (!selected) {
      setError('请先选择一个发布地')
      return
    }
    setSaving(true)
    setError('')
    try {
      const resp = await putPublishLocation(uid, {
        fishAdCode: selected.divisionId,
        gps: `${selected.latitude},${selected.longitude}`,
        latitude: selected.latitude,
        longitude: selected.longitude,
        name: selected.name,
        address: selected.address,
        province: selected.province,
        city: selected.city,
        district: selected.district,
      })
      await onSaved?.(resp?.record ?? null)
      emitAccountsSynced({ uid, source: 'publish-location-save' })
      onClose()
    } catch (e: any) {
      setError(e?.message || '保存失败')
    } finally {
      setSaving(false)
    }
  }

  async function handleClear() {
    setClearing(true)
    setError('')
    try {
      await deletePublishLocation(uid)
      await onSaved?.(null)
      emitAccountsSynced({ uid, source: 'publish-location-clear' })
      onClose()
    } catch (e: any) {
      setError(e?.message || '清空失败')
    } finally {
      setClearing(false)
    }
  }

  const currentSummary = useMemo(() => {
    if (!current) return ''
    const currentName = String(current.name || '').trim()
    const currentAddress = String(current.address || '').trim()
    if (currentName) {
      return [current.province, current.city, currentName].filter(Boolean).join(' · ')
    }
    if (currentAddress) {
      return [current.province, current.city, current.district, currentAddress].filter(Boolean).join(' · ')
    }
    return [current.province, current.city, current.district].filter(Boolean).join(' · ')
  }, [current])

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={`设置发布地${nick ? ` · ${maskedNick}` : ''}`}
      width={560}
      footer={(
        <>
          {current ? (
            <button
              onClick={handleClear}
              className="btn-secondary btn-sm"
              disabled={saving || clearing}
            >
              {clearing ? '清空中…' : '清空设置'}
            </button>
          ) : null}
          <button onClick={onClose} className="btn-secondary btn-sm" disabled={saving || clearing}>取消</button>
          <button onClick={handleSave} className="btn-primary btn-sm disabled:opacity-40" disabled={saving || clearing || !selected}>
            {saving ? '保存中…' : '保存'}
          </button>
        </>
      )}
    >
      <div className="space-y-3">
        {current ? (
          <div className="rounded-lg px-3 py-2 text-[12px]" style={{ background: 'var(--surface-soft)', color: 'var(--text-main)' }}>
            <div className="flex items-center gap-1.5">
              <MapPin className="w-3 h-3" />
              <span>当前发布地：</span>
              <span className="font-medium">{currentSummary || current.fishAdCode}</span>
            </div>
            <div className="mt-1 text-[11px] font-mono" style={{ color: 'var(--text-muted)' }}>
              {current.fishAdCode}{current.gps ? ` · ${current.gps}` : ''}
            </div>
          </div>
        ) : (
          <div className="rounded-lg px-3 py-2 text-[12px]" style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>
            当前未设置发布地。你可以现在设置，也可以以后在发布商品时单独选择。
          </div>
        )}

        {selected ? (
          <div className="rounded-lg px-3 py-2 text-[12px]" style={{ background: 'var(--primary-light)', color: 'var(--primary)' }}>
            <div className="font-medium">{selected.name}</div>
            <div className="mt-1" style={{ color: 'var(--text-secondary)' }}>
              {selectedSummary || selected.address || '已选择位置'}
            </div>
            <div className="mt-1 text-[11px] font-mono" style={{ color: 'var(--text-muted)' }}>
              {selected.divisionId}{selected.latitude ? ` · ${selected.latitude},${selected.longitude}` : ''}
            </div>
          </div>
        ) : null}

        <label className="block relative">
          <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2" style={{ color: 'var(--text-muted)' }} />
          <input
            className="input pl-9"
            placeholder="搜索地点（省 / 市 / 区 / 学校 / 小区）"
            value={search}
            onChange={(e) => {
              setSearch(e.target.value)
              setError('')
            }}
          />
          {searching ? (
            <Loader2 className="w-3.5 h-3.5 absolute right-3 top-1/2 -translate-y-1/2 animate-spin" style={{ color: 'var(--text-muted)' }} />
          ) : null}
        </label>

        {loading ? (
          <div className="text-center py-6 text-[13px]" style={{ color: 'var(--text-muted)' }}>加载中…</div>
        ) : (
          <div
            className="rounded-lg overflow-y-auto"
            style={{ maxHeight: 320, border: '1px solid var(--border-light)' }}
          >
            {!search.trim() ? (
              <div className="text-center py-6 text-[13px]" style={{ color: 'var(--text-muted)' }}>
                输入关键词搜索真实发布地位置
              </div>
            ) : results.length === 0 ? (
              <div className="text-center py-6 text-[13px]" style={{ color: 'var(--text-muted)' }}>
                {searching ? '搜索中…' : '无匹配位置'}
              </div>
            ) : (
              results.map((candidate, index) => {
                const active = selected?.name === candidate.name
                  && selected?.latitude === candidate.latitude
                  && selected?.longitude === candidate.longitude
                return (
                  <button
                    key={`${candidate.name}-${candidate.latitude}-${candidate.longitude}-${index}`}
                    type="button"
                    onClick={() => chooseCandidate(candidate)}
                    className="w-full text-left px-3 py-2 transition-colors cursor-pointer"
                    style={{
                      background: active ? 'var(--control-active)' : 'rgba(255,255,255,0.48)',
                      borderTop: '1px solid var(--control-border)',
                    }}
                  >
                    <div className="flex items-center justify-between gap-3">
                      <div className="min-w-0">
                        <div className="text-[13px] font-medium truncate" style={{ color: 'var(--text-main)' }}>
                          {candidate.name}
                        </div>
                        <div className="text-[11px] truncate" style={{ color: 'var(--text-muted)' }}>
                          {candidate.address || candidate.district || '—'}
                        </div>
                      </div>
                      <div className="text-[11px] font-mono shrink-0" style={{ color: 'var(--text-muted)' }}>
                        {candidate.divisionId || '—'}
                      </div>
                    </div>
                  </button>
                )
              })
            )}
          </div>
        )}

        {error ? (
          <div className="liquid-notice liquid-notice-danger rounded-xl px-3 py-2 text-[12px] flex items-center gap-1.5">
            <Trash2 className="w-3 h-3" />
            {error}
          </div>
        ) : null}
      </div>
    </Modal>
  )
}
