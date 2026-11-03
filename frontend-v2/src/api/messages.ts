import type { RequestOptions, Session, Message } from './types'
import { request, getBaseURL } from './client'
import { emitActionFailure, emitActionStarted, emitActionSuccess } from '../bootstrap/actionLogging'

// 引用回复发送：可选的 PNM + 引用文本。后端 live_ops.rs 把 PNM 塞进 WS 帧
// 的 extension.extJson，把 reply_to_text stash 进 message_history_cache 让
// 接收端不必回扫历史就能渲染气泡 quote 块。
export interface PcSendReplyContext {
  reply_to_message_id?: string
  reply_to_text?: string
}

export const pcSendMessage = (
  uid: string,
  sessionId: string,
  toUid: string,
  content: string,
  reply?: PcSendReplyContext,
) =>
  request<{
    ok: boolean
    messageId: string
    upstreamMessageId: string
    ts: number
  }>('POST', '/api/messages/pc/send', {
    uid,
    session_id: sessionId,
    to_uid: toUid,
    content,
    reply_to_message_id: reply?.reply_to_message_id || '',
    reply_to_text: reply?.reply_to_text || '',
  })

// ==================== IM 消息 ====================

export const pcSessionsBatch = (num = 30) =>
  request<{ sessions: Session[] }>('GET', `/api/messages/pc/sessions/batch?num=${num}`)

export const pcMarkSessionRead = (uid: string, sessionId: string, sessionType = 1) =>
  request<{ ok: boolean }>('POST', '/api/messages/pc/sessions/read', {
    uid,
    session_id: sessionId,
    session_type: sessionType,
  })

export const pcSessions = async (
  uid: string,
  types = '[1,3,6]',
  num = 30,
  { cached = false, force = false } = {},
) => {
  const r = await request<any>(
    'GET',
    `/api/messages/pc/sessions?uid=${uid}&types=${encodeURIComponent(types)}&num=${num}${cached ? '&cached=true' : ''}${force ? '&force=true' : ''}`,
  )
  const sessions = r?.sessions || r?.normalized?.sessions || r?.data?.sessions || []
  return {
    ...r,
    sessions,
    warning: r?.warning || r?.normalized?.warning || null,
    stale: Boolean(r?.stale ?? r?.normalized?.stale),
  } as { sessions: Session[]; warning?: string | null; stale?: boolean }
}

export const pcMessageHistory = async (
  uid: string,
  sessionId: string,
  sessionType = 1,
  num = 30,
  ts: number | string = 0,
  { cached = false, force = false }: { cached?: boolean; force?: boolean } = {},
) => {
  const q = `uid=${uid}&session_id=${sessionId}&session_type=${sessionType}&num=${num}&ts=${ts}` +
    (cached ? '&cached=true' : '') + (force ? '&force=true' : '')
  const r = await request<any>('GET', `/api/messages/pc/history?${q}`)
  const inner = r?.normalized || r
  const messages = inner?.messages || []
  return {
    ...inner,
    messages,
    warning: inner?.warning || null,
    stale: Boolean(inner?.stale),
  } as { messages: Message[]; hasMore?: boolean; nextCursor?: number; latestCursor?: number; warning?: string | null; stale?: boolean }
}

export const pcSessionInfo = (uid: string, sessionId: string, sessionType = 1) =>
  request<Session>(
    'GET',
    `/api/messages/pc/session-info?uid=${uid}&session_id=${sessionId}&session_type=${sessionType}`,
  )

export const pcMessageHeadinfo = (uid: string, sessionId: string, itemId: string, peerUid = '') =>
  request<any>(
    'GET',
    `/api/messages/pc/headinfo?uid=${uid}&session_id=${sessionId}&item_id=${itemId}&peer_uid=${peerUid}`,
  )

export const uploadChatImage = async (uid: string, file: File) => {
  const fd = new FormData()
  fd.append('uid', uid)
  fd.append('file', file)
  emitActionStarted({
    module: 'messages',
    action: 'upload_chat_image',
    message: '开始上传聊天图片',
    context: { uid, fileSize: file.size || 0, fileType: file.type || '' },
  })
  const baseURL = await getBaseURL()
  try {
    const r = await fetch(`${baseURL}/api/chat/upload_image`, { method: 'POST', body: fd })
    const payload = await r.json().catch(() => ({}))
    if (!r.ok) {
      throw new Error(payload?.detail || payload?.message || `HTTP ${r.status}`)
    }
    emitActionSuccess({
      module: 'messages',
      action: 'upload_chat_image',
      message: '聊天图片上传成功',
      context: {
        uid,
        fileSize: file.size || 0,
        width: Number(payload?.width || 0),
        height: Number(payload?.height || 0),
        hasUrl: Boolean(payload?.url),
      },
    })
    return payload as { success?: boolean; url?: string; width?: number; height?: number }
  } catch (error) {
    emitActionFailure({
      module: 'messages',
      action: 'upload_chat_image',
      message: '聊天图片上传失败',
      error,
      context: { uid, fileSize: file.size || 0, fileType: file.type || '' },
    })
    throw error
  }
}

