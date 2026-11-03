// @ts-nocheck
import { Heart, MapPin, Mic, Package, Phone, Sparkles } from 'lucide-react'

import { sanitizeMessageHtml } from './sanitizeHtml'
import { BRAND, resolveMessageImage, sessionType } from './messagesConstants'
import { XIANYU_EMOJI_MAP } from './xianyuEmojis'
import { SafeRemoteImage } from './SafeRemoteImage'
import { SensitiveValue, useSensitiveText } from '../../components/SensitiveValue'

// Render bracket tokens like `[撇嘴]` / `[送花]` with the real 闲鱼 emoji
// image. Mapping table auto-generated from the captured mtop face.emoji.load
// response (see xianyuEmojis.ts for provenance). Unknown tokens fall back
// to a subtle `[xxx]` chip. Voice/rtc tokens still get an icon chip.
function tokenSpecialStyle(token: string): { icon: any; hint: string } | null {
  if (token.startsWith('语音聊天') || token.startsWith('语音通话')) {
    return { icon: Phone, hint: token }
  }
  if (token.startsWith('语音')) return { icon: Mic, hint: token } // [语音 3″] / [语音消息]
  if (token === '动态消息') return { icon: Sparkles, hint: token }
  return null
}

export function renderRichText(raw: string) {
  const pattern = /\[([^\[\]\n]{1,16})\]/g
  const parts: any[] = []
  let lastIndex = 0
  let match: RegExpExecArray | null
  let idx = 0
  while ((match = pattern.exec(raw)) !== null) {
    if (match.index > lastIndex) {
      parts.push(raw.slice(lastIndex, match.index))
    }
    const token = match[1]
    const emojiUrl = XIANYU_EMOJI_MAP[token]
    if (emojiUrl) {
      parts.push(
        <SafeRemoteImage
          key={`xe-${idx}`}
          src={emojiUrl}
          alt={`[${token}]`}
          title={token}
          className="inline-block h-5 w-5 align-middle"
          style={{ verticalAlign: '-4px' }}
          draggable={false}
          fallback={(
            <span
              className="mx-0.5 inline-block rounded px-1 align-middle text-[12px]"
              style={{ backgroundColor: 'var(--surface-soft)', color: BRAND.textSecondary }}
            >
              [{token}]
            </span>
          )}
        />,
      )
    } else {
      const special = tokenSpecialStyle(token)
      if (special) {
        const Icon = special.icon
        parts.push(
          <span
            key={`sp-${idx}`}
            className="mx-0.5 inline-flex items-center gap-1 rounded-full border px-2 py-0.5 align-middle text-[12px]"
            style={{ backgroundColor: 'var(--surface-soft)', borderColor: BRAND.border, color: BRAND.textSecondary }}
          >
            <Icon className="h-3 w-3" />
            {special.hint}
          </span>,
        )
      } else {
        // Unknown token — keep the `[xxx]` text verbatim but give it a subtle
        // chip styling so it's visually distinct from surrounding prose.
        parts.push(
          <span
            key={`tk-${idx}`}
            className="mx-0.5 inline-block rounded px-1 align-middle text-[12px]"
            style={{ backgroundColor: 'var(--surface-soft)', color: BRAND.textSecondary }}
          >
            [{token}]
          </span>,
        )
      }
    }
    lastIndex = match.index + match[0].length
    idx += 1
  }
  if (lastIndex < raw.length) parts.push(raw.slice(lastIndex))
  if (!parts.length) return raw
  return <>{parts}</>
}

// Item card — `contentType === 7` (我想要 / 我要咨询) or 8 (image-led item
// card). Platform sends the rich `itemCard` payload via cardMeta:
//   { kind: 'item', title, mainPic, price, itemTip }
// Render as a small horizontal card with light-grey background, thumbnail
// (left), title (1-2 lines), and a red ¥ price (bottom-right). Falls back to
// legacy `title` / `imageUrl` / `text` for older messages that don't have
// cardMeta attached.
export function isProductCardMessage(message): boolean {
  const ct = Number(message?.contentType ?? message?.content_type ?? 0)
  if (ct === 7 || ct === 8) return true
  const meta = message?.cardMeta
  if (meta && typeof meta === 'object' && meta?.kind === 'item') return true
  return false
}

