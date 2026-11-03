// @ts-nocheck
// 闲鱼 emoji picker. Each tile shows the real 闲鱼 CDN icon for codes present
// in XIANYU_EMOJI_MAP; clicking inserts the original 闲鱼 bracket code so the
// sender, receiver, and local bubble all render the same custom image.
//
// Two layouts: `desktop` renders a small floating popover anchored above
// the composer's emoji button; `mobile` renders a half-height bottom sheet
// that slides up from the bottom of the viewport. Both call `onPick` with
// the chosen bracket string so the parent can insert it at the textarea's
// cursor.
import { useEffect, useRef } from 'react'

import { XIANYU_EMOJI_MAP, xianyuEmojiSendText } from './xianyuEmojis'
import { SafeRemoteImage } from './SafeRemoteImage'

// Curated picker order — popular/expressive codes first, then alphabetical
// for the long tail. Codes that aren't in XIANYU_EMOJI_MAP render as a
// text-pill fallback so this list stays robust if the map shrinks.
const PICKER_CODES: string[] = [
  // top row — most expressive / common
  '偷笑', '微笑', '大哭', '流泪', '破涕为笑', '憨笑', '捂脸哭', '愉快',
  '调皮', '坏笑', '害羞', '飞吻', '色', '得意', '机智', '比心',
  // middle — reactions
  '惊讶', '惊慌', '疑问', '思考', '费解', '暗中观察', '谁懂啊', '盯',
  '白眼', '撇嘴', '无聊', '呆', '困', '睡', '尴尬', '托腮',
  '抠鼻', '挥手', '拜', '拜托', '握手', '拥抱', '摊手', '拒绝',
  // emotional / strong
  '感动', '感谢', '委屈', '难过', '叹气', '滴泪', '吐', '晕',
  '方了', '裂开', '衰', '小丑', '佛系', '酸了', '激推', '鼓掌',
  '拍手', '棒', '真棒', '666',
  // hands / actions
  '打call', '打包', '握手', '举手', '举杯', '拉钩', '搬砖', '蹲',
  // money / shopping
  '数钱', '没钱', '吃土', '红包', '送花', '递钱', '招财猫', '快递',
  '包邮', '超便宜', '小刀', '不刀', '同出', '种草', '求赞', '白菜',
  // cute / animals
  '鸭鸭', '吗喽', '菜狗', '旺财', '桃子', '啤酒', '爱你', '爱心',
  '飞吻', '尊嘟假嘟', '吃瓜', '咒骂', '嘘', '嘿哈', '哇哦', '耶',
  '号外',
  // misc
  '流汗', '流鼻血', '发怒', '奋斗', '好喜欢', '期待', '开心到模糊',
  '开心面具', '非主流', '鸡哔你', '功德+1', '沙发', '排', '跪了',
  '瑞思拜', '可', '南', '再见', '不好意思', '秃头', '龇牙',
  // english / brand
  'ok', 'cool', 'hot', 'new', 'yeah', '1', 'city不city啊',
]

// Dedupe while preserving order — the curated list above intentionally
// repeats a couple of codes between sections; useState keys must be unique.
const SEEN = new Set<string>()
const PICKER_CODES_DEDUPED: string[] = PICKER_CODES.filter((c) => {
  if (SEEN.has(c)) return false
  SEEN.add(c)
  return true
})

interface EmojiPanelProps {
  open: boolean
  onClose: () => void
  onPick: (emoji: string) => void
  variant?: 'desktop' | 'mobile'
  inline?: boolean
  /** Anchor coordinates for desktop popover (bottom-left of the trigger button). */
  anchor?: { left: number; bottom: number } | null
}

// Each tile: 闲鱼 CDN image when available, otherwise a styled bracket-text
// pill. Click inserts the original bracket shortcode into the composer state.
function EmojiTile({ code, size, onPick }: { code: string; size: number; onPick: (s: string) => void }) {
  const url = XIANYU_EMOJI_MAP[code]
  return (
    <button
      type="button"
      onClick={() => onPick(xianyuEmojiSendText(code))}
      title={`[${code}]`}
      aria-label={`[${code}]`}
      style={{
        padding: 4,
        border: 'none',
        background: 'transparent',
        cursor: 'pointer',
        borderRadius: 8,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        minHeight: size + 8,
      }}
      onMouseEnter={(e) => {
        ;(e.currentTarget as HTMLElement).style.background = 'rgba(255, 255, 255, 0.64)'
      }}
      onMouseLeave={(e) => {
        ;(e.currentTarget as HTMLElement).style.background = 'transparent'
      }}
    >
      {url ? (
        <SafeRemoteImage
          src={url}
          alt={`[${code}]`}
          style={{ width: size, height: size, objectFit: 'contain', display: 'block' }}
          draggable={false}
          fallback={(
            <span
              style={{
                display: 'inline-block',
                background: 'var(--surface-soft)',
                border: '1px solid var(--control-border)',
                borderRadius: 6,
                padding: '2px 6px',
                fontSize: 12,
                color: 'var(--text-main)',
                whiteSpace: 'nowrap',
                lineHeight: 1.2,
              }}
            >
              [{code}]
            </span>
          )}
        />
      ) : (
        <span
          style={{
            display: 'inline-block',
            background: 'var(--surface-soft)',
            border: '1px solid var(--control-border)',
            borderRadius: 6,
            padding: '2px 6px',
            fontSize: 12,
            color: 'var(--text-main)',
            whiteSpace: 'nowrap',
            lineHeight: 1.2,
          }}
        >
          [{code}]
        </span>
      )}
    </button>
  )
}

