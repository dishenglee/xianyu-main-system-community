// @ts-nocheck
// 5-icon strip + supporting popovers/modals for the message composer.
// Used by both the desktop `ConversationPanel` and the mobile `MobileChat`.
//
// Layout (left-to-right): emoji / image / item / location.
// Each callback returns a Promise<void> (or void) — the strip owns no send
// state, just the trigger UX (popovers, modals, location confirm, toasts).
import { useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ImagePlus, MapPin, ShoppingBag, Smile } from 'lucide-react'

import { useLocationPicker } from '../../components/LocationPicker'
import { EmojiPanel } from './EmojiPanel'
import { ItemPickerModal } from './ItemPickerModal'

type Variant = 'desktop' | 'mobile'

interface ComposerActionStripProps {
  variant: Variant
  uid: string
  // sessionId is required by the item-card picker — the upstream
  // `mtop.taobao.idlemessage.pc.tool.item.search` API uses it to resolve who
  // the peer is when `searchItemRole=other`. Empty disables the picker.
  sessionId?: string
  disabled?: boolean
  imageInputRef: React.RefObject<HTMLInputElement>
  onImagePick: (e: React.ChangeEvent<HTMLInputElement>) => void
  onInsertEmoji: (emoji: string) => void
  onSendItem: (item: { itemId: string; title: string; mainPic: string; price: string }) => Promise<void> | void
  // POI-shaped payload — caller (ComposerActionStrip) has already resolved
  // GPS → AMap nearby POIs → user pick before calling this. Lat/lng are
  // strings so the backend can wire-encode them verbatim.
  onSendLocation: (poi: {
    latitude: string
    longitude: string
    title: string
    content: string
  }) => Promise<void> | void
}

// Simple toast we render in a portal so it floats above the conversation
// panel's overflow:hidden clip. Two-line max, auto-dismisses.
interface ToastSpec {
  id: number
  text: string
  tone: 'success' | 'error' | 'info'
}

function Toast({ toasts }: { toasts: ToastSpec[] }) {
  if (!toasts.length) return null
  return createPortal(
    <div
      style={{
        position: 'fixed',
        top: 24,
        left: 0,
        right: 0,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 8,
        pointerEvents: 'none',
        zIndex: 2147483647,
      }}
    >
      {toasts.map((t) => (
        <div
          className="liquid-panel"
          key={t.id}
          style={{
            borderColor:
              t.tone === 'success' ? 'rgba(52,199,89,0.24)'
                : t.tone === 'error' ? 'rgba(255,59,48,0.24)'
                  : 'color-mix(in srgb, var(--primary) 22%, transparent)',
            color:
              t.tone === 'success' ? 'var(--status-success-text)'
                : t.tone === 'error' ? 'var(--status-danger-text)'
                  : 'var(--primary)',
            fontSize: 13,
            fontWeight: 500,
            padding: '8px 16px',
            borderRadius: 999,
            maxWidth: '80vw',
            textAlign: 'center',
          }}
        >
          {t.text}
        </div>
      ))}
    </div>,
    document.body,
  )
}