export function renderProductCard(message) {
  return <ProductCard message={message} />
}

function ProductCard({ message }) {
  const meta = (message && typeof message.cardMeta === 'object' && message.cardMeta !== null)
    ? message.cardMeta
    : null

  // cardMeta takes priority over the legacy flat fields — the new event
  // parser fills it from the upstream itemCard payload.
  const title = String(meta?.title ?? message?.title ?? '').trim()
  const pic = String(meta?.mainPic ?? message?.imageUrl ?? message?.image_url ?? '').trim()
  const tip = String(meta?.itemTip ?? '').trim()
  const priceRaw = String(meta?.price ?? '').trim()

  // Older path: text was "action\nprice". Split it as a fallback when
  // cardMeta isn't attached (history rows pre-dating the parser update).
  const raw = String(message?.text ?? message?.content ?? '').trim()
  const lines = raw.split(/\n+/).map((s) => s.trim()).filter(Boolean)
  const fallbackAction = !priceRaw && lines[0] && !/^\d/.test(lines[0]) ? lines[0] : ''
  const fallbackPrice = !priceRaw && lines[1] ? lines[1] : ''

  const priceText = priceRaw || fallbackPrice
  const priceDisplay = priceText
    ? (/^\d+(\.\d+)?$/.test(priceText) ? `¥${priceText}` : (priceText.startsWith('¥') ? priceText : `¥${priceText}`))
    : ''
  const subtitle = tip || fallbackAction
  const safeItemLabel = useSensitiveText('item-title', title || '商品')

  return (
    <div
      className="flex w-[260px] gap-3 rounded-xl p-2"
      style={{ backgroundColor: 'var(--surface-soft)' }}
    >
      {pic ? (
        <SafeRemoteImage
          src={pic}
          alt={safeItemLabel}
          sensitiveItem
          className="h-24 w-24 shrink-0 rounded-lg object-cover"
          style={{ borderColor: BRAND.border }}
          fallback={(
            <div
              className="flex h-24 w-24 shrink-0 items-center justify-center rounded-lg"
              style={{ backgroundColor: BRAND.selected, color: BRAND.accent }}
            >
              <Package className="h-6 w-6" />
            </div>
          )}
        />
      ) : (
        <div
          className="flex h-24 w-24 shrink-0 items-center justify-center rounded-lg"
          style={{ backgroundColor: BRAND.selected, color: BRAND.accent }}
        >
          <Package className="h-6 w-6" />
        </div>
      )}
      <div className="flex min-w-0 flex-1 flex-col justify-between gap-1 py-1">
        <div
          className="line-clamp-2 text-[13px] font-medium leading-5"
          style={{ color: BRAND.textMain }}
        >
          <SensitiveValue kind="item-title" value={title || '关联商品'} />
        </div>
        {subtitle ? (
          <div className="truncate text-[11px]" style={{ color: BRAND.textSecondary }}>
            {subtitle}
          </div>
        ) : null}
        {priceDisplay ? (
          <div className="flex justify-end">
            <span className="text-[15px] font-semibold" style={{ color: BRAND.accent }}>
              <SensitiveValue kind="money" value={priceDisplay} />
            </span>
          </div>
        ) : null}
      </div>
    </div>
  )
}

// Location card — `contentType === 30`. Platform sends a `locationCard`
// payload with title, content (subtitle), and a static map preview URL.
// We render it as a small horizontal card with the light-grey background,
// a square map-preview tile on the left (red pin overlay), and bold title +
// grey subtitle on the right. Click → opens the deep link.
export function isLocationCardMessage(message): boolean {
  const ct = Number(message?.contentType ?? message?.content_type ?? 0)
  if (ct === 30) return true
  const meta = message?.cardMeta
  return Boolean(meta && typeof meta === 'object' && meta?.kind === 'location')
}

