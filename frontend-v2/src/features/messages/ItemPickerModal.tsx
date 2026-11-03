// @ts-nocheck
// Item picker shown when the user taps the bag icon in the composer.
//
// "我的宝贝" uses the stable local item cache (`/api/items`). Only "TA的宝贝"
// calls the IM-specific MTOP proxy. Android WebView is unforgiving when that
// IM transport has a transient fetch failure, so keep the default picker path
// isolated from it.
//
// `query` is debounced 300ms and sent verbatim as `queryWord` so the upstream
// does the title filter. Each tab maintains its own state slice; switching
// tabs refetches with `pageNumber=1`.
//
// Rendered via Portal in document.body to escape the conversation panel's
// `overflow-hidden + rounded-[24px]` clip. Uses inline styles for the modal
// chrome so Tailwind 4 arbitrary classes (`z-[9999]`, etc.) don't get tree-
// shaken out of the bundle.
import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Package, Search } from 'lucide-react'

import { getItems, searchSendItems } from '../../api/index'
import { SafeRemoteImage } from './SafeRemoteImage'
import { emitActionFailure, emitActionSuccess } from '../../bootstrap/actionLogging'
import { SensitiveValue, maskSensitiveText, useSensitiveHidden } from '../../components/SensitiveValue'

interface PickerItem {
  itemId: string
  title: string
  mainPic: string
  price: string
}

type Role = 'own' | 'other'

interface ItemPickerModalProps {
  open: boolean
  uid: string
  sessionId: string
  variant?: 'desktop' | 'mobile'
  onClose: () => void
  onPick: (item: PickerItem) => void
}

function normalize(raw: any): PickerItem | null {
  const itemId = String(raw?.itemId ?? raw?.item_id ?? '').trim()
  if (!itemId) return null
  return {
    itemId,
    title: String(raw?.title ?? raw?.itemTitle ?? '').trim() || '无标题',
    // Backend normalises to `mainPic`; tolerate the legacy `picUrl` shape so
    // we don't crash if a stale response ever shows up.
    mainPic: String(raw?.mainPic ?? raw?.picUrl ?? raw?.pic_url ?? '').trim(),
    price: String(raw?.price ?? '').trim(),
  }
}

// Strip a leading "¥" (or other currency prefix) for display — the renderer
// adds its own "¥" so a raw upstream "¥50.00" doesn't render as "¥¥50.00".
function stripPriceSymbol(price: string): string {
  return price.replace(/^[¥￥$]\s*/, '').trim()
}

function normalizeLocalItem(raw: any): PickerItem | null {
  const itemId = String(raw?.itemId ?? raw?.item_id ?? raw?.id ?? '').trim()
  if (!itemId) return null
  return {
    itemId,
    title: String(raw?.title ?? raw?.itemTitle ?? raw?.name ?? '').trim() || '无标题',
    mainPic: String(raw?.mainPic ?? raw?.picUrl ?? raw?.pic_url ?? raw?.pic ?? raw?.imageUrl ?? '').trim(),
    price: String(raw?.price ?? raw?.priceText ?? raw?.price_text ?? '').trim(),
  }
}

async function searchOwnItemsFallback(uid: string, query: string): Promise<PickerItem[]> {
  const load = (force: boolean) => getItems(uid, 1, 30, 'ON_SALE', {
    force,
    requestOptions: {
      timeoutMs: force ? 8000 : 6000,
      timeoutMessage: force ? '商品同步超时，请重试' : '商品加载超时，请重试',
    },
  })

  let resp: any = await load(false)
  let rawList = Array.isArray(resp?.items)
    ? resp.items
    : Array.isArray(resp?.data?.itemList)
      ? resp.data.itemList
      : Array.isArray(resp?.data?.items)
        ? resp.data.items
        : []
  const neverSynced = Number(resp?._last_synced_at || resp?.lastSyncedAt || 0) <= 0
  if (rawList.length === 0 && neverSynced) {
    resp = await load(true)
  }

  rawList = Array.isArray(resp?.items)
    ? resp.items
    : Array.isArray(resp?.data?.itemList)
      ? resp.data.itemList
      : Array.isArray(resp?.data?.items)
        ? resp.data.items
        : []
  const keyword = query.trim().toLowerCase()
  return rawList
    .map(normalizeLocalItem)
    .filter(Boolean)
    .filter((item: PickerItem) => !keyword || item.title.toLowerCase().includes(keyword))
}