export function EmojiPanel({ open, onClose, onPick, variant = 'desktop', inline = false, anchor }: EmojiPanelProps) {
  // Close on click-outside (desktop popover) — the bottom sheet uses an
  // explicit backdrop button so this listener doesn't fight the sheet's tap.
  const ref = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    if (!open || variant !== 'desktop') return
    function onDoc(e: MouseEvent) {
      if (!ref.current) return
      if (!ref.current.contains(e.target as Node)) onClose()
    }
    // Defer so the click that opened the panel doesn't immediately close it.
    const t = window.setTimeout(() => document.addEventListener('mousedown', onDoc), 0)
    return () => {
      window.clearTimeout(t)
      document.removeEventListener('mousedown', onDoc)
    }
  }, [open, variant, onClose])

  if (!open) return null

  if (variant === 'mobile') {
    if (inline) {
      return (
        <div
          ref={ref}
          className="mt-3 rounded-[22px] border"
          style={{
            background: 'rgba(255,255,255,0.72)',
            borderColor: 'rgba(255,255,255,0.72)',
            boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.78)',
            padding: '10px 10px 12px',
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              marginBottom: 8,
            }}
          >
            <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-main)' }}>选择表情</span>
            <button
              type="button"
              onClick={onClose}
              style={{
                background: 'transparent',
                border: 'none',
                color: 'var(--text-muted)',
                fontSize: 16,
                cursor: 'pointer',
                padding: 4,
              }}
              aria-label="关闭"
            >
              ✕
            </button>
          </div>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(7, 1fr)',
              gap: 4,
              maxHeight: 'min(210px, 30dvh)',
              overflowY: 'auto',
              paddingRight: 2,
              scrollbarWidth: 'none',
            }}
          >
            {PICKER_CODES_DEDUPED.map((code) => (
              <EmojiTile key={code} code={code} size={30} onPick={onPick} />
            ))}
          </div>
        </div>
      )
    }

    return (
      <div
        style={{
          position: 'fixed',
          inset: 0,
          zIndex: 2147483647,
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <button
          type="button"
          aria-label="关闭表情面板"
          onClick={onClose}
          style={{
            flex: 1,
            background:
              'radial-gradient(circle at 50% 18%, rgba(255,255,255,0.18), transparent 34%), rgba(17,24,39,0.32)',
            backdropFilter: 'saturate(150%) blur(12px)',
            WebkitBackdropFilter: 'saturate(150%) blur(12px)',
            border: 'none',
            padding: 0,
            cursor: 'pointer',
          }}
        />
        <div
          ref={ref}
          className="liquid-sheet"
          style={{
            borderTopLeftRadius: 32,
            borderTopRightRadius: 32,
            padding: '14px 14px calc(14px + env(safe-area-inset-bottom))',
            maxHeight: '36dvh',
            overflowY: 'auto',
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              marginBottom: 10,
            }}
          >
            <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-main)' }}>选择表情</span>
            <button
              type="button"
              onClick={onClose}
              style={{
                background: 'transparent',
                border: 'none',
                color: 'var(--text-muted)',
                fontSize: 18,
                cursor: 'pointer',
                padding: 4,
              }}
              aria-label="关闭"
            >
              ✕
            </button>
          </div>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(7, 1fr)',
              gap: 4,
            }}
          >
            {PICKER_CODES_DEDUPED.map((code) => (
              <EmojiTile key={code} code={code} size={32} onPick={onPick} />
            ))}
          </div>
        </div>
      </div>
    )
  }

  // Desktop popover — anchored above the trigger button. Bottom of popover
  // sits at `anchor.bottom`, left at `anchor.left`.
  const left = anchor?.left ?? 24
  const bottom = anchor?.bottom ?? 80
  return (
    <div
      ref={ref}
      role="dialog"
      aria-label="表情面板"
      className="liquid-panel"
      style={{
        position: 'fixed',
        left,
        bottom,
        width: 340,
        maxHeight: 320,
        overflowY: 'auto',
        borderRadius: 22,
        padding: 10,
        zIndex: 2147483647,
      }}
    >
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(8, 1fr)',
          gap: 2,
        }}
      >
        {PICKER_CODES_DEDUPED.map((code) => (
          <EmojiTile key={code} code={code} size={28} onPick={onPick} />
        ))}
      </div>
    </div>
  )
}