export function renderLocationCard(message) {
  const meta = (message && typeof message.cardMeta === 'object' && message.cardMeta !== null)
    ? message.cardMeta
    : null

  const title = String(meta?.title ?? message?.title ?? '').trim() || '位置'
  const subtitle = String(meta?.content ?? '').trim()
  const previewUrl = String(meta?.previewUrl ?? message?.imageUrl ?? message?.image_url ?? '').trim()
  const actionUrl = String(meta?.actionUrl ?? '').trim()

  const onClick = () => {
    if (actionUrl) window.open(actionUrl, '_blank', 'noopener')
  }

  return (
    <div
      role={actionUrl ? 'button' : undefined}
      onClick={onClick}
      className={`flex w-[260px] gap-3 rounded-xl p-2 ${actionUrl ? 'cursor-pointer' : ''}`}
      style={{ backgroundColor: 'var(--surface-soft)' }}
    >
      <div
        className="relative h-24 w-24 shrink-0 overflow-hidden rounded-lg"
        style={{
          backgroundColor: 'var(--surface-sunken)',
          // Subtle grid pattern so the tile reads as a map preview even
          // when no upstream static-map image is supplied.
          backgroundImage:
            'linear-gradient(rgba(255,255,255,0.42) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.36) 1px, transparent 1px)',
          backgroundSize: '16px 16px',
        }}
      >
        {previewUrl ? (
          <SafeRemoteImage
            src={previewUrl}
            alt=""
            className="absolute inset-0 h-full w-full object-cover"
            fallback={null}
          />
        ) : null}
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <MapPin
            className="h-8 w-8"
            style={{ color: BRAND.accent, filter: 'drop-shadow(0 1px 2px rgba(0,0,0,0.25))' }}
          />
        </div>
      </div>
      <div className="flex min-w-0 flex-1 flex-col justify-center gap-1 py-1">
        <div
          className="line-clamp-2 text-[14px] font-semibold leading-5"
          style={{ color: BRAND.textMain }}
        >
          {title}
        </div>
        {subtitle ? (
          <div className="truncate text-[12px]" style={{ color: BRAND.textSecondary }}>
            {subtitle}
          </div>
        ) : null}
      </div>
    </div>
  )
}

// Notification card (闲鱼"通知消息" screen, sessionType=3). Full-width
// card with bold title + body. When backend parsed a dxCard (content_type
// 25/26/28 — 交易/订单/物流), `message.cardMeta` carries the seller + item
// metadata and we render the rich 闲鱼-app layout: seller avatar + seller
// nick row + "联系卖家" pill, bold title, and an item sub-card with
// thumbnail + title + "点击查看订单详情 >".
export function renderNotificationItem(message, session) {
  return <NotificationItem message={message} session={session} />
}

