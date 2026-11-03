// @ts-nocheck
// Mobile chat detail page — `/messages/chat/:uid/:sessionId/:sessionType`.
// This route is a sibling of MobileMessages under `MessagesRouteLayout`, so
// both consume the same `useMessagesPageState` instance via context. That
// means:
//   * sending a message here updates the list's summary/ts/unread on the
//     same render — the user pops back and sees the new conversation at the
//     top of the list with their own message as the preview;
//   * the unread badge cleared by `markSessionRead` (driven by `selectSession`
//     on mount) survives the back navigation because the hook state is
//     owned one level up;
//   * history, optimistic bubbles, recall rewrites all flow through the
//     shared `historyByKey` store so the list never sees a stale snapshot
//     after a chat-side mutation.
// Previously this component owned a private `historyStateRef` + send/recall
// closures, so its writes were invisible to the list. See the comment on
// `MessagesRouteLayout` for the routing diff.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useParams } from 'react-router-dom'
import { MoreHorizontal, Plus, SendHorizontal, Smile, X } from 'lucide-react'

import { itemDetail, pcSessionInfo } from '../../api/index'
import { TopBar } from '../../components/TopBar'
import { SensitiveValue, useSensitiveText } from '../../components/SensitiveValue'
import { ComposerActionStrip } from './ComposerActionStrip'
import { EmojiPanel } from './EmojiPanel'
import { canRecallMessage } from './messageBubbleLogic'
import { sessionKey as makeSessionKey } from './messagesConstants'
import { useMessagesContext } from './MessagesRouteLayout'
import { MessageActionSheet, MobileMessageList } from './MobileChatMessages'
import { RichComposerTextarea } from './RichComposerTextarea'
import { SafeRemoteImage } from './SafeRemoteImage'
import { useAuthScope } from '../../hooks/useAuthScope'

// Long-press window for the recall menu (matches Xianyu mobile UX).
const LONG_PRESS_MS = 450

// 1=普通商品会话；3/6 是平台虚拟通道（系统/互动消息），不展示商品卡。
function isItemChat(stRaw: any) {
  return Number(stRaw || 1) === 1
}

function resolveSessionKind(stRaw: any) {
  switch (Number(stRaw || 1)) {
    case 1:
      return '买家私聊'
    case 3:
      return '通知消息'
    case 6:
      return '互动消息'
    default:
      return `会话类型 ${Number(stRaw || 1)}`
  }
}

// 从各种 session 形态里抠出商品卡需要的字段，对齐桌面版 resolveSessionItem。
function resolveItemFromSession(session: any) {
  if (!session) return null
  return {
    itemId: String(session.itemId ?? session.item?.id ?? '').trim(),
    pic: session.itemPicUrl ?? session.itemPic ?? session.item?.pic ?? '',
    title: session.itemTitle ?? session.item?.title ?? '',
    price: session.itemPrice ?? session.item?.price ?? '',
  }
}