// Send an already-uploaded image URL via the live ACCS socket.
// Backend wraps it as contentType=2 (image) inside the standard
// contentType=101 envelope — see live_ops.rs::live_send_image.
export const liveSendImage = (
  uid: string,
  sessionId: string,
  toUid: string,
  imageUrl: string,
  width = 0,
  height = 0,
) =>
  request<{ ok: boolean; messageId?: string; upstreamMessageId?: string }>('POST', '/api/messages/send/image', {
    uid,
    session_id: sessionId,
    to_uid: toUid,
    image_url: imageUrl,
    width,
    height,
  })

// Read-receipt: tell the upstream we've read the session up to lastReadVersion.
// Backend forwards `mtop.taobao.idlemessage.message.read` (v1.0) which accepts
// a list of message ids. Safe to fire-and-forget. Pass `msgIdList` with all
// currently-visible unread message ids, or omit to let the backend fall back
// to the cached "latest message" cursor.
//   POST /api/messages/session/read
export const liveMarkSessionRead = (
  uid: string,
  sessionId: string,
  opts: { msgIdList?: string[]; msgId?: string; peerUid?: string } = {},
  options?: RequestOptions,
) =>
  request<{ ok: boolean; msgCount: number; upstream: unknown }>(
    'POST',
    '/api/messages/session/read',
    {
      uid,
      session_id: sessionId,
      msg_id_list: opts.msgIdList || [],
      msg_id: opts.msgId || '',
      peer_uid: opts.peerUid || '',
    },
    options,
  )

// Send a chosen POI as a chat location card. The PC-web flow is: get GPS,
// fetch nearby POIs from AMap, user picks one, send the POI (not the raw GPS).
// Backend expects latitude/longitude as STRINGS — Xianyu wire encodes them
// as JSON strings inside `locationCard`, and serde would reject a number.
//   POST /api/messages/send/location
export const liveSendLocation = (
  uid: string,
  sessionId: string,
  toUid: string,
  latitude: string,
  longitude: string,
  title: string,
  content: string,
) =>
  request<{ ok: boolean; messageId?: string; upstreamMessageId?: string }>('POST', '/api/messages/send/location', {
    uid,
    session_id: sessionId,
    to_uid: toUid,
    latitude,
    longitude,
    title,
    content,
  })

// Search items for the composer item-card picker. Wraps the upstream
// `mtop.taobao.idlemessage.pc.tool.item.search` call: `role` decides whose
// listings we're enumerating — "own" returns the current account's items,
// "other" returns the peer's items in the conversation. `query` is the
// server-side title filter (no client-side fan-out needed).
//   POST /api/messages/items/search
export interface ItemSearchResultItem {
  itemId: string
  title: string
  mainPic: string
  price: string
}
export interface ItemSearchResult {
  items: ItemSearchResultItem[]
  hasMore: boolean
  pageNumber: number
  role: 'own' | 'other'
}
export const searchSendItems = (
  uid: string,
  sessionId: string,
  role: 'own' | 'other',
  query = '',
  page = 1,
  pageSize = 20,
  options?: RequestOptions,
) =>
  request<ItemSearchResult>('POST', '/api/messages/items/search', {
    uid,
    session_id: sessionId,
    role,
    query,
    page,
    page_size: pageSize,
  }, options)

// Send one of the user's own items as a card message.
//   POST /api/messages/send/item
export const liveSendItem = (
  uid: string,
  sessionId: string,
  toUid: string,
  itemId: string,
  title: string,
  mainPic: string,
  price: string,
  itemTip = '',
) =>
  request<{ ok: boolean; messageId?: string; upstreamMessageId?: string }>('POST', '/api/messages/send/item', {
    uid,
    session_id: sessionId,
    to_uid: toUid,
    item_id: itemId,
    title,
    main_pic: mainPic,
    price,
    item_tip: itemTip,
  })

// Recall (撤回) one of the user's own outgoing messages.
// Backend calls `mtop.taobao.idlemessage.recall.message` v1.0 and locally
// rewrites the cached row to `kind="recalled"` so the bubble flips to the
// system pill. `messageId` must be the PNM-format server-side id (e.g.
// "4111237573609.PNM"), NOT the local UUID — pull it from the cached
// history entry's `messageId` field.
// Xianyu convention: this only succeeds within ~2 minutes of send.
//   POST /api/messages/recall
export const liveRecallMessage = (
  uid: string,
  sessionId: string,
  messageId: string,
  // PNM 格式的上游 messageId — 撤回 API 唯一认的格式。前端从
  // `message.upstreamMessageId`（normalize 时塞进去的）拿；不传后端会按
  // (uid, session_id, message_id) 在 cache 里查。
  upstreamMessageId?: string,
) =>
  request<{ ok: boolean; upstreamOk: boolean; rowsUpdated: number; upstream: unknown }>(
    'POST',
    '/api/messages/recall',
    {
      uid,
      session_id: sessionId,
      message_id: messageId,
      upstream_message_id: upstreamMessageId || '',
    },
  )