function NotificationItem({ message, session }) {
  const cardMeta = (message && typeof message.cardMeta === 'object' && message.cardMeta !== null)
    ? message.cardMeta
    : null

  const title = String(message?.title ?? '').trim()
  const { html: titleHtml, text: titleText } = sanitizeMessageHtml(title)
  const rawBody = String(message?.text ?? message?.content ?? message?.preview ?? '')
  const { html: bodyHtml, text: bodyText } = sanitizeMessageHtml(rawBody)
  const pic = String(message?.imageUrl ?? message?.image_url ?? '').trim()

  const sellerAvatar = String(cardMeta?.sellerAvatar ?? '').trim()
  const sellerNickMeta = String(cardMeta?.sellerNick ?? '').trim()
  const itemTitle = String(cardMeta?.itemTitle ?? '').trim()
  const itemSubtitle = String(cardMeta?.itemSubtitle ?? '').trim()
  const itemPic = String(cardMeta?.itemPic ?? '').trim()

  const icon =
    sellerAvatar ||
    String(message?.senderLogo ?? message?.sender_logo ?? '').trim() ||
    String(session?.peerLogo ?? '').trim()
  const senderNick =
    sellerNickMeta ||
    String(message?.senderNick ?? message?.sender_nick ?? '').trim() ||
    String(session?.peerNick ?? '').trim()

  const hasRichCard = Boolean(cardMeta && (itemTitle || itemPic))
  const safeItemLabel = useSensitiveText('item-title', itemTitle || '交易商品')

  return (
    <div
      className="w-full rounded-2xl border px-4 py-3"
      style={{ backgroundColor: BRAND.card, borderColor: BRAND.border }}
    >
      {(icon || senderNick) ? (
        <div className="mb-2 flex items-center gap-2">
          {icon ? (
            <SafeRemoteImage
              src={icon}
              alt=""
              className="h-7 w-7 shrink-0 rounded-full object-cover"
              fallback={null}
            />
          ) : null}
          {senderNick ? (
            <span className="flex-1 truncate text-[13px] font-medium" style={{ color: BRAND.textMain }}>
              <SensitiveValue kind="buyer" value={senderNick} />
            </span>
          ) : <span className="flex-1" />}
          {hasRichCard ? (
            <span
              className="rounded-full border px-2.5 py-0.5 text-[11px] font-medium"
              style={{ color: BRAND.textSecondary, borderColor: BRAND.border }}
            >
              联系卖家
            </span>
          ) : null}
        </div>
      ) : null}

      {title ? (
        <div
          className="mb-1 text-[15px] font-semibold"
          style={{ color: BRAND.textMain }}
        >
          <SensitiveValue kind="order-id">
            {titleHtml ? (
              <span className="rich-msg" dangerouslySetInnerHTML={{ __html: titleHtml }} />
            ) : (
              titleText || title
            )}
          </SensitiveValue>
        </div>
      ) : null}

      {rawBody ? (
        <div
          className="text-[13px] leading-5"
          style={{ color: BRAND.textSecondary, whiteSpace: 'pre-wrap' }}
        >
          <SensitiveValue kind="order-id">
            {bodyHtml ? (
              <span className="rich-msg" dangerouslySetInnerHTML={{ __html: bodyHtml }} />
            ) : (
              <span>{renderRichText(bodyText || rawBody)}</span>
            )}
          </SensitiveValue>
        </div>
      ) : null}

      {hasRichCard ? (
        <div
          className="mt-3 flex items-center gap-3 rounded-xl border px-3 py-2.5"
          style={{ backgroundColor: BRAND.page, borderColor: BRAND.border }}
        >
          {itemPic ? (
            <SafeRemoteImage
              src={itemPic}
              alt={safeItemLabel}
              className="h-14 w-14 shrink-0 rounded-lg object-cover"
              sensitiveItem
              fallback={null}
            />
          ) : null}
          <div className="min-w-0 flex-1">
            {itemTitle ? (
              <div className="truncate text-[13px] font-medium" style={{ color: BRAND.textMain }}>
                <SensitiveValue kind="item-title" value={itemTitle} />
              </div>
            ) : null}
            {itemSubtitle ? (
              <div className="mt-0.5 truncate text-[12px]" style={{ color: BRAND.textTertiary }}>
                <SensitiveValue kind="order-id" value={itemSubtitle} />
              </div>
            ) : null}
            <div className="mt-1 text-[11px]" style={{ color: BRAND.textTertiary }}>
              点击查看订单详情 &rsaquo;
            </div>
          </div>
        </div>
      ) : pic ? (
        <SafeRemoteImage
          src={pic}
          alt=""
          className="mt-3 max-h-64 rounded-xl border object-contain"
          style={{ borderColor: BRAND.border }}
          fallback={null}
        />
      ) : null}
    </div>
  )
}


