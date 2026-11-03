// @ts-nocheck
// Reusable location picker — global capability, anywhere in the app can ask
// the user for a manually entered POI without re-implementing AMap fetch + UI.
//
// Three layers, pick what fits the call site:
//   1) `pickLocation()`  — imperative, async. Mounts a temporary picker into
//      `document.body`, returns the chosen POI (or null on cancel). Cleanest
//      for one-shot flows ("send a location card", "set check-in spot").
//   2) `useLocationPicker()` — React hook. Returns `{ open, picker }`. The
//      picker is the JSX you mount in your tree. Use this when the consumer
//      wants to control where the modal renders (e.g., inside a form wizard)
//      or react to picker state in render.
//   3) `<LocationPickerModal>` — the underlying modal. Use directly if you
//      already have the GPS coords from somewhere else (e.g., a saved
//      address) and just need the user to refine it from a POI list.
//
// Backend dependency: `POST /api/messages/location/{nearby,search}` — these
// proxy AMap so the frontend doesn't need an AMap key. They live under
// `/api/messages/` for historical reasons (chat composer was the first
// caller); the routes themselves are not chat-specific.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import { request } from '../api/index'

export interface PickedPoi {
  /// POI name, e.g. "长沙大学(地铁站)".
  name: string
  /// Short address line, e.g. "洪山路98号长沙学院" (may be empty for some POIs).
  address: string
  /// Latitude as string ("28.193512"). Strings preserved as AMap returned —
  /// fewer rounding surprises (Xianyu wire wants 6 decimals; AMap already
  /// returns that) and matches our backend send payloads.
  latitude: string
  longitude: string
  /// 行政区字段，全部来自高德返的 pname / cityname / adname。`divisionId` 就是
  /// 高德的 adcode（国标 6 位行政区划代码，等同闲鱼/淘宝的 fishAdCode /
  /// divisionId）。整个项目一律用高德这一套，不再走淘宝 POI 接口。
  province?: string
  city?: string
  district?: string
  /// 国标 6 位行政区划代码（adcode）。发布商品当 divisionId 用。
  divisionId?: string
}

// ─── Imperative API ────────────────────────────────────────────────────────

interface PickOptions {
  /// Override the modal title (default "发送位置"). Useful when the picker is
  /// for non-chat flows like "选择发货地点" / "设置常去位置".
  title?: string
  /// Pre-filled GPS, skips the browser geolocation prompt. Use when the
  /// caller already has coords (saved address, account profile, etc.).
  initialOrigin?: { latitude: number; longitude: number }
}

/// Mount a one-shot picker in `document.body` and return the chosen POI.
/// Resolves `null` if the user cancels.
export function pickLocation(opts: PickOptions = {}): Promise<PickedPoi | null> {
  return new Promise((resolve) => {
    // We avoid React's tree by creating a detached host node. This works for
    // any caller (modal, route component, even a vanilla event handler).
    const host = document.createElement('div')
    document.body.appendChild(host)
    let cleanedUp = false
    const cleanup = (result: PickedPoi | null) => {
      if (cleanedUp) return
      cleanedUp = true
      try { ReactRoot?.unmount() } catch { /* ignore */ }
      host.remove()
      resolve(result)
    }
    // Lazy-import react-dom/client to avoid SSR import cost in apps that
    // never use `pickLocation()`.
    let ReactRoot: any = null
    import('react-dom/client').then(({ createRoot }) => {
      const root = createRoot(host)
      ReactRoot = root
      root.render(
        <LocationPickerLauncher
          {...opts}
          onResolve={cleanup}
        />,
      )
    }).catch(() => cleanup(null))
  })
}

