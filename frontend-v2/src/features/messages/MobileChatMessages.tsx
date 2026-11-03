// @ts-nocheck
/**
 * Pure presentational sub-components for MobileChat —— 消息列表 + 底部 ActionSheet
 * 抽出来让 MobileChat 主组件保持在 500 行以内，渲染逻辑完全复用桌面端的
 * MessageRenderers，仅 UI 细节（左右气泡 / 长按手势）按触屏适配。
 */
import { Bell } from 'lucide-react'

import {
  isStickerMessage, isSystemStyleMessage,
  renderInteractionItem, renderMessageContent,
  renderNotificationItem, renderSystemMessageBody,
} from './MessageRenderers'
import { formatMessageTime, resolveMessageIsMine } from './messagesConstants'
import { MessageAvatar, resolvePeerAvatarUrl, resolveSelfAvatarUrl } from './MessageAvatar'
import { SafeRemoteImage } from './SafeRemoteImage'
import {
  canReeditRecalled,
  computeReadHint,
  getReplyQuote,
  hasRecallOriginal,
  recallOriginalContent,
} from './messageBubbleLogic'

interface ActionSheetState {
  message: any
  mine: boolean
  canRecall: boolean
}

export function MobileMessageList({
  messages,
  loading,
  uid,
  sessionId,
  sessionType,
  peerNick,
  peerUid,
  peerLogo,
  selfNick,
  selfAvatar,
  bottomRef,
  onLongPressStart,
  onLongPressMove,
  onLongPressEnd,
  onOpenSheet,
  canRecallMessage,
  onReedit,
}: {
  messages: any[]
  loading: boolean
  uid: string
  sessionId: string
  sessionType: number
  peerNick: string
  peerUid: string
  peerLogo?: string
  /// 自己一侧头像：当前账号的 nick + avatar，由 MobileChat 透传 initialSession 拆出。
  selfNick?: string
  selfAvatar?: string
  bottomRef: any
  onLongPressStart: (e: React.TouchEvent, message: any, mine: boolean) => void
  onLongPressMove: (e: React.TouchEvent) => void
  onLongPressEnd: () => void
  onOpenSheet: (state: ActionSheetState) => void
  canRecallMessage: (message: any, mine: boolean) => boolean
  /// 「重新编辑」点击：拿撤回前的原文本回填到输入框。MobileChat 透传 setComposerText。
  onReedit?: (originalText: string) => void
}) {
  return (
    <div className="xianyu-chat-messages min-h-0 flex-1 overflow-y-auto">
      {loading && !messages.length && (
        <div className="py-10 text-center text-[13px]" style={{ color: 'var(--text-secondary)' }}>加载中…</div>
      )}
      <div className="flex flex-col gap-4">
        {messages.map((msg, i) => {
        const key = msg?.messageId || `${msg?.ts || 0}_${i}`
        const sessionLite = { sessionId, sessionType, peerNick, peerUid, peerLogo }

        // sessionType=3 通知消息 —— 全宽卡片
        if (sessionType === 3) {
          return (
            <div key={key} className="flex w-full flex-col gap-1.5">
              <span className="text-center text-[10.5px]" style={{ color: 'var(--text-muted)' }}>
                {formatMessageTime(msg?.ts)}
              </span>
              {renderNotificationItem(msg, sessionLite)}
            </div>
          )
        }
        // sessionType=6 互动消息 —— 紧凑列表行
        if (sessionType === 6) {
          return (
            <div key={key} className="flex w-full flex-col gap-1.5">
              <span className="text-[10.5px]" style={{ color: 'var(--text-muted)' }}>
                {formatMessageTime(msg?.ts)}
              </span>
              {renderInteractionItem(msg, sessionLite)}
            </div>
          )
        }
        // 系统提示 / 撤回 / 平台卡片 —— 居中灰胶囊
        if (isSystemStyleMessage(msg, sessionLite)) {
          const senderIcon = String(msg?.senderLogo ?? msg?.sender_logo ?? '').trim()
          const reeditable = canReeditRecalled(msg)
          const showRecallOriginal = hasRecallOriginal(msg)
          const original = showRecallOriginal ? recallOriginalContent(msg) : { text: '', imageUrl: '' }
          return (
            <div key={key} className="flex flex-col items-center gap-1.5">
              <div
                className="flex max-w-[86%] flex-col gap-2 rounded-[20px] px-4 py-3 text-[12.5px] leading-5 break-words"
                style={{
                  background: 'var(--surface-card)',
                  color: 'var(--text-secondary)',
                  border: '1px solid var(--glass-border)',
                  boxShadow: 'var(--glass-shadow, 0 12px 28px rgba(17,24,39,0.08))',
                  whiteSpace: 'pre-wrap',
                }}
              >
                <div className="flex items-start gap-2.5">
                  {senderIcon ? (
                    <SafeRemoteImage
                      src={senderIcon}
                      alt=""
                      className="h-6 w-6 shrink-0 rounded-full object-cover"
                      fallback={(
                        <div
                          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full"
                          style={{ background: 'var(--mobile-glass-surface)', border: '1px solid var(--glass-border)' }}
                        >
                          <Bell className="h-3.5 w-3.5" style={{ color: 'var(--text-secondary)' }} />
                        </div>
                      )}
                    />
                  ) : (
                    <div
                      className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full"
                      style={{ background: 'var(--mobile-glass-surface)', border: '1px solid var(--glass-border)' }}
                    >
                      <Bell className="h-3.5 w-3.5" style={{ color: 'var(--text-secondary)' }} />
                    </div>
                  )}
                  <div className="min-w-0 flex-1 text-left">
                    {renderSystemMessageBody(msg)}
                    {reeditable && onReedit ? (
                      <button
                        type="button"
                        onClick={() => onReedit(recallOriginalContent(msg).text)}
                        className="ml-1.5 underline"
                        style={{ color: 'var(--primary)', background: 'transparent', padding: 0 }}
                      >
                        重新编辑
                      </button>
                    ) : null}
                  </div>
                </div>
                {/* 反撤回：胶囊下方塞原内容（本端存档，仅给你看） */}
                {showRecallOriginal ? (
                  <div
                    className="ml-[34px] rounded-[14px] px-3 py-2.5 text-[12px] leading-5"
                    style={{
                      backgroundColor: 'var(--mobile-glass-surface)',
                      color: 'var(--text-main)',
                      border: '1px dashed var(--glass-border)',
                      whiteSpace: 'pre-wrap',
                    }}
                  >
                    <div className="mb-1 text-[10.5px]" style={{ color: 'var(--text-muted)' }}>
                      撤回前内容
                    </div>
                    {original.imageUrl ? (
                      <SafeRemoteImage
                        src={original.imageUrl}
                        alt="原消息图片"
                        className="max-h-48 max-w-full rounded-md object-contain"
                        style={{ border: '1px solid var(--glass-border)' }}
                        fallback={null}
                      />
                    ) : null}
                    {original.text ? (
                      <div className="break-words">{original.text}</div>
                    ) : null}
                  </div>
                ) : null}
              </div>
              <span className="text-[10.5px]" style={{ color: 'var(--text-muted)' }}>
                {formatMessageTime(msg?.ts)}
              </span>
            </div>
          )
        }

        // 普通 1v1 气泡 —— 左右分布 + 长按弹 ActionSheet
        const mine = resolveMessageIsMine(msg, uid)
        const sticker = isStickerMessage(msg)
        const bubbleClass = sticker
          ? 'break-words'
          : `xianyu-chat-bubble break-words ${mine ? 'xianyu-chat-bubble-mine' : 'xianyu-chat-bubble-peer'}`
        const bubbleStyle = sticker
          ? { whiteSpace: 'pre-wrap' as const }
          : (mine
              ? {
                whiteSpace: 'pre-wrap' as const,
              }
              : {
                whiteSpace: 'pre-wrap' as const,
              })

        // 已读/未读 chip — 走共享 helper（messageBubbleLogic.ts），跟桌面一字不差
        const { show: showReadHint, isRead } = computeReadHint({ message: msg, mine, sessionType })
        const peerAvatarUrl = resolvePeerAvatarUrl(msg, sessionLite)
        const selfAvatarUrl = resolveSelfAvatarUrl({ _accountAvatar: selfAvatar })
        // 引用回复：跟桌面端复用同一 helper，气泡上方画一个浅灰小块。
        const replyQuote = getReplyQuote(msg, messages)

        return (
          <div key={key} className={`flex ${mine ? 'justify-end' : 'justify-start'} items-end gap-2.5`}>
            {!mine && (
              <MessageAvatar src={peerAvatarUrl} label={peerNick} kind="peer" size={38} />
            )}
            <div className={`flex max-w-[80%] flex-col gap-1.5 ${mine ? 'items-end' : 'items-start'}`}>
              {replyQuote ? (
                <div
                  style={{
                    maxWidth: '100%',
                    padding: '6px 10px 6px 12px',
                    fontSize: 12,
                    lineHeight: 1.3,
                    color: 'var(--text-secondary)',
                    background: 'var(--mobile-glass-surface)',
                    borderLeft: '3px solid color-mix(in srgb, var(--primary) 28%, transparent)',
                    borderRadius: 12,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                  title={replyQuote.text}
                >
                  {replyQuote.text}
                </div>
              ) : null}
              <div
                className={bubbleClass}
                style={bubbleStyle}
                // 触屏长按 / 桌面端右键 —— 都打开 ActionSheet
                onTouchStart={(e) => onLongPressStart(e, msg, mine)}
                onTouchMove={onLongPressMove}
                onTouchEnd={onLongPressEnd}
                onTouchCancel={onLongPressEnd}
                onContextMenu={(e) => {
                  e.preventDefault()
                  onOpenSheet({ message: msg, mine, canRecall: canRecallMessage(msg, mine) })
                }}
              >
                {renderMessageContent(msg, sessionLite)}
              </div>
              <div className="flex items-center gap-1.5 px-1">
                <span className="text-[10px]" style={{ color: 'var(--text-muted)' }}>
                  {formatMessageTime(msg?.ts)}
                </span>
                {showReadHint ? (
                  <span
                    className="rounded-full px-2 py-0.5 text-[10px] font-medium"
                    style={{
                      color: isRead ? 'var(--text-muted)' : 'var(--danger)',
                      background: isRead ? 'var(--mobile-glass-surface)' : 'color-mix(in srgb, var(--danger) 10%, transparent)',
                    }}
                    aria-label={isRead ? '对方已读' : '对方未读'}
                  >
                    {isRead ? '已读' : '未读'}
                  </span>
                ) : null}
              </div>
            </div>
            {mine && (
              <MessageAvatar src={selfAvatarUrl} label={String(selfNick || '')} kind="self" size={38} />
            )}
          </div>
        )
        })}
      </div>
      <div ref={bottomRef} />
    </div>
  )
}

// 长按消息弹出的 ActionSheet —— iOS 风格底部弹层。
// 复制 / 回复对所有消息可用；撤回仅对自己 2 分钟内发送的非乐观消息（canRecall）。
export function MessageActionSheet({
  state,
  onClose,
  onCopy,
  onReply,
  onRecall,
}: {
  state: ActionSheetState | null
  onClose: () => void
  onCopy: (message: any) => void
  onReply?: (message: any) => void
  onRecall: (message: any) => void
}) {
  if (!state) return null
  // 引用回复必须能拿到 PNM——拿不到（极早期 / 数字 sync）就把 button 隐掉
  const replyPnm = String(
    state.message?.upstreamMessageId || state.message?.upstream_message_id || '',
  ).trim()
  const canReply = Boolean(onReply) && Boolean(replyPnm)
  return (
    <div
      className="fixed inset-0 z-[80] flex flex-col justify-end"
      style={{ backgroundColor: 'color-mix(in srgb, var(--text-main) 34%, transparent)', backdropFilter: 'blur(8px)', WebkitBackdropFilter: 'blur(8px)' }}
      onClick={onClose}
    >
      <div
        className="mx-3 mb-3 overflow-hidden rounded-[28px]"
        style={{
          background: 'var(--mobile-glass-surface-strong, var(--mobile-glass-surface))',
          paddingBottom: 'env(safe-area-inset-bottom)',
          border: '1px solid var(--glass-border)',
          boxShadow: 'var(--glass-shadow, 0 24px 60px rgba(17,24,39,0.16))',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex justify-center px-4 pt-3">
          <span className="h-1.5 w-12 rounded-full" style={{ background: 'var(--glass-border)' }} />
        </div>
        <button
          onClick={() => onCopy(state.message)}
          className="w-full cursor-pointer px-4 py-4 text-center text-[15px]"
          style={{ color: 'var(--text-main)', borderBottom: '1px solid var(--glass-border)' }}
        >
          复制
        </button>
        {canReply && (
          <button
            onClick={() => onReply?.(state.message)}
            className="w-full cursor-pointer px-4 py-4 text-center text-[15px]"
            style={{
              color: 'var(--text-main)',
              borderBottom: state.canRecall ? '1px solid var(--glass-border)' : undefined,
            }}
          >
            回复
          </button>
        )}
        {state.canRecall && (
          <button
            onClick={() => onRecall(state.message)}
            className="w-full cursor-pointer px-4 py-4 text-center text-[15px]"
            style={{ color: 'var(--danger)' }}
          >
            撤回
          </button>
        )}
      </div>
      <button
        onClick={onClose}
        className="mx-3 mb-3 cursor-pointer rounded-[22px] py-4 text-center text-[15px]"
        style={{
          background: 'var(--mobile-glass-surface-strong, var(--mobile-glass-surface))',
          color: 'var(--primary)',
          paddingBottom: 'calc(1rem + env(safe-area-inset-bottom))',
          border: '1px solid var(--glass-border)',
          boxShadow: 'var(--glass-shadow, 0 18px 40px rgba(17,24,39,0.12))',
        }}
      >
        取消
      </button>
    </div>
  )
}