export function ItemPickerModal({
  open,
  uid,
  sessionId,
  variant = 'desktop',
  onClose,
  onPick,
}: ItemPickerModalProps) {
  const [role, setRole] = useState<Role>('own')
  const [items, setItems] = useState<PickerItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [query, setQuery] = useState('')
  const [retryKey, setRetryKey] = useState(0)
  // Debounced server-side `queryWord` — separates the input value from the
  // fetch trigger so each keystroke doesn't fire an mtop call.
  const [debouncedQuery, setDebouncedQuery] = useState('')
  const isSensitiveHidden = useSensitiveHidden()

  // Reset state every time the modal opens so a stale "other" tab from a
  // previous session doesn't bleed through.
  useEffect(() => {
    if (!open) return
    setRole('own')
    setQuery('')
    setDebouncedQuery('')
    setError('')
  }, [open])

  // 300ms debounce on the search input.
  useEffect(() => {
    if (!open) return
    const handle = setTimeout(() => setDebouncedQuery(query), 300)
    return () => clearTimeout(handle)
  }, [query, open])

  // Track the latest request so out-of-order responses don't clobber the UI.
  const reqIdRef = useRef(0)

  useEffect(() => {
    if (!open) return
    if (!uid || !sessionId) return
    const reqId = ++reqIdRef.current
    setLoading(true)
    setError('')
    const loadItems = role === 'own'
      ? searchOwnItemsFallback(uid, debouncedQuery)
      : searchSendItems(uid, sessionId, role, debouncedQuery, 1, 20, {
        timeoutMs: 8000,
        timeoutMessage: '商品搜索超时，请重试',
      }).then((res: any) => {
        const rawList = res?.items || []
        return rawList.map(normalize).filter(Boolean) as PickerItem[]
      })

    loadItems
      .then((res: any) => {
        if (reqIdRef.current !== reqId) return
        const nextItems = Array.isArray(res) ? res : []
        setItems(nextItems)
        emitActionSuccess({
          module: 'messages',
          action: 'load_item_picker',
          message: '聊天发送宝贝列表加载成功',
          context: {
            uid,
            sessionId,
            role,
            queryLength: debouncedQuery.length,
            itemCount: nextItems.length,
          },
        })
      })
      .catch(async (err: any) => {
        if (reqIdRef.current !== reqId) return
        setItems([])
        setError(String(err?.message || err || '加载失败'))
        emitActionFailure({
          module: 'messages',
          action: 'load_item_picker',
          message: '聊天发送宝贝列表加载失败',
          error: err,
          context: {
            uid,
            sessionId,
            role,
            queryLength: debouncedQuery.length,
          },
        })
      })
      .finally(() => {
        if (reqIdRef.current !== reqId) return
        setLoading(false)
      })
  }, [open, uid, sessionId, role, debouncedQuery, retryKey])

  // The upstream already filters by `queryWord`, so we don't need a second
  // client-side filter. Memoise just to keep the render path stable.
  const visible = useMemo(() => items, [items])

  if (!open) return null

  const isMobile = variant === 'mobile'
  // 桌面端：紧凑 modal，固定高度，list 内部滚动；手机端：半屏 sheet。
  const panelStyle: React.CSSProperties = isMobile
    ? {
        position: 'absolute',
        right: 0,
        bottom: 'max(env(safe-area-inset-bottom), 8px)',
        left: 0,
        height: 'min(54dvh, 520px)',
        maxHeight: 'calc(100dvh - max(env(safe-area-inset-top), 24px) - max(env(safe-area-inset-bottom), 8px) - 12px)',
        borderTopLeftRadius: 24,
        borderTopRightRadius: 24,
        overflow: 'hidden',
        display: 'flex',
        flexDirection: 'column',
      }
    : {
        position: 'relative',
        width: 'min(460px, 94vw)',
        height: 'min(560px, 86vh)',
        borderRadius: 26,
        overflow: 'hidden',
        display: 'flex',
        flexDirection: 'column',
      }

  const renderEmptyMessage = (): string => {
    if (debouncedQuery) return '没有找到匹配的商品'
    if (role === 'other') return '对方暂无在售商品'
    return '暂无在售商品'
  }

  const renderTab = (value: Role, label: string) => {
    const active = role === value
    // 截图样式：选中的 tab 文字加粗 + 文字底下一条浅黄高亮条，未选中灰色
    return (
      <button
        key={value}
        type="button"
        onClick={() => setRole(value)}
        style={{
          position: 'relative',
          padding: '4px 2px',
          fontSize: active ? 17 : 15,
          fontWeight: active ? 700 : 500,
          color: active ? 'var(--text-main)' : 'var(--text-muted)',
          background: active
            ? 'linear-gradient(180deg, transparent 58%, color-mix(in srgb, var(--primary) 18%, transparent) 58%, color-mix(in srgb, var(--primary) 18%, transparent) 90%, transparent 90%)'
            : 'transparent',
          border: 'none',
          cursor: 'pointer',
          transition: 'all 0.15s ease',
        }}
      >
        {label}
      </button>
    )
  }

  // Skeleton: list rows matching the real layout (image + 2-line meta + send btn)
  const renderSkeleton = () => (
    <>
      {Array.from({ length: 5 }).map((_, idx) => (
        <div
          key={idx}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 12,
            padding: '12px 16px',
            borderBottom: '1px solid var(--surface-soft)',
          }}
        >
          <div
            style={{
              width: 72,
              height: 72,
              flexShrink: 0,
              borderRadius: 8,
              background: 'linear-gradient(90deg, var(--surface-soft) 25%, rgba(255,255,255,0.74) 50%, var(--surface-soft) 75%)',
              backgroundSize: '200% 100%',
              animation: 'itemPickerShimmer 1.4s ease-in-out infinite',
            }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ height: 14, borderRadius: 4, background: 'var(--surface-soft)', marginBottom: 8 }} />
            <div style={{ height: 14, width: '40%', borderRadius: 4, background: 'var(--surface-soft)' }} />
          </div>
          <div style={{ width: 56, height: 28, borderRadius: 999, background: 'var(--surface-soft)', flexShrink: 0 }} />
        </div>
      ))}
    </>
  )

  const content = (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        background: isMobile
          ? 'var(--bg-page)'
          : 'radial-gradient(circle at 50% 18%, rgba(255,255,255,0.18), transparent 34%), rgba(17,24,39,0.32)',
        backdropFilter: isMobile ? undefined : 'saturate(150%) blur(12px)',
        WebkitBackdropFilter: isMobile ? undefined : 'saturate(150%) blur(12px)',
        zIndex: 2147483647,
        display: 'flex',
        alignItems: isMobile ? 'flex-end' : 'center',
        justifyContent: 'center',
        padding: isMobile ? '0' : 16,
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget && !isMobile) onClose()
      }}
    >
      <style>{`@keyframes itemPickerShimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }`}</style>
      <div
        className={isMobile ? 'liquid-sheet' : 'liquid-panel'}
        style={{
          ...panelStyle,
          background: isMobile ? 'var(--surface-card-strong)' : undefined,
          color: 'var(--text-main)',
        }}
      >
        {/* Header — tab 切换 + 关闭 */}
        <div
          style={{
            padding: '14px 16px',
            minHeight: 56,
            borderBottom: '1px solid var(--control-border)',
            display: 'flex',
            alignItems: 'center',
            gap: 16,
          }}
        >
          {renderTab('other', 'TA的宝贝')}
          {renderTab('own', '我的宝贝')}
          <div style={{ flex: 1 }} />
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            style={{
              background: 'transparent',
              border: 'none',
              fontSize: 20,
              color: 'var(--text-muted)',
              cursor: 'pointer',
              padding: 4,
              lineHeight: 1,
            }}
          >
            ✕
          </button>
        </div>

        {/* Search */}
        <div style={{ padding: '10px 16px', borderBottom: '1px solid var(--surface-soft)' }}>
          <div
            className="glass-control"
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              borderRadius: 12,
              padding: '8px 12px',
            }}
          >
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={role === 'own' ? '搜索我在卖的宝贝' : '搜索TA在卖的宝贝'}
              style={{
                flex: 1,
                border: 'none',
                outline: 'none',
                background: 'transparent',
                fontSize: 13,
                color: 'var(--text-main)',
              }}
            />
            <Search size={14} style={{ color: 'var(--text-muted)' }} />
          </div>
        </div>

        {/* List — 一行一个商品，图 + 标题 + 红色价格 + 发送按钮 */}
        <div style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
          {loading ? (
            renderSkeleton()
          ) : error ? (
            <div style={{ textAlign: 'center', padding: 40, color: 'var(--danger)', fontSize: 13 }}>
              <div>加载失败 — {error}</div>
              <button
                type="button"
                onClick={() => setRetryKey((value) => value + 1)}
                style={{
                  marginTop: 14,
                  padding: '8px 18px',
                  borderRadius: 999,
                  border: '1px solid var(--control-border)',
                  background: 'var(--surface-control)',
                  color: 'var(--text-main)',
                  fontSize: 13,
                  fontWeight: 600,
                  cursor: 'pointer',
                }}
              >
                重新加载
              </button>
            </div>
          ) : visible.length === 0 ? (
            <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-muted)', fontSize: 13 }}>
              {renderEmptyMessage()}
            </div>
          ) : (
            <>
              {visible.map((it) => {
                const priceLabel = stripPriceSymbol(it.price)
                const safeTitle = isSensitiveHidden ? maskSensitiveText('item-title', it.title) : it.title
                return (
                  <div
                    key={it.itemId}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 12,
                      padding: '12px 16px',
                      borderBottom: '1px solid var(--surface-soft)',
                      background: 'transparent',
                    }}
                  >
                    <div
                      style={{
                        width: 72,
                        height: 72,
                        flexShrink: 0,
                        borderRadius: 8,
                        background: 'var(--surface-soft)',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        overflow: 'hidden',
                      }}
                    >
                      {it.mainPic ? (
                        <SafeRemoteImage
                          src={it.mainPic}
                          alt={safeTitle}
                          style={{ width: '100%', height: '100%', objectFit: 'cover' }}
                          sensitiveItem
                          fallback={<Package size={24} style={{ color: 'var(--text-tertiary)' }} />}
                        />
                      ) : (
                        <Package size={24} style={{ color: 'var(--text-tertiary)' }} />
                      )}
                    </div>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div
                        style={{
                          fontSize: 14,
                          lineHeight: 1.4,
                          color: 'var(--text-main)',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                          marginBottom: 6,
                        }}
                      >
                        <SensitiveValue kind="item-title" value={it.title} />
                      </div>
                      {priceLabel && (
                        <div style={{ fontSize: 15, fontWeight: 700, color: 'var(--danger)' }}>
                          ¥<SensitiveValue kind="money" value={priceLabel} />
                        </div>
                      )}
                    </div>
                    <button
                      type="button"
                      onClick={() => onPick(it)}
                      aria-label={`发送 ${safeTitle}`}
                      style={{
                        padding: '7px 18px',
                        borderRadius: 999,
                        border: '1px solid var(--control-border)',
                        background: 'var(--surface-control)',
                        color: 'var(--text-main)',
                        fontSize: 13,
                        fontWeight: 500,
                        cursor: 'pointer',
                        flexShrink: 0,
                      }}
                      onMouseEnter={(e) => {
                        (e.currentTarget as HTMLElement).style.background = 'var(--primary)'
                        ;(e.currentTarget as HTMLElement).style.color = 'var(--liquid-highlight)'
                        ;(e.currentTarget as HTMLElement).style.borderColor = 'var(--primary)'
                      }}
                      onMouseLeave={(e) => {
                        (e.currentTarget as HTMLElement).style.background = 'var(--surface-control)'
                        ;(e.currentTarget as HTMLElement).style.color = 'var(--text-main)'
                        ;(e.currentTarget as HTMLElement).style.borderColor = 'var(--control-border)'
                      }}
                    >
                      发送
                    </button>
                  </div>
                )
              })}
              <div style={{ textAlign: 'center', padding: '14px 0', color: 'var(--text-tertiary)', fontSize: 12 }}>
                哎呀，到底啦~
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  )

  return createPortal(content, document.body)
}