function LocationPickerLauncher({
  title,
  initialOrigin,
  onResolve,
}: PickOptions & { onResolve: (poi: PickedPoi | null) => void }) {
  const [origin, setOrigin] = useState<{ lat: number; lng: number } | null>(
    initialOrigin ? { lat: initialOrigin.latitude, lng: initialOrigin.longitude } : null,
  )
  useEffect(() => {
    if (initialOrigin) setOrigin({ lat: initialOrigin.latitude, lng: initialOrigin.longitude })
    else setOrigin(null)
  }, [initialOrigin?.latitude, initialOrigin?.longitude])

  return (
    <LocationPickerModal
      title={title}
      origin={origin}
      onCancel={() => onResolve(null)}
      onPick={(poi) => onResolve(poi)}
    />
  )
}

// ─── Hook API ──────────────────────────────────────────────────────────────

/// React hook variant of `pickLocation`. Returns:
///   - `open(opts?)` — same options + return shape as `pickLocation()`
///   - `picker` — the React node to mount somewhere in your tree (or null)
///
/// You only need `picker` if you want the modal anchored to your component
/// tree (e.g., for theming context). If you just need the result, the
/// imperative `pickLocation()` is simpler.
export function useLocationPicker(): {
  open: (opts?: PickOptions) => Promise<PickedPoi | null>
  picker: ReturnType<typeof LocationPickerLauncher> | null
} {
  const [active, setActive] = useState<{
    opts: PickOptions
    resolve: (poi: PickedPoi | null) => void
  } | null>(null)
  const open = useCallback((opts: PickOptions = {}) => {
    return new Promise<PickedPoi | null>((resolve) => {
      setActive({ opts, resolve })
    })
  }, [])
  const picker = active
    ? (
      <LocationPickerLauncher
        title={active.opts.title}
        initialOrigin={active.opts.initialOrigin}
        onResolve={(poi) => {
          active.resolve(poi)
          setActive(null)
        }}
      />
    )
    : null
  return { open, picker }
}

// ─── Modal (low-level) ─────────────────────────────────────────────────────