// Interaction row (闲鱼"互动消息" screen, sessionType=6). Compact list row
// with left avatar, middle title+preview+time, right thumbnail.
export function renderInteractionItem(message, session) {
  const senderIcon =
    String(message?.senderLogo ?? message?.sender_logo ?? '').trim() ||
    String(session?.peerLogo ?? '').trim()
  const rawTitle = String(message?.title ?? '').trim()
  const { html: titleHtml, text: titleText } = sanitizeMessageHtml(rawTitle)
  const rawBody = String(message?.text ?? message?.content ?? '').trim()
  const { html: bodyHtml, text: bodyText } = sanitizeMessageHtml(rawBody)
  const pic = String(message?.imageUrl ?? message?.image_url ?? '').trim()

  return (
    <div
      className="flex w-full items-start gap-3 rounded-2xl border px-3 py-3"
      style={{ backgroundColor: BRAND.card, borderColor: BRAND.border }}
    >
      {senderIcon ? (
        <SafeRemoteImage
          src={senderIcon}
          alt=""
          className="h-10 w-10 shrink-0 rounded-full object-cover"
          fallback={(
            <div
              className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full"
              style={{ background: 'var(--surface-soft)', border: `1px solid ${BRAND.border}` }}
            >
              <Heart className="h-4 w-4" style={{ color: BRAND.textSecondary }} />
            </div>
          )}
        />
      ) : (
        <div
          className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full"
          style={{ background: 'var(--surface-soft)', border: `1px solid ${BRAND.border}` }}
        >
          <Heart className="h-4 w-4" style={{ color: BRAND.textSecondary }} />
        </div>
      )}

      <div className="min-w-0 flex-1">
        {rawTitle ? (
          <div className="truncate text-[14px] font-medium" style={{ color: BRAND.textMain }}>
            {titleHtml ? (
              <span className="rich-msg" dangerouslySetInnerHTML={{ __html: titleHtml }} />
            ) : (
              titleText || rawTitle
            )}
          </div>
        ) : null}
        {rawBody ? (
          <div
            className="mt-0.5 line-clamp-2 text-[13px]"
            style={{ color: BRAND.textSecondary }}
          >
            {bodyHtml ? (
              <span className="rich-msg" dangerouslySetInnerHTML={{ __html: bodyHtml }} />
            ) : (
              <span>{renderRichText(bodyText || rawBody)}</span>
            )}
          </div>
        ) : null}
      </div>

      {pic ? (
        <SafeRemoteImage
          src={pic}
          alt=""
          className="h-14 w-14 shrink-0 rounded-lg border object-cover"
          style={{ borderColor: BRAND.border }}
          fallback={null}
        />
      ) : null}
    </div>
  )
}

// Voice message (content_type=3) / voice-video call (content_type=18).
// 闲鱼 / the Python reference impl parse out audio.url + audio.duration,
// but neither backend actually forwards the URL to the client — we only
// have the "[语音 3″]" text placeholder. Render as a proper voice bubble
// (mic icon + duration) instead of mixing a chip into text flow.
export function isVoiceMessage(message): boolean {
  const ct = Number(message?.contentType ?? message?.content_type ?? 0)
  return ct === 3 || ct === 18
}