export function ComposerActionStrip({
  variant,
  uid,
  sessionId,
  disabled,
  imageInputRef,
  onImagePick,
  onInsertEmoji,
  onSendItem,
  onSendLocation,
}: ComposerActionStripProps) {
  const [emojiOpen, setEmojiOpen] = useState(false)
  const [emojiAnchor, setEmojiAnchor] = useState<{ left: number; bottom: number } | null>(null)
  const [itemPickerOpen, setItemPickerOpen] = useState(false)
  // 全局 LocationPicker —— GPS 取位 + 高德 POI nearby/inputtips + 选择 UI
  // 都在 components/LocationPicker.tsx，本组件只负责调 `open()` 然后拿结果发出去。
  const { open: openLocationPicker, picker: locationPickerNode } = useLocationPicker()
  const [toasts, setToasts] = useState<ToastSpec[]>([])
  const toastIdRef = useRef(0)

  function pushToast(text: string, tone: ToastSpec['tone'] = 'success') {
    const id = ++toastIdRef.current
    setToasts((prev) => [...prev, { id, text, tone }])
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id))
    }, 2500)
  }

  const emojiBtnRef = useRef<HTMLButtonElement | null>(null)
  function openEmoji() {
    if (variant === 'desktop' && emojiBtnRef.current) {
      const r = emojiBtnRef.current.getBoundingClientRect()
      setEmojiAnchor({ left: r.left, bottom: window.innerHeight - r.top + 6 })
    }
    setEmojiOpen(true)
  }

  function pickEmoji(emoji: string) {
    onInsertEmoji(emoji)
    if (variant === 'desktop') {
      setEmojiOpen(false)
    }
    // 在 mobile 上保留面板让用户连选多次表情；点 ✕ 或背景关闭。
  }

  function openItemPicker() {
    if (!uid) {
      pushToast('请先选择会话', 'error')
      return
    }
    setItemPickerOpen(true)
  }

  // Click 📍 → 全局 LocationPicker（GPS + AMap）→ 用户选 POI → 发出去。
  // GPS 失败 / 用户取消都在 picker 内部处理，回来 null 我们就当无操作。
  async function requestLocation() {
    const poi = await openLocationPicker()
    if (!poi) return
    try {
      await onSendLocation({
        latitude: poi.latitude,
        longitude: poi.longitude,
        title: poi.name,
        content: poi.address,
      })
      pushToast('发送成功', 'success')
    } catch (e: any) {
      pushToast(`发送失败：${e?.message || e || '未知错误'}`, 'error')
    }
  }

  async function pickItem(it: { itemId: string; title: string; mainPic: string; price: string }) {
    setItemPickerOpen(false)
    try {
      await onSendItem({
        itemId: it.itemId,
        title: it.title,
        // ItemPickerModal already returns the upstream `mainPic` field.
        // Carry it through verbatim — `liveSendItem` forwards it as the
        // itemCard's `mainPic` so the bubble preview lines up with what the
        // peer sees server-side.
        mainPic: it.mainPic,
        price: it.price,
      })
      pushToast('发送成功', 'success')
    } catch (e: any) {
      pushToast(`发送失败：${e?.message || e || '未知错误'}`, 'error')
    }
  }

  const btnBase: React.CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 30,
    height: 30,
    borderRadius: 10,
    background: 'transparent',
    border: 'none',
    cursor: disabled ? 'not-allowed' : 'pointer',
    color: 'var(--text-secondary)',
    flex: '0 0 auto',
  }

  const isMobile = variant === 'mobile'
  const stripStyle: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: isMobile ? 14 : 4,
    padding: isMobile ? '14px 16px 16px' : '1px 2px',
    overflowX: isMobile ? 'visible' : 'visible',
    flexWrap: isMobile ? 'wrap' : 'nowrap',
    justifyContent: isMobile ? 'flex-start' : 'flex-start',
    scrollbarWidth: 'none',
    borderRadius: isMobile ? 0 : 0,
    background: isMobile ? 'transparent' : undefined,
    border: 'none',
    boxShadow: 'none',
  }

  const mobileBtnStyle = isMobile
    ? {
      ...btnBase,
      width: 54,
      height: 54,
      borderRadius: 19,
      color: 'var(--text-main)',
      background:
        'linear-gradient(180deg, rgba(255,255,255,0.84), rgba(241,246,253,0.66))',
      border: '1px solid rgba(255,255,255,0.72)',
      boxShadow:
        '0 10px 24px rgba(25,35,55,0.10), inset 0 1px 0 rgba(255,255,255,0.78)',
      backdropFilter: 'saturate(170%) blur(18px)',
      WebkitBackdropFilter: 'saturate(170%) blur(18px)',
    }
    : btnBase

  function renderMobileTool(
    key: string,
    label: string,
    icon: React.ReactNode,
    onClick: () => void,
    disabledValue = disabled,
  ) {
    return (
      <div key={key} className="flex w-[64px] flex-col items-center gap-2">
        <button
          type="button"
          onClick={onClick}
          disabled={disabledValue}
          aria-label={label}
          title={label}
          style={mobileBtnStyle}
        >
          {icon}
        </button>
        <span className="text-center text-[12px] font-medium leading-none" style={{ color: 'var(--text-secondary)' }}>{label}</span>
      </div>
    )
  }

  if (isMobile) {
    return (
      <>
        <div style={stripStyle}>
          {renderMobileTool('image', '发送图片', <ImagePlus size={25} />, () => imageInputRef.current?.click())}
          {renderMobileTool('location', '发送位置', <MapPin size={26} />, requestLocation)}
          {renderMobileTool('item', '发送宝贝', <ShoppingBag size={26} />, openItemPicker)}
        </div>
        <input
          ref={imageInputRef}
          type="file"
          accept="image/*"
          style={{ display: 'none' }}
          onChange={(e) => {
            onImagePick(e)
            pushToast('图片发送中…', 'info')
          }}
        />

        <EmojiPanel
          open={emojiOpen}
          variant={variant}
          anchor={emojiAnchor}
          onPick={pickEmoji}
          onClose={() => setEmojiOpen(false)}
        />

        <ItemPickerModal
          open={itemPickerOpen}
          uid={uid}
          sessionId={sessionId || ''}
          variant={variant}
          onClose={() => setItemPickerOpen(false)}
          onPick={pickItem}
        />

        {locationPickerNode}
        <Toast toasts={toasts} />
      </>
    )
  }

  return (
    <>
      <div style={stripStyle}>
        <button
          ref={emojiBtnRef}
          type="button"
          onClick={openEmoji}
          disabled={disabled}
          aria-label="表情"
          title="表情"
          style={mobileBtnStyle}
          onMouseEnter={(e) => {
            if (!disabled && !isMobile) (e.currentTarget as HTMLElement).style.background = 'var(--surface-soft)'
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLElement).style.background = isMobile ? 'var(--primary-light)' : 'transparent'
          }}
        >
          <Smile size={isMobile ? 17 : 17} />
        </button>

        <button
          type="button"
          onClick={() => imageInputRef.current?.click()}
          disabled={disabled}
          aria-label="图片"
          title="图片"
          style={mobileBtnStyle}
          onMouseEnter={(e) => {
            if (!disabled && !isMobile) (e.currentTarget as HTMLElement).style.background = 'var(--surface-soft)'
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLElement).style.background = isMobile ? 'var(--primary-light)' : 'transparent'
          }}
        >
          <ImagePlus size={isMobile ? 17 : 17} />
        </button>
        <input
          ref={imageInputRef}
          type="file"
          accept="image/*"
          style={{ display: 'none' }}
          onChange={(e) => {
            onImagePick(e)
            // Tiny ack — handleImagePick handles real upload errors via console.
            pushToast('图片发送中…', 'info')
          }}
        />

        <button
          type="button"
          onClick={openItemPicker}
          disabled={disabled}
          aria-label="发送商品"
          title="发送商品"
          style={mobileBtnStyle}
          onMouseEnter={(e) => {
            if (!disabled && !isMobile) (e.currentTarget as HTMLElement).style.background = 'var(--surface-soft)'
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLElement).style.background = isMobile ? 'var(--primary-light)' : 'transparent'
          }}
        >
          <ShoppingBag size={isMobile ? 17 : 17} />
        </button>

        <button
          type="button"
          onClick={requestLocation}
          disabled={disabled}
          aria-label="发送位置"
          title="发送位置"
          style={mobileBtnStyle}
          onMouseEnter={(e) => {
            if (!disabled && !isMobile) (e.currentTarget as HTMLElement).style.background = 'var(--surface-soft)'
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLElement).style.background = isMobile ? 'var(--primary-light)' : 'transparent'
          }}
        >
          <MapPin size={isMobile ? 17 : 17} />
        </button>
      </div>

      <EmojiPanel
        open={emojiOpen}
        variant={variant}
        anchor={emojiAnchor}
        onPick={pickEmoji}
        onClose={() => setEmojiOpen(false)}
      />

      <ItemPickerModal
        open={itemPickerOpen}
        uid={uid}
        sessionId={sessionId || ''}
        variant={variant}
        onClose={() => setItemPickerOpen(false)}
        onPick={pickItem}
      />

      {locationPickerNode}

      <Toast toasts={toasts} />
    </>
  )
}
