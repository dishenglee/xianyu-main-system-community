// @ts-nocheck
// handleSend / handleImagePick / recallMessage factories, pulled out of
// useMessagesPageState so the main hook stays under the 600-line target.
// These are imperative async functions; they receive the relevant state +
// setters via the options bag, no React state owned here.
import {
  liveRecallMessage,
  liveSendImage,
  liveSendItem,
  liveSendLocation,
  request,
  uploadChatImage,
} from '../../../api/index'
import { mergeHistoryState } from '../historyState'
import { sessionKey } from '../messagesConstants'
import {
  emitActionFailure,
  emitActionStarted,
  emitActionSuccess,
} from '../../../bootstrap/actionLogging'

interface SendActionsOptions {
  selectedSessionRef: { current: any }
  composerTextRef: { current: string }
  sendingRef: { current: boolean }
  replyTargetRef: { current: { messageId: string; text: string } | null }
  setSending: (v: boolean) => void
  setComposerText: (v: string) => void
  setHistoryByKey: (
    updater: (prev: Record<string, any>) => Record<string, any>,
  ) => void
  setReplyTarget: (v: { messageId: string; text: string } | null) => void
  updateSessionsForUid: (uid: string, updater: (sessions: any[]) => any[]) => void
  loadHistory: (session: any, opts?: { background?: boolean }) => Promise<void>
  notifyError?: (message: string) => void
}

export function removeOptimisticMessageFromHistory(
  prev: Record<string, any>,
  key: string,
  messageId: string,
) {
  const existing = prev[key]
  if (!existing || !messageId) return prev
  const messages = (existing.messages || []).filter((m: any) => m?.messageId !== messageId)
  if (messages.length === (existing.messages || []).length) return prev
  return { ...prev, [key]: { ...existing, messages } }
}