/// Headless modal — no GPS prompt, takes pre-resolved `origin`. Use
/// directly when you already have lat/lng (saved address, manual input, etc.)
/// and just need the user to refine via the POI list.
///
/// Three data sources internally:
///   1) nearby POIs (auto-fetched on mount from origin coords)
///   2) AMap inputtips autocomplete (debounced 300ms on `query` change)
///   3) `selected` — last-tapped POI gets a ✓ in either list
///
/// Click anywhere → calls `onPick(poi)`. Backdrop click / ✕ → `onCancel`.
export function LocationPickerModal({
  origin,
  notice,
  title = '发送位置',
  onCancel,
  onPick,
}: {
  origin: { lat: number; lng: number } | null
  notice?: string
  title?: string
  onCancel: () => void
  onPick: (poi: PickedPoi) => void | Promise<void>
}) {
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [nearby, setNearby] = useState<PickedPoi[]>([])
  const [query, setQuery] = useState('')
  const [tips, setTips] = useState<PickedPoi[]>([])
  const [tipsLoading, setTipsLoading] = useState(false)
  const [selected, setSelected] = useState<PickedPoi | null>(null)

  useEffect(() => {
    let cancelled = false
    if (!origin) {
      setNearby([])
      setLoading(false)
      setError(null)
      return () => { cancelled = true }
    }
    setLoading(true)
    setError(null)
    fetchNearbyPois(origin.lat, origin.lng)
      .then((rows) => {
        if (cancelled) return
        setNearby(rows)
        setLoading(false)
      })
      .catch((e) => {
        if (cancelled) return
        setError(e?.message || '加载兴趣点失败')
        setLoading(false)
      })
    return () => { cancelled = true }
  }, [origin?.lat, origin?.lng])

  // 300ms debounce + sequence guard so out-of-order responses don't clobber UI.
  const reqIdRef = useRef(0)
  useEffect(() => {
    const kw = query.trim()
    if (!kw) {
      setTips([])
      setTipsLoading(false)
      return
    }
    const myReq = ++reqIdRef.current
    setTipsLoading(true)
    const timer = window.setTimeout(async () => {
      try {
        const rows = await searchPois(kw, origin)
        if (reqIdRef.current !== myReq) return
        setTips(rows)
      } catch {
        if (reqIdRef.current !== myReq) return
        setTips([])
      } finally {
        if (reqIdRef.current === myReq) setTipsLoading(false)
      }
    }, 300)
    return () => window.clearTimeout(timer)
  }, [query, origin?.lat, origin?.lng])

  const isSelected = useMemo(() => {
    if (!selected) return () => false
    return (p: PickedPoi) =>
      p.name === selected.name && p.latitude === selected.latitude && p.longitude === selected.longitude
  }, [selected])

  const content = (
    <div
      className="liquid-scrim"
      style={{
        position: 'fixed', inset: 0,
        zIndex: 2147483647,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: 16,
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onCancel() }}
    >
      <div
        className="liquid-panel"
        style={{
          width: 'min(460px, 94vw)',
          height: 'min(560px, 86vh)',
          borderRadius: 26,
          display: 'flex', flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        <div style={{
          display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          padding: '14px 16px', borderBottom: '1px solid var(--control-border)',
        }}>
          <span style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-main)' }}>{title}</span>
          <button
            type="button" onClick={onCancel} aria-label="关闭"
            style={{ border: 'none', background: 'transparent', color: 'var(--text-muted)', fontSize: 22, lineHeight: 1, cursor: 'pointer', padding: 4 }}
          >×</button>
        </div>

        <div style={{ position: 'relative', padding: '10px 16px' }}>
          {notice ? (
            <div
              style={{
                marginBottom: 8,
                borderRadius: 12,
                background: 'var(--status-warning-bg)',
                color: 'var(--status-warning-text)',
                fontSize: 12,
                lineHeight: 1.45,
                padding: '8px 10px',
              }}
            >
              {notice}
            </div>
          ) : null}
          <div style={{ position: 'relative' }}>
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="输入地址或位置名称"
              style={{
                width: '100%', boxSizing: 'border-box',
                padding: '10px 36px 10px 14px',
                borderRadius: 12, border: '1px solid var(--control-border)',
                background: 'var(--surface-control)', fontSize: 14, color: 'var(--text-main)', outline: 'none',
              }}
            />
            <span style={{
              position: 'absolute', right: 12, top: '50%', transform: 'translateY(-50%)',
              color: 'var(--text-muted)', fontSize: 14, pointerEvents: 'none',
            }}>🔍</span>
          </div>
          {query.trim() && (
            <div className="liquid-list-panel" style={{
              position: 'absolute', left: 16, right: 16, top: '100%',
              borderRadius: 14,
              maxHeight: 280, overflowY: 'auto', zIndex: 1,
            }}>
              {tipsLoading ? (
                <PlaceholderRow text="搜索中…" />
              ) : tips.length === 0 ? (
                <PlaceholderRow text="无匹配结果" />
              ) : tips.map((t, i) => (
                <PoiRow
                  key={`tip-${t.name}-${t.latitude}-${i}`}
                  poi={t}
                  divider={i < tips.length - 1}
                  variant="compact"
                  onClick={() => {
                    setSelected(t)
                    setQuery(t.name)
                    setTips([])
                    onPick(t)
                  }}
                />
              ))}
            </div>
          )}
        </div>

        <div style={{ flex: 1, overflowY: 'auto', minHeight: 120, padding: '0 0 8px' }}>
          {!origin && !query.trim() ? (
            <PlaceholderRow text="输入地址或位置名称后选择发送" padded />
          ) : loading ? (
            <PlaceholderRow text="正在加载附近位置…" padded />
          ) : error ? (
            <PlaceholderRow text={error} tone="error" padded />
          ) : nearby.length === 0 ? (
            <PlaceholderRow text="无附近位置" padded />
          ) : (
            nearby.map((p, i) => (
              <PoiRow
                key={`${p.name}-${p.latitude}-${p.longitude}-${i}`}
                poi={p}
                checked={isSelected(p)}
                onClick={() => { setSelected(p); onPick(p) }}
              />
            ))
          )}
        </div>
      </div>
    </div>
  )
  return createPortal(content, document.body)
}

// ─── Backend proxy helpers ─────────────────────────────────────────────────

async function fetchNearbyPois(lat: number, lng: number): Promise<PickedPoi[]> {
  const resp = await request<any>('POST', '/api/messages/location/nearby', {
    latitude: lat,
    longitude: lng,
    radius: 500,
    page_size: 30,
  })
  const pois = Array.isArray(resp?.pois) ? resp.pois : []
  return pois
    .map((p: any): PickedPoi => ({
      name: String(p?.name || '').trim(),
      address: String(p?.address || '').trim(),
      latitude: String(p?.latitude || lat).trim(),
      longitude: String(p?.longitude || lng).trim(),
      // 高德返的 pname/cityname/adname → 省市区；adcode = 国标 6 位行政区
      // 代码，发布商品的 divisionId / 闲鱼 fishAdCode 也都用这个。
      province: String(p?.province || '').trim(),
      city: String(p?.city || '').trim(),
      district: String(p?.district || '').trim(),
      divisionId: String(p?.adcode || '').trim(),
    }))
    .filter((p: PickedPoi) => p.name)
}

async function searchPois(
  keyword: string,
  origin?: { lat: number; lng: number } | null,
): Promise<PickedPoi[]> {
  const payload: any = { keyword }
  if (origin) {
    payload.origin_latitude = origin.lat
    payload.origin_longitude = origin.lng
  }
  const resp = await request<any>('POST', '/api/messages/location/search', payload)
  const arr = Array.isArray(resp?.tips) ? resp.tips : []
  return arr
    .map((t: any): PickedPoi => ({
      name: String(t?.name || ''),
      address: String(t?.address || ''),
      latitude: String(t?.latitude || ''),
      longitude: String(t?.longitude || ''),
      // inputtips 的 district 字段是「市+区」一体的字符串（如"长沙市岳麓区"），
      // 我们存到 district 里前端不分拆；adcode 才是机器可读的关键字段。
      district: String(t?.district || ''),
      divisionId: String(t?.adcode || ''),
    }))
    .filter((t: PickedPoi) => t.name && t.latitude && t.longitude)
}

// ─── Internal sub-components ───────────────────────────────────────────────

function PoiRow({
  poi, checked, divider = true, variant = 'normal', onClick,
}: {
  poi: PickedPoi
  checked?: boolean
  divider?: boolean
  variant?: 'normal' | 'compact'
  onClick: () => void
}) {
  const padding = variant === 'compact' ? '10px 14px' : '12px 16px'
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: 'flex', alignItems: 'center', gap: 12,
        width: '100%', textAlign: 'left',
        padding,
        border: 'none',
        borderBottom: divider ? '1px solid var(--surface-soft)' : 'none',
        background: 'transparent',
        cursor: 'pointer',
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'rgba(255, 255, 255, 0.62)' }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent' }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{
          fontSize: variant === 'compact' ? 13 : 14,
          fontWeight: variant === 'compact' ? 500 : 600,
          color: 'var(--text-main)', marginBottom: 2,
        }}>
          {poi.name}
        </div>
        {(poi.address || variant === 'normal') && (
          <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.4 }}>
            {poi.address || '—'}
          </div>
        )}
      </div>
      {checked && (
        <span style={{ color: 'var(--primary)', fontSize: 18, fontWeight: 700, flexShrink: 0 }}>✓</span>
      )}
    </button>
  )
}

function PlaceholderRow({
  text, tone = 'muted', padded = false,
}: { text: string; tone?: 'muted' | 'error'; padded?: boolean }) {
  return (
    <div style={{
      padding: padded ? '32px 16px' : '14px 16px',
      textAlign: 'center',
      color: tone === 'error' ? 'var(--danger)' : 'var(--text-muted)',
      fontSize: 13,
    }}>
      {text}
    </div>
  )
}
