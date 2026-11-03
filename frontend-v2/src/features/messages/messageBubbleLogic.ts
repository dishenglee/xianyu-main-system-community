// Pure helpers shared by Desktop (MessagePanels.tsx) and Mobile
// (MobileChatMessages.tsx + MobileChat.tsx). Anything that decides "what
// state is this bubble in" lives here. Each renderer only differs in
// className / inline style / sheet vs context-menu. Add a rule once here
// and both ends pick it up automatically.

// 撤回窗口 — 闲鱼客户端限定 2 分钟内可撤回。
export const RECALL_WINDOW_MS = 2 * 60 * 1000
// 撤回后「重新编辑」链接显示窗口 — 闲鱼/微信约定 2 分钟。
export const REEDIT_WINDOW_MS = 2 * 60 * 1000

export function isOptimisticMessage(message: any): boolean {
  return String(message?.messageId || '').startsWith('optimistic_')
}

export function messageKindOf(message: any): string {
  return String(message?.kind || '')
}

/// 撤回资格：自己发的、非乐观、非已撤回、2 分钟内 + 有真实服务端 message_id。
/// (mine 由调用方传入，因为 self 判断依赖 currentUid 上下文。)
export function canRecallMessage(message: any, mine: boolean): boolean {
  if (!mine) return false
  if (isOptimisticMessage(message)) return false
  if (messageKindOf(message) === 'recalled') return false
  const realId = String(message?.messageId || '').trim()
  if (!realId) return false
  const ts = Number(message?.ts) || 0
  if (!ts) return false
  return Date.now() - ts <= RECALL_WINDOW_MS
}

/// 「重新编辑」资格：刚撤回（kind=recalled）、有原文本、自己发的、2 分钟内。
/// **仅对纯文本撤回有效**——图片/位置/商品撤回不允许重编（没法 put image
/// 回输入框，卡片也不是可编辑文本）。判断方式：原图 URL 必须为空。
/// 优先用前端 React state 里的 `originalText`（撤回瞬间 stash），没有就用
/// 后端持久化的 `recalledOriginalText`。
export function canReeditRecalled(message: any): boolean {
  if (messageKindOf(message) !== 'recalled') return false
  if (!message?.isSelf) return false
  const original = recallOriginalContent(message)
  // 文本撤回：text 非空 + imageUrl 空。
  // 图片撤回：text 空 + imageUrl 非空 → 屏蔽。
  // 卡片撤回（商品/位置）：text 是 title + imageUrl 是缩略图 → 屏蔽。
  if (!original.text) return false
  if (original.imageUrl) return false
  const recalledAt = Number(message?.recalledAt) || Number(message?.recalled_at) || 0
  if (!recalledAt) return false
  return Date.now() - recalledAt <= REEDIT_WINDOW_MS
}

/// 反撤回价值：撤回前的原内容（用户能看到对方撤回了什么）。
/// 两个来源——前端 stash 的 `originalText`（recall 操作瞬间记录的，最及时）+
/// 后端持久化的 `recalledOriginalText`/`recalledOriginalImageUrl`
/// （reload 之后还能看到，对方撤回的也走这条）。
export function recallOriginalContent(message: any): { text: string; imageUrl: string } {
  if (!message) return { text: '', imageUrl: '' }
  const text = String(
    message.originalText
    ?? message.recalledOriginalText
    ?? message.recalled_original_text
    ?? '',
  )
  const imageUrl = String(
    message.recalledOriginalImageUrl
    ?? message.recalled_original_image_url
    ?? '',
  )
  return { text, imageUrl }
}

/// 渲染条件：撤回胶囊下面要不要插原内容。
/// **只对接收方（!isSelf）显示**——这条是反撤回的核心价值：你想看对方撤了啥。
/// 发送方自己撤的不显示原内容（写的人自己记得；想重发用「重新编辑」按钮就行）。
export function hasRecallOriginal(message: any): boolean {
  if (messageKindOf(message) !== 'recalled') return false
  if (message?.isSelf) return false
  const { text, imageUrl } = recallOriginalContent(message)
  return Boolean(text || imageUrl)
}

/// 引用回复：从消息上扒出 quote 头要渲染的内容（PNM + 文本）。
/// 一份逻辑给桌面 + 移动两边复用。`replyToText` 上游不下发——发送时由发送方
/// stash 一份；接收方拿到 push 时只有 PNM，所以 fallback：在 `messages` 数组
/// 里反查同 PNM 行的 preview/text。找不到就用 [图片] 占位（最常见 case 是
/// 引用一张图片或被卡片样消息）。
export function getReplyQuote(
  message: any,
  allMessages?: any[],
): { messageId: string; text: string } | null {
  if (!message) return null
  const pnm = String(message?.replyToMessageId || message?.reply_to_message_id || '').trim()
  if (!pnm) return null
  let text = String(message?.replyToText || message?.reply_to_text || '').trim()
  if (!text && Array.isArray(allMessages)) {
    const target = allMessages.find(
      (m: any) => String(m?.upstreamMessageId || m?.upstream_message_id || '').trim() === pnm,
    )
    if (target) {
      text = String(
        target?.text ?? target?.preview ?? target?.title ?? '',
      ).trim()
      if (!text) {
        const imageUrl = String(target?.imageUrl ?? '').trim()
        if (imageUrl) text = '[图片]'
      }
    }
  }
  if (!text) text = '[图片]'
  return { messageId: pnm, text }
}

/// 已读/未读 chip 应不应该出现 + 是否已读。
/// 数据契约（抓包确认）：
///   - sessionType=1（聊天会话）才显示；3/6/53 等系统通道不显示
///   - 必须是自己发出（mine=true），有真实 server version，非乐观、非撤回
///   - message.unreadCount = 0 → 对方已读；>0 → 对方未读
export function computeReadHint(args: {
  message: any
  mine: boolean
  sessionType: number
}): { show: boolean; isRead: boolean } {
  const { message, mine, sessionType } = args
  const msgVersion = Number(message?.version) || 0
  const peerUnread = Number(message?.unreadCount)
  const unreadKnown = Number.isFinite(peerUnread) && peerUnread >= 0
  const show = Boolean(
    mine
      && sessionType === 1
      && msgVersion > 0
      && !isOptimisticMessage(message)
      && messageKindOf(message) !== 'recalled'
      && unreadKnown
  )
  return { show, isRead: peerUnread === 0 }
}