export function createSendActions(opts: SendActionsOptions) {
  const {
    selectedSessionRef, composerTextRef, sendingRef, replyTargetRef,
    setSending, setComposerText, setHistoryByKey, setReplyTarget,
    updateSessionsForUid, loadHistory, notifyError,
  } = opts

  function removeOptimistic(key: string, messageId: string) {
    setHistoryByKey((prev) => removeOptimisticMessageFromHistory(prev, key, messageId))
  }

  function formatSendError(error: any, fallback: string) {
    const message = String(error?.message || error || '').trim()
    return message ? `${fallback}：${message}` : fallback
  }

  function messageLogContext(session: any, extra: Record<string, unknown> = {}) {
    return {
      uid: session?._uid || '',
      sessionId: session?.sessionId || '',
      peerUid: session?.peerUid || session?.peer_uid || session?.targetUid || '',
      ...extra,
    }
  }

  async function handleImagePick(event: React.ChangeEvent<HTMLInputElement>) {
    const selectedSession = selectedSessionRef.current
    const file = event.target.files?.[0]
    if (!file || !selectedSession?._uid) return
    // Reset so the same file can be re-selected later.
    event.target.value = ''
    setSending(true)
    emitActionStarted({
      module: 'messages',
      action: 'send_image',
      message: '开始发送图片消息',
      context: messageLogContext(selectedSession, {
        fileSize: file.size || 0,
        fileType: file.type || '',
      }),
    })
    let optKey = ''
    let optimisticMessageId = ''
    try {
      // 1) Upload bytes to Xianyu CDN. Backend returns
      //    `{success, url, width, height}`.
      const uploaded = await uploadChatImage(selectedSession._uid, file)
      const imageUrl = String(uploaded?.url || '').trim()
      if (!imageUrl) {
        throw new Error('图片上传未返回 URL')
      }
      // 2) Send the URL over the live ACCS socket (POST /api/messages/send/image).
      //    Backend wraps it as contentType=2 inside the standard 101 envelope —
      //    see live_ops.rs::live_send_image.
      const peerUid =
        selectedSession.peerUid ||
        selectedSession.peer_uid ||
        selectedSession.targetUid ||
        ''
      if (!peerUid) {
        throw new Error('缺少对方 uid (peerUid)')
      }
      // 2a) Show the image bubble IMMEDIATELY — same optimistic pattern as
      //     handleSend for text. The loadHistory below replaces it with the
      //     real PNM-id echo once the live worker persists.
      optKey = sessionKey(selectedSession)
      const optimisticImage = {
        messageId: `optimistic_${Date.now()}`,
        imageUrl,
        kind: 'image',
        text: '',
        contentType: 2,
        isSelf: true,
        ts: Date.now(),
        version: Date.now(),
      }
      optimisticMessageId = optimisticImage.messageId
      setHistoryByKey((prev) => ({
        ...prev,
        [optKey]: mergeHistoryState(prev[optKey] || {}, { messages: [optimisticImage] }, 'refresh'),
      }))
      updateSessionsForUid(selectedSession._uid, (sessions: any[]) => sessions.map((current: any) => (
        current.sessionId === selectedSession.sessionId
          ? { ...current, summary: '[图片]', ts: Date.now(), unread: 0 }
          : current
      )))
      const sendResult = await liveSendImage(
        selectedSession._uid,
        selectedSession.sessionId,
        String(peerUid),
        imageUrl,
        Number(uploaded?.width || 0),
        Number(uploaded?.height || 0),
      )
      // Promote optimistic→real：拿后端返回的 messageId + PNM 把 optimistic 行
      // 升级成「可撤回」状态。没这一步，bubble 一直停在 optimistic_xxx ID 上，
      // canRecallMessage 走 isOptimistic 闸门 reject，撤回按钮永远不会显示。
      const realId = String(sendResult?.messageId || '').trim()
      const pnm = String(sendResult?.upstreamMessageId || '').trim()
      if (!realId || !pnm) {
        throw new Error('图片发送未收到闲鱼确认，请重试')
      }
      if (realId) {
        setHistoryByKey((prev) => {
          const existing = prev[optKey]
          if (!existing) return prev
          const messages = (existing.messages || []).map((m: any) => {
            if (m?.messageId !== optimisticImage.messageId) return m
            return { ...m, messageId: realId, upstreamMessageId: pnm, version: Date.now() }
          })
          return { ...prev, [optKey]: { ...existing, messages } }
        })
      }
      // 3) Let the server fan out before refreshing from cache. Backend
      //    persists the sent-image echo into `message_history_cache` via the
      //    live worker, same as for text sends.
      await new Promise((r) => setTimeout(r, 300))
      await loadHistory(selectedSession, { background: true })
      emitActionSuccess({
        module: 'messages',
        action: 'send_image',
        message: '图片消息发送成功',
        context: messageLogContext(selectedSession, { messageId: realId }),
      })
    } catch (error) {
      emitActionFailure({
        module: 'messages',
        action: 'send_image',
        message: '图片消息发送失败',
        error,
        context: messageLogContext(selectedSession),
      })
      console.error('upload/send image error', error)
      removeOptimistic(optKey, optimisticMessageId)
      notifyError?.(formatSendError(error, '图片发送失败'))
    } finally {
      setSending(false)
    }
  }

  async function handleSend() {
    const selectedSession = selectedSessionRef.current
    if (!selectedSession || sendingRef.current) return
    const text = composerTextRef.current.trim()
    if (!text) return

    // 把发送瞬间的 reply target 锁住——下面把它清掉之后 setReplyTarget(null)
    // 才不会影响 optimistic 行。
    const replyTarget = replyTargetRef.current
    const replyPNM = String(replyTarget?.messageId || '').trim()
    const replyText = String(replyTarget?.text || '').trim()

    const key = sessionKey(selectedSession)
    const optimisticMessage = {
      messageId: `optimistic_${Date.now()}`,
      text,
      isSelf: true,
      ts: Date.now(),
      // 立刻在 optimistic 气泡上画 quote 块——避免「发完→等接收侧持久化→刷
      // 历史」期间引用条消失。后端的持久化行也带这两字段，下次 loadHistory
      // 不会把它抹掉。
      replyToMessageId: replyPNM,
      replyToText: replyText,
    }

    setSending(true)
    emitActionStarted({
      module: 'messages',
      action: 'send_text',
      message: '开始发送文本消息',
      context: messageLogContext(selectedSession, {
        contentLength: text.length,
        hasReply: Boolean(replyPNM),
      }),
    })
    setComposerText('')
    setReplyTarget(null)
    setHistoryByKey((prev) => ({
      ...prev,
      [key]: mergeHistoryState(prev[key] || {}, { messages: [optimisticMessage] }, 'refresh'),
    }))
    updateSessionsForUid(selectedSession._uid, (sessions: any[]) => sessions.map((current: any) => (
      current.sessionId === selectedSession.sessionId
        ? { ...current, summary: text, ts: Date.now(), unread: 0 }
        : current
    )))

    try {
      // Route to the live ACCS WebSocket path (POST /api/messages/pc/send).
      // Backend maps uid -> active WS sender, pushes the MessageSend payload,
      // waits for the ACCS ACK (2s timeout), then returns the real server-side
      // PNM messageId as `upstreamMessageId`. Returns 503 if the live channel
      // for this uid isn't connected yet.
      const peerUid =
        selectedSession.peerUid ||
        selectedSession.peer_uid ||
        selectedSession.targetUid ||
        ''
      if (!peerUid) {
        throw new Error('缺少对方 uid (peerUid)')
      }
      const sendResult = await request<any>('POST', '/api/messages/pc/send', {
        uid: selectedSession._uid,
        session_id: selectedSession.sessionId,
        to_uid: String(peerUid),
        content: text,
        // 引用回复：reply_to_message_id 必须是 PNM 格式（"<seq>.PNM"），后端
        // 把它塞到 WS 帧的 extension.extJson；reply_to_text 仅本端 stash 到
        // message_history_cache 给接收侧渲染气泡 quote 用。
        reply_to_message_id: replyPNM,
        reply_to_text: replyText,
      })
      // Promote the optimistic bubble: swap its `optimistic_<ts>` id for the
      // real client UUID + PNM the backend just got from the ACCS ACK. Without
      // this swap, `canRecallMessage` rejects it as optimistic (see
      // messageBubbleLogic.ts:22) and right-click/long-press recall never
      // surfaces until the user refreshes the page.
      const realId = String(sendResult?.messageId || '').trim()
      const pnm = String(sendResult?.upstreamMessageId || '').trim()
      if (!realId || !pnm) {
        throw new Error('消息发送未收到闲鱼确认，请重试')
      }
      if (realId) {
        setHistoryByKey((prev) => {
          const existing = prev[key]
          if (!existing) return prev
          const messages = (existing.messages || []).map((m: any) => {
            if (m?.messageId !== optimisticMessage.messageId) return m
            return {
              ...m,
              messageId: realId,
              upstreamMessageId: pnm,
              // Stamp a numeric server version so computeReadHint and similar
              // helpers stop treating this bubble as in-flight.
              version: Number(sendResult?.ts) || m.version || Date.now(),
            }
          })
          return { ...prev, [key]: { ...existing, messages } }
        })
      }
      // Give the server ~300ms to fan out before refetching history so the
      // new message shows up instead of the stale snapshot.
      await new Promise((r) => setTimeout(r, 300))
      await loadHistory(selectedSession, { background: true })
      emitActionSuccess({
        module: 'messages',
        action: 'send_text',
        message: '文本消息发送成功',
        context: messageLogContext(selectedSession, { messageId: realId }),
      })
    } catch (error) {
      emitActionFailure({
        module: 'messages',
        action: 'send_text',
        message: '文本消息发送失败',
        error,
        context: messageLogContext(selectedSession, {
          contentLength: text.length,
          hasReply: Boolean(replyPNM),
        }),
      })
      console.error('send message error', error)
      removeOptimistic(key, optimisticMessage.messageId)
      setComposerText(text)
      // 发送失败把 reply target 还回去，用户可以重发。
      if (replyTarget) setReplyTarget(replyTarget)
      notifyError?.(formatSendError(error, '消息发送失败'))
    } finally {
      setSending(false)
    }
  }

  // Recall (撤回) a previously-sent outgoing message. Triggered from the
  // right-click context menu (desktop) or long-press action sheet (mobile).
  // 2-minute window (闲鱼 convention) is enforced at the menu render level —
  // backend will still attempt the rewrite even past that window, but the
  // upstream LWP frame is what the peer respects.
  async function recallMessage(message: any) {
    const selectedSession = selectedSessionRef.current
    if (!selectedSession || !message?.messageId) return
    const sid = selectedSession.sessionId
    const uid = selectedSession._uid
    try {
      emitActionStarted({
        module: 'messages',
        action: 'recall_message',
        message: '开始撤回消息',
        context: messageLogContext(selectedSession, {
          messageId: String(message.messageId || ''),
          hasUpstreamMessageId: Boolean(message.upstreamMessageId),
        }),
      })
      // 撤回 API（mtop.taobao.idlemessage.recall.message）只认 PNM 格式
      // messageId（"<seq>.PNM"），不认我们做主键用的 UUID。所以传两个：
      //   • message_id — UUID，后端用来更新本地 cache 行
      //   • upstream_message_id — PNM，后端用来调上游撤回 API
      // 后端在 upstream_message_id 缺失时会按 (uid, sid, message_id) 在 cache 里查。
      await liveRecallMessage(
        uid,
        sid,
        String(message.messageId),
        String(message.upstreamMessageId || ''),
      )
      // Optimistic local rewrite: flip the bubble to a `kind="recalled"`
      // entry so the existing system-pill renderer picks it up immediately.
      // Backend has also rewritten the cached row, so the next history reload
      // preserves this state. We stash the original text in `originalText`
      // (and `recalledAt`) so the bubble can render a 重新编辑 link — Xianyu
      // shows this for ~2 minutes after recall and clicking it loads the
      // original text back into the composer. The backend's recalled row
      // doesn't preserve original text, so after a reload the link
      // disappears; that matches official Xianyu's behavior.
      const key = sessionKey(selectedSession)
      const originalText = String(message?.text || '')
      const recalledAt = Date.now()
      setHistoryByKey((prev) => {
        const existing = prev[key]
        if (!existing) return prev
        const messages = (existing.messages || []).map((m: any) => {
          if (String(m?.messageId || '') !== String(message.messageId || '')) return m
          return {
            ...m,
            kind: 'recalled',
            text: '你撤回了一条消息',
            title: '',
            imageUrl: '',
            imageUrls: [],
            preview: '你撤回了一条消息',
            originalText,
            recalledAt,
          }
        })
        return { ...prev, [key]: { ...existing, messages } }
      })
      emitActionSuccess({
        module: 'messages',
        action: 'recall_message',
        message: '消息撤回成功',
        context: messageLogContext(selectedSession, {
          messageId: String(message.messageId || ''),
        }),
      })
    } catch (error) {
      emitActionFailure({
        module: 'messages',
        action: 'recall_message',
        message: '消息撤回失败',
        error,
        context: messageLogContext(selectedSession, {
          messageId: String(message.messageId || ''),
        }),
      })
      console.error('recall message error', error)
      // 把错误抛出去，让 UI（MobileChat / MessagePanels）catch 后 notify 用户。
      // 否则后端撤回失败时前端不改 bubble 状态又不报错，用户不知道发生了什么。
      throw error
    }
  }

  // Send a POI the user picked from the AMap nearby list as a location-card
  // chat message. Caller (ComposerActionStrip) already resolved GPS → POI list
  // → user pick, so by the time we get here `title` is the POI name and
  // `content` is its address. Mirrors handleSendItem in shape: optimistic
  // bubble inserted, history refetched after a small delay.
  //
  // latitude/longitude are passed through as strings — the backend payload
  // types them as String and Xianyu wire-encodes them as JSON strings inside
  // the locationCard.
  async function handleSendLocation(coords: {
    latitude: number | string
    longitude: number | string
    title: string
    content?: string
  }) {
    const selectedSession = selectedSessionRef.current
    if (!selectedSession?._uid) return
    const peerUid =
      selectedSession.peerUid ||
      selectedSession.peer_uid ||
      selectedSession.targetUid ||
      ''
    if (!peerUid) throw new Error('缺少对方 uid (peerUid)')

    setSending(true)
    emitActionStarted({
      module: 'messages',
      action: 'send_location',
      message: '开始发送位置消息',
      context: messageLogContext(selectedSession, {
        hasAddress: Boolean(coords.content),
        hasTitle: Boolean(coords.title),
      }),
    })
    let optKey = ''
    let optimisticMessageId = ''
    try {
      optKey = sessionKey(selectedSession)
      const lat = typeof coords.latitude === 'string'
        ? coords.latitude
        : Number(coords.latitude).toFixed(6)
      const lng = typeof coords.longitude === 'string'
        ? coords.longitude
        : Number(coords.longitude).toFixed(6)
      const title = coords.title || '位置'
      const content = coords.content || ''
      // optimistic 必须跟后端 persist 的 cache 行字段完全一致——否则前端
      // renderer 因为 contentType/cardMeta 缺失把它当系统胶囊渲染（=== `[位置] xx` 灰胶囊）。
      // 真实流程：发出去 → backend 持久化 → 300ms 后 loadHistory 拿到真行 → 替换 optimistic。
      // 这中间这一帧必须能渲染成正确的位置卡片。
      const optimistic = {
        messageId: `optimistic_${Date.now()}`,
        kind: 'card',
        contentType: 30,
        text: title,
        title,
        preview: `[位置] ${title}`,
        imageUrl: 'https://img.alicdn.com/imgextra/i4/O1CN01t5Yxwi1vA3LNVBAWU_!!6000000006131-2-tps-300-300.png',
        cardMeta: {
          kind: 'location',
          title,
          content,
          latitude: lat,
          longitude: lng,
          previewUrl: 'https://img.alicdn.com/imgextra/i4/O1CN01t5Yxwi1vA3LNVBAWU_!!6000000006131-2-tps-300-300.png',
        },
        isSelf: true,
        ts: Date.now(),
        version: Date.now(),
      }
      optimisticMessageId = optimistic.messageId
      setHistoryByKey((prev) => ({
        ...prev,
        [optKey]: mergeHistoryState(prev[optKey] || {}, { messages: [optimistic] }, 'refresh'),
      }))
      updateSessionsForUid(selectedSession._uid, (sessions: any[]) => sessions.map((current: any) => (
        current.sessionId === selectedSession.sessionId
          ? { ...current, summary: `[位置] ${title}`, ts: Date.now(), unread: 0 }
          : current
      )))
      const sendResult = await liveSendLocation(
        selectedSession._uid,
        selectedSession.sessionId,
        String(peerUid),
        lat,
        lng,
        title,
        content,
      )
      // Promote optimistic→real：换上后端返回的 messageId + PNM，让撤回能用。
      const realId = String(sendResult?.messageId || '').trim()
      const pnm = String(sendResult?.upstreamMessageId || '').trim()
      if (!realId || !pnm) {
        throw new Error('位置发送未收到闲鱼确认，请重试')
      }
      if (realId) {
        setHistoryByKey((prev) => {
          const existing = prev[optKey]
          if (!existing) return prev
          const messages = (existing.messages || []).map((m: any) => {
            if (m?.messageId !== optimistic.messageId) return m
            return { ...m, messageId: realId, upstreamMessageId: pnm, version: Date.now() }
          })
          return { ...prev, [optKey]: { ...existing, messages } }
        })
      }
      await new Promise((r) => setTimeout(r, 300))
      await loadHistory(selectedSession, { background: true })
      emitActionSuccess({
        module: 'messages',
        action: 'send_location',
        message: '位置消息发送成功',
        context: messageLogContext(selectedSession, { messageId: realId }),
      })
    } catch (error) {
      emitActionFailure({
        module: 'messages',
        action: 'send_location',
        message: '位置消息发送失败',
        error,
        context: messageLogContext(selectedSession, {
          hasAddress: Boolean(coords.content),
          hasTitle: Boolean(coords.title),
        }),
      })
      removeOptimistic(optKey, optimisticMessageId)
      throw error
    } finally {
      setSending(false)
    }
  }

  // Send one of the user's own items as a card message. Caller passes the
  // item fields harvested from the picker grid.
  async function handleSendItem(item: {
    itemId: string
    title: string
    mainPic: string
    price: string
    itemTip?: string
  }) {
    const selectedSession = selectedSessionRef.current
    if (!selectedSession?._uid) return
    const peerUid =
      selectedSession.peerUid ||
      selectedSession.peer_uid ||
      selectedSession.targetUid ||
      ''
    if (!peerUid) throw new Error('缺少对方 uid (peerUid)')

    setSending(true)
    emitActionStarted({
      module: 'messages',
      action: 'send_item',
      message: '开始发送商品卡片',
      context: messageLogContext(selectedSession, {
        itemId: item.itemId,
        hasMainPic: Boolean(item.mainPic),
        hasPrice: Boolean(item.price),
      }),
    })
    let optKey = ''
    let optimisticMessageId = ''
    try {
      optKey = sessionKey(selectedSession)
      // optimistic 字段跟 backend 持久化的 cache 行对齐——`isProductCardMessage`
      // 检查 contentType===7 || cardMeta.kind==='item'，两者都要给上，否则
      // optimistic 那一帧会被当系统胶囊渲染。
      const optimistic = {
        messageId: `optimistic_${Date.now()}`,
        kind: 'card',
        contentType: 7,
        text: item.title,
        title: item.title,
        preview: `[商品] ${item.title}`,
        imageUrl: item.mainPic,
        cardMeta: {
          kind: 'item',
          itemId: item.itemId,
          title: item.title,
          mainPic: item.mainPic,
          price: item.price,
          itemTip: item.itemTip || '我想要',
        },
        isSelf: true,
        ts: Date.now(),
        version: Date.now(),
      }
      optimisticMessageId = optimistic.messageId
      setHistoryByKey((prev) => ({
        ...prev,
        [optKey]: mergeHistoryState(prev[optKey] || {}, { messages: [optimistic] }, 'refresh'),
      }))
      updateSessionsForUid(selectedSession._uid, (sessions: any[]) => sessions.map((current: any) => (
        current.sessionId === selectedSession.sessionId
          ? { ...current, summary: `[商品]${item.title}`, ts: Date.now(), unread: 0 }
          : current
      )))
      const sendResult = await liveSendItem(
        selectedSession._uid,
        selectedSession.sessionId,
        String(peerUid),
        item.itemId,
        item.title,
        item.mainPic,
        item.price,
        item.itemTip || '',
      )
      // Promote optimistic→real：换上后端返回的 messageId + PNM，让撤回能用。
      const realId = String(sendResult?.messageId || '').trim()
      const pnm = String(sendResult?.upstreamMessageId || '').trim()
      if (!realId || !pnm) {
        throw new Error('商品卡片发送未收到闲鱼确认，请重试')
      }
      if (realId) {
        setHistoryByKey((prev) => {
          const existing = prev[optKey]
          if (!existing) return prev
          const messages = (existing.messages || []).map((m: any) => {
            if (m?.messageId !== optimistic.messageId) return m
            return { ...m, messageId: realId, upstreamMessageId: pnm, version: Date.now() }
          })
          return { ...prev, [optKey]: { ...existing, messages } }
        })
      }
      await new Promise((r) => setTimeout(r, 300))
      await loadHistory(selectedSession, { background: true })
      emitActionSuccess({
        module: 'messages',
        action: 'send_item',
        message: '商品卡片发送成功',
        context: messageLogContext(selectedSession, {
          itemId: item.itemId,
          messageId: realId,
        }),
      })
    } catch (error) {
      emitActionFailure({
        module: 'messages',
        action: 'send_item',
        message: '商品卡片发送失败',
        error,
        context: messageLogContext(selectedSession, {
          itemId: item.itemId,
          hasMainPic: Boolean(item.mainPic),
          hasPrice: Boolean(item.price),
        }),
      })
      removeOptimistic(optKey, optimisticMessageId)
      throw error
    } finally {
      setSending(false)
    }
  }

  return {
    handleImagePick,
    handleSend,
    handleSendLocation,
    handleSendItem,
    recallMessage,
  }
}