export function renderVoiceBubble(message) {
  const ct = Number(message?.contentType ?? message?.content_type ?? 0)
  const raw = String(message?.text ?? message?.content ?? '').trim()
  // Try to extract duration from "[语音 3″]" / "[语音 15″]" pattern.
  const durMatch = raw.match(/(\d+)\s*[″"秒]/)
  const duration = durMatch ? Number(durMatch[1]) : 0
  const isCall = ct === 18
  const Icon = isCall ? Phone : Mic
  const label = isCall
    ? (raw.replace(/^\[|\]$/g, '').trim() || '语音/视频通话')
    : (duration > 0 ? `${duration}″` : (raw.replace(/^\[|\]$/g, '').trim() || '语音消息'))

  // Width scales softly with duration so a 30″ voice is visibly longer
  // than a 2″ voice — mirrors how WeChat / 闲鱼 render voice bubbles.
  const width = isCall
    ? 160
    : Math.max(88, Math.min(220, 88 + duration * 4))

  return (
    <div
      className="inline-flex items-center gap-2"
      style={{ width, minHeight: 24 }}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span className="text-[13px]">{label}</span>
    </div>
  )
}

// Sticker / animated emoji (content_type=5). Backend stored the 闲鱼 CDN
// gif URL in imageUrl. Render as a small standalone image WITHOUT a chat
// bubble — that matches how 闲鱼 shows stickers in the real app.
export function isStickerMessage(message): boolean {
  const ct = Number(message?.contentType ?? message?.content_type ?? 0)
  if (ct !== 5) return false
  const url =
    String(message?.imageUrl ?? message?.image_url ?? '').trim() ||
    (Array.isArray(message?.imageUrls) ? String(message.imageUrls[0] ?? '').trim() : '')
  return Boolean(url)
}

export function renderStickerImage(message) {
  const url =
    String(message?.imageUrl ?? message?.image_url ?? '').trim() ||
    (Array.isArray(message?.imageUrls) ? String(message.imageUrls[0] ?? '').trim() : '')
  const name = String(message?.text ?? message?.preview ?? '').trim() || '表情'
  return (
    <SafeRemoteImage
      src={url}
      alt={name}
      title={name}
      className="h-[60px] w-[60px] object-contain"
      draggable={false}
      fallback={null}
    />
  )
}

export function renderMessageContent(message, _session) {
  if (isProductCardMessage(message)) {
    return renderProductCard(message)
  }
  if (isLocationCardMessage(message)) {
    return renderLocationCard(message)
  }
  // Voice / voice-call — render dedicated voice bubble (not a tiny chip).
  if (isVoiceMessage(message)) {
    return renderVoiceBubble(message)
  }
  // Sticker — bare image, no bubble chrome (handled at the wrapper).
  if (isStickerMessage(message)) {
    return renderStickerImage(message)
  }

  const imageUrl = resolveMessageImage(message)
  if (imageUrl) {
    return <SafeRemoteImage src={imageUrl} alt="图片消息" className="max-w-[220px] rounded-2xl" fallback={null} />
  }

  const raw = String(message?.text ?? message?.content ?? '')

  const { html, text } = sanitizeMessageHtml(raw)
  // System notifications come as HTML — keep rich rendering intact.
  if (html) return <span className="rich-msg" dangerouslySetInnerHTML={{ __html: html }} />
  // Plain text (real chat): process bracket tokens like `[撇嘴]`.
  const body = text || raw
  if (!body) return ' '
  return renderRichText(body)
}

// System-style messages render centered with a muted pill (think WeChat
// "X 撤回了一条消息" / WhatsApp "Joined the group"):
//   - Whole session is sessionType 3 (系统通知) or 6 (互动消息)
//   - Or the message kind is `tip` / `card` / `unknown` — platform injects
//     these for transaction state ("卖家已发货", "订单即将自动确认收货"),
//     platform banners ("你的闲鱼小法庭诚邀..."), survey invites etc.
// Regular user text/image messages stay as left/right bubbles.
export function isSystemStyleMessage(message, session) {
  // Item cards (contentType=7/8) are user-initiated "我想要" / "我要咨询这个
  // 商品" bubbles, location cards (contentType=30) are user "分享了一个位置"
  // bubbles — both render as normal left/right chat bubbles wrapping a rich
  // card, NOT centered system pills.
  if (isProductCardMessage(message)) return false
  if (isLocationCardMessage(message)) return false

  const st = sessionType(session)
  if (st === 3 || st === 6) return true
  const kind = String(message?.kind ?? message?.type ?? '').trim().toLowerCase()
  if (!kind) return false
  return kind !== 'text' && kind !== 'image'
}

// Pull a short headline for card-kind system messages when we have a title.
// Both `title` and `text` can carry HTML (e.g. the 请不要脱离闲鱼 banner
// ships `title='<strong><font...>请不要脱离闲鱼沟通及交易</font></strong>'`),
// so we sanitize+dangerouslySetInnerHTML for both — otherwise React escapes
// the tags and the user sees literal `<strong>` / `<font>` markup.
export function renderSystemMessageBody(message) {
  const rawTitle = String(message?.title ?? '').trim()
  const { html: titleHtml, text: titleText } = sanitizeMessageHtml(rawTitle)

  const rawBody = String(message?.text ?? message?.content ?? message?.preview ?? '')
  const { html: bodyHtml, text: bodyText } = sanitizeMessageHtml(rawBody)

  return (
    <>
      {rawTitle ? (
        <div className="mb-0.5 text-[13px] font-semibold" style={{ color: BRAND.textMain }}>
          {titleHtml ? (
            <span className="rich-msg" dangerouslySetInnerHTML={{ __html: titleHtml }} />
          ) : (
            titleText || rawTitle
          )}
        </div>
      ) : null}
      {bodyHtml ? (
        <span className="rich-msg" dangerouslySetInnerHTML={{ __html: bodyHtml }} />
      ) : (
        <span>{renderRichText(bodyText || rawBody || ' ')}</span>
      )}
    </>
  )
}