export default function MobileChat() {
  const { uid = '', sessionId = '', sessionType = '1' } = useParams<{
    uid: string
    sessionId: string
    sessionType: string
  }>()
  const location = useLocation() as any
  const stNum = Number(sessionType || 1)
  // Stable key for this URL — `historyByKey` / `historyLoadingByKey` use the
  // same scheme so we can read history directly out of the shared store.
  const routeKey = `${uid}:${sessionId}:${stNum}`
  const { authToken, captureScope, isScopeCurrent } = useAuthScope()
  const initialAuthTokenRef = useRef(authToken)

  // Seed from navigation state (MobileMessages passes the full row) — gives
  // us peerUid / peerNick / itemId / latestMessageId immediately so the
  // shared hook can mark read + send / recall without a round-trip. Direct
  // refresh on the URL won't have this; we'll fetch from pcSessionInfo and
  // upsert below.
  const rawInitialSession = location.state?.session || null

  const {
    activeAccountUids,
    historyByKey,
    historyLoadingByKey,
    headinfoBySessionId,
    itemPriceByItemId,
    composerText,
    setComposerText,
    sending,
    imageInputRef,
    selectedSession,
    selectSession,
    setSelectedSessionId,
    handleSend,
    handleImagePick,
    handleSendLocation,
    handleSendItem,
    recallMessage,
    loadHistory,
    upsertSeedSession,
    replyTarget,
    setReplyTarget,
  } = useMessagesContext()
  const routeUidAllowed = activeAccountUids.includes(uid)
  const initialSession = routeUidAllowed && initialAuthTokenRef.current === authToken
    ? rawInitialSession
    : null

  // ─── seed + selection ────────────────────────────────────────────────
  // Push the route's session into the shared store the first time we render
  // for a given URL. If we have a richer object from location.state, prefer
  // it; otherwise lay down a minimal stub so handleSend / markSessionRead /
  // loadHistory all have an _uid + sessionId + sessionType to operate on.
  // The next loadAllSessions merges over this (mergeSessionLists preserves
  // existing peerUid / itemId fields the API doesn't carry).
  useEffect(() => {
    if (!routeUidAllowed) return
    if (!uid || !sessionId) return
    const seed = initialSession
      ? { ...initialSession, _uid: uid }
      : { _uid: uid, sessionId, sessionType: stNum }
    upsertSeedSession(seed)
    // selectSession() flips selectedSessionId, clears the composer, and runs
    // markSessionRead (local zero-out + global store + backend pcMarkSessionRead
    // + liveMarkSessionRead for type=1). That last bit is what makes the
    // list's unread badge drop the moment we enter, even if we navigate back
    // before the upstream call resolves.
    selectSession(seed)
    return () => {
      // Clear the selection on unmount so the list reconciler can re-pick the
      // top session — without this, navigating back leaves the hook focused
      // on a chat the user just left, and the unreadStore "focused" sentinel
      // would suppress badge bumps for that session.
      setSelectedSessionId('')
    }
  }, [uid, sessionId, stNum, routeUidAllowed, initialSession, selectSession, setSelectedSessionId, upsertSeedSession])

  // Direct-link fallback: no nav state means we hit the URL fresh — fetch
  // the session header so peerUid / itemId surface for sendActions + the
  // product card.
  useEffect(() => {
    if (initialSession) return
    if (!routeUidAllowed) return
    if (!uid || !sessionId) return
    const scope = captureScope(routeKey)
    pcSessionInfo(uid, sessionId, stNum)
      .then((sess: any) => {
        if (!isScopeCurrent(scope, routeKey)) return
        if (!sess) return
        const seed = { ...sess, _uid: uid }
        upsertSeedSession(seed)
        selectSession(seed)
      })
      .catch(() => {})
  }, [uid, sessionId, stNum, initialSession, routeUidAllowed, routeKey, captureScope, isScopeCurrent, selectSession, upsertSeedSession])

  // ─── derived view state from the shared store ────────────────────────
  // selectedSession resolves through allSessions (not just filteredSessions)
  // so it hydrates as soon as our upsertSeedSession runs.
  const effectiveSession = useMemo(() => {
    if (!routeUidAllowed) return null
    if (selectedSession && makeSessionKey(selectedSession) === routeKey) return selectedSession
    if (initialSession) return { ...initialSession, _uid: uid }
    return { _uid: uid, sessionId, sessionType: stNum }
  }, [selectedSession, initialSession, uid, sessionId, stNum, routeKey, routeUidAllowed])

  const messages = useMemo(() => {
    const slot = historyByKey[routeKey]
    return Array.isArray(slot?.messages) ? slot.messages : []
  }, [historyByKey, routeKey])
  const loading = Boolean(historyLoadingByKey[routeKey]?.loading)

  const peerNick = String(effectiveSession?.peerNick || '')
  const peerUid = String(effectiveSession?.peerUid || effectiveSession?.peer_uid || '')

  // ─── product card ────────────────────────────────────────────────────
  // The hook's useSelectedSessionResolvers auto-fetches headInfo + itemPrice
  // when selectedSession is non-null, populating headinfoBySessionId /
  // itemPriceByItemId. We read those slots directly so the desktop and
  // mobile chat panels share the same async resolution path.
  const sessionItem = resolveItemFromSession(effectiveSession)
  const headInfo = routeUidAllowed ? headinfoBySessionId[sessionId] || null : null
  const itemIdForPrice = sessionItem?.itemId || ''
  const itemPriceFromHook = itemIdForPrice ? itemPriceByItemId[itemIdForPrice] : ''

  // Belt + braces: when headInfo + the hook's price both miss (rare — happens
  // when the session record has no itemId), pull from itemDetail locally.
  // Mirrors the prior MobileChat logic so we don't regress the "价格加载中…"
  // surface for direct-link refresh on minimal seeds.
  const [localItemPrice, setLocalItemPrice] = useState('')
  useEffect(() => {
    setLocalItemPrice('')
  }, [authToken, routeKey])
  useEffect(() => {
    if (!routeUidAllowed) return
    if (itemPriceFromHook) return
    if (!uid || !itemIdForPrice) return
    if (headInfo?.price) return
    const scope = captureScope(`${routeKey}:${itemIdForPrice}`)
    itemDetail(uid, itemIdForPrice)
      .then((detail: any) => {
        if (!isScopeCurrent(scope, `${routeKey}:${itemIdForPrice}`)) return
        const raw =
          detail?.price ??
          detail?.item?.price ??
          detail?.itemDetail?.price ??
          detail?.data?.price ??
          ''
        const normalized = typeof raw === 'number' ? String(raw) : String(raw || '').trim()
        if (normalized) setLocalItemPrice(normalized)
      })
      .catch(() => {})
  }, [uid, itemIdForPrice, headInfo?.price, itemPriceFromHook, routeUidAllowed, routeKey, captureScope, isScopeCurrent])

  const rawPrice = headInfo?.price || itemPriceFromHook || localItemPrice || sessionItem?.price || ''
  const showItemCard = isItemChat(stNum) && (sessionItem?.itemId || sessionItem?.pic || sessionItem?.title || headInfo)
  const cardPic = headInfo?.picUrl || sessionItem?.pic || ''
  const cardTitle = headInfo?.desc || sessionItem?.title || '关联商品'
  const cardPrice = rawPrice
    ? (/^\d+(\.\d+)?$/.test(String(rawPrice).trim()) ? `¥${rawPrice}` : String(rawPrice))
    : ''
  const cardPromoPrice = headInfo?.promotionPrice || ''
  const safePeerNick = useSensitiveText('buyer', peerNick || '聊天详情')
  const safePeerUid = useSensitiveText('uid', peerUid)
  const safeCardTitle = useSensitiveText('item-title', cardTitle)

  // ─── auto-scroll on new messages ────────────────────────────────────
  const bottomRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  // ─── background polling — same 15s cadence the desktop hook uses ─────
  // The shared hook already runs a one-shot loadHistory effect when
  // selectedSession appears; that handles the initial load. The interval
  // here closes the gap when live-WS pushes are dropped while the user is
  // sitting on the chat. We pass the explicit session so the hook doesn't
  // need to resolve via filteredSessions.
  useEffect(() => {
    if (!routeUidAllowed) return
    if (!uid || !sessionId) return
    const id = window.setInterval(() => {
      if (!sending) loadHistory(effectiveSession, { background: true }).catch(() => {})
    }, 15000)
    return () => window.clearInterval(id)
  }, [uid, sessionId, sending, effectiveSession, loadHistory, routeUidAllowed])

  // ─── send / recall — delegated to the shared sendActions ────────────
  function onSendText() {
    setToolPanelOpen(false)
    setEmojiOpen(false)
    handleSend()
  }
  function onImageChange(e: React.ChangeEvent<HTMLInputElement>) {
    handleImagePick(e)
  }
  async function onRecall(message: any) {
    setActionSheet(null)
    if (!message?.messageId) return
    recallMessage(message)
  }
  // canRecallMessage 走共享 helper（messageBubbleLogic.ts）

  // ─── long-press gesture (touch) ─────────────────────────────────────
  const [actionSheet, setActionSheet] = useState<{
    message: any
    mine: boolean
    canRecall: boolean
  } | null>(null)
  const [toolPanelOpen, setToolPanelOpen] = useState(false)
  const [emojiOpen, setEmojiOpen] = useState(false)
  const composerRef = useRef<HTMLTextAreaElement | null>(null)
  const pressTimerRef = useRef<number | null>(null)
  const pressOriginRef = useRef<{ x: number; y: number } | null>(null)
  function clearPressTimer() {
    if (pressTimerRef.current != null) {
      window.clearTimeout(pressTimerRef.current)
      pressTimerRef.current = null
    }
    pressOriginRef.current = null
  }
  function startLongPress(e: React.TouchEvent, message: any, mine: boolean) {
    clearPressTimer()
    const touch = e.touches[0]
    if (!touch) return
    pressOriginRef.current = { x: touch.clientX, y: touch.clientY }
    pressTimerRef.current = window.setTimeout(() => {
      setActionSheet({
        message,
        mine,
        canRecall: canRecallMessage(message, mine),
      })
    }, LONG_PRESS_MS)
  }
  function moveLongPress(e: React.TouchEvent) {
    const origin = pressOriginRef.current
    const touch = e.touches[0]
    if (!origin || !touch) return
    if (Math.hypot(touch.clientX - origin.x, touch.clientY - origin.y) > 12) {
      clearPressTimer()
    }
  }

  async function handleCopyMessage(message: any) {
    setActionSheet(null)
    const text = String(message?.text ?? message?.content ?? message?.preview ?? '').trim()
    if (!text) return
    try {
      if (navigator?.clipboard?.writeText) {
        await navigator.clipboard.writeText(text)
      } else {
        const ta = document.createElement('textarea')
        ta.value = text
        ta.style.position = 'fixed'
        ta.style.opacity = '0'
        document.body.appendChild(ta)
        ta.select()
        try { document.execCommand('copy') } catch {}
        document.body.removeChild(ta)
      }
    } catch (err) {
      console.error('copy message error', err)
    }
  }

  function handleReplyMessage(message: any) {
    setActionSheet(null)
    const pnm = String(message?.upstreamMessageId || message?.upstream_message_id || '').trim()
    if (!pnm) return
    const text = String(
      message?.text ?? message?.preview ?? message?.title ?? '',
    ).trim() || '[图片]'
    setReplyTarget?.({ messageId: pnm, text })
    // 把焦点交给输入框，跟「重新编辑」一样的 UX。
    setTimeout(() => {
      const ta = document.querySelector<HTMLTextAreaElement>('textarea[placeholder*="输入消息"]')
      if (ta) { ta.focus(); ta.scrollIntoView({ block: 'end' }) }
    }, 30)
  }

  function insertEmoji(emoji: string) {
    setComposerText((text: string) => `${text || ''}${emoji}`)
  }

  function focusComposer() {
    if (toolPanelOpen) setToolPanelOpen(false)
    if (emojiOpen) setEmojiOpen(false)
  }

  function openEmojiPanel() {
    setToolPanelOpen(false)
    setEmojiOpen((open) => !open)
  }

  return (
    <div className={`mobile-chat-shell xianyu-chat-page relative flex flex-col mobile-page-shell overflow-hidden ${toolPanelOpen ? 'xianyu-chat-page-tools-open' : ''}`}>
      <TopBar
        title={safePeerNick}
        subtitle={peerUid ? `会员名：${safePeerUid}` : undefined}
        back
        right={(
          <button type="button" className="xianyu-chat-more-button" aria-label="更多">
            <MoreHorizontal size={24} />
          </button>
        )}
      />

      <div className="mobile-chat-body relative min-h-0 flex-1 mobile-page-body">
        <div className="mobile-chat-stack flex h-full min-h-0 flex-col">
          {showItemCard ? (
            <section className="xianyu-chat-product-bar">
              {cardPic ? (
                <SafeRemoteImage
                  src={cardPic}
                  alt={safeCardTitle}
                  className="h-[54px] w-[54px] shrink-0 rounded-[9px] object-cover"
                  sensitiveItem
                  fallback={<div className="h-[54px] w-[54px] shrink-0 rounded-[9px]" style={{ background: '#ededed' }} />}
                />
              ) : (
                <div className="h-[54px] w-[54px] shrink-0 rounded-[9px]" style={{ background: '#ededed' }} />
              )}

              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-start justify-between gap-3">
                  <div className="min-w-0">
                    {cardPrice ? (
                      <div className="text-[22px] font-semibold leading-tight" style={{ color: 'var(--xianyu-orange)' }}>
                        <SensitiveValue kind="money" value={cardPrice} />
                      </div>
                    ) : (
                      <div className="text-[13px] leading-tight" style={{ color: '#888' }}>价格加载中…</div>
                    )}
                    {cardPromoPrice ? (
                      <span className="mt-0.5 inline-flex rounded-full px-1.5 py-0.5 text-[10px] font-semibold" style={{ backgroundColor: 'var(--xianyu-orange)', color: '#fff' }}>
                        <SensitiveValue kind="money" value={cardPromoPrice} />
                      </span>
                    ) : null}
                    <p className="mt-0.5 truncate text-[13px]" style={{ color: '#565656' }}><SensitiveValue kind="item-title" value={cardTitle} /></p>
                    <p className="mt-0.5 truncate text-[12px]" style={{ color: '#aaa' }}>
                      {itemIdForPrice ? <>商品 <SensitiveValue kind="order-id" value={itemIdForPrice} /></> : '关联商品'}
                    </p>
                  </div>
                </div>
              </div>
            </section>
          ) : null}

          <MobileMessageList
            messages={messages}
            loading={loading}
            uid={uid}
            sessionId={sessionId}
            sessionType={stNum}
            peerNick={peerNick}
            peerUid={peerUid}
            peerLogo={initialSession?.peerLogo}
            selfNick={initialSession?._accountNick}
            selfAvatar={initialSession?._accountAvatar}
            bottomRef={bottomRef}
            onLongPressStart={startLongPress}
            onLongPressMove={moveLongPress}
            onLongPressEnd={clearPressTimer}
            onOpenSheet={setActionSheet}
            canRecallMessage={canRecallMessage}
            onReedit={(originalText: string) => {
              setComposerText(originalText)
              setTimeout(() => {
                const ta = document.querySelector<HTMLTextAreaElement>('textarea[placeholder*="想跟TA说点什么"]')
                if (ta) { ta.focus(); ta.scrollIntoView({ block: 'end' }) }
              }, 30)
            }}
          />

          <section className="xianyu-chat-composer">
            {replyTarget ? (
              <div className="mb-2 flex items-center gap-2 rounded-[14px] px-2.5 py-1.5" style={{ color: '#666', background: '#f4f4f4' }}>
                <span className="rounded-full px-2 py-0.5 text-[10px]" style={{ color: '#111', background: '#e9e9e9' }}>回复</span>
                <span className="flex-1 truncate text-[12px]" title={replyTarget.text}>{replyTarget.text}</span>
                <button type="button" onClick={() => setReplyTarget?.(null)} aria-label="取消引用" className="text-[14px]" style={{ background: 'transparent', border: 0, padding: 4, color: '#777' }}>
                  ✕
                </button>
              </div>
            ) : null}

            <div className="flex items-center gap-3">
              <div className="xianyu-composer-input">
                <RichComposerTextarea
                  ref={composerRef}
                  className="max-h-[88px] min-h-[36px] w-full resize-none bg-transparent text-[15px] outline-none"
                  style={{ color: '#111', lineHeight: 1.4 }}
                  previewStyle={{ fontSize: 15, lineHeight: 1.4 }}
                  placeholder="想跟TA说点什么..."
                  rows={1}
                  value={composerText}
                  onChange={setComposerText}
                  onFocus={focusComposer}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); onSendText() }
                  }}
                />
              </div>
              <button
                type="button"
                className="xianyu-composer-icon-button"
                aria-label="表情"
                onClick={openEmojiPanel}
              >
                <Smile size={24} />
              </button>
              {composerText.trim() ? (
                <button onClick={onSendText} disabled={sending || !composerText.trim()} className="xianyu-send-button" aria-label="发送">
                  <SendHorizontal className="h-[18px] w-[18px]" />
                </button>
              ) : (
                <button type="button" className="xianyu-composer-icon-button" aria-label={toolPanelOpen ? '关闭更多功能' : '更多功能'} onClick={() => { setEmojiOpen(false); setToolPanelOpen((open) => !open) }}>
                  {toolPanelOpen ? <X size={24} /> : <Plus size={25} />}
                </button>
              )}
            </div>

            {toolPanelOpen ? (
              <ComposerActionStrip
                variant="mobile"
                uid={uid}
                sessionId={sessionId}
                disabled={sending}
                imageInputRef={imageInputRef}
                onImagePick={onImageChange}
                onInsertEmoji={insertEmoji}
                onSendItem={async (it) => { if (handleSendItem) await handleSendItem(it) }}
                onSendLocation={async (c) => { if (handleSendLocation) await handleSendLocation(c) }}
              />
            ) : null}

            <EmojiPanel
              open={emojiOpen}
              variant="mobile"
              inline
              onPick={insertEmoji}
              onClose={() => setEmojiOpen(false)}
            />
          </section>
        </div>
      </div>

      <MessageActionSheet
        state={actionSheet}
        onClose={() => setActionSheet(null)}
        onCopy={handleCopyMessage}
        onReply={handleReplyMessage}
        onRecall={onRecall}
      />
    </div>
  )
}
