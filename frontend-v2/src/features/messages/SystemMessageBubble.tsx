// @ts-nocheck
// System-style message capsule (recall notices, refunds, etc.) — extracted
// from MessagePanels.tsx so the conversation panel stays under the file-size
// budget. Pure presentational; props mirror the inline render path.
import { Bell } from 'lucide-react'

import { canReeditRecalled, hasRecallOriginal, recallOriginalContent } from './messageBubbleLogic'
import { BRAND, formatMessageTime } from './messagesConstants'
import { renderSystemMessageBody } from './MessageRenderers'
import { SafeRemoteImage } from './SafeRemoteImage'

interface SystemMessageBubbleProps {
  message: any
  onReedit: (text: string) => void
}

export function SystemMessageBubble({ message, onReedit }: SystemMessageBubbleProps) {
  const senderIcon = String(message?.senderLogo ?? message?.sender_logo ?? '').trim()
  const reeditable = canReeditRecalled(message)
  const showRecallOriginal = hasRecallOriginal(message)
  const original = showRecallOriginal ? recallOriginalContent(message) : { text: '', imageUrl: '' }

  return (
    <div className="flex flex-col items-center gap-1">
      <div
        className="flex max-w-[85%] flex-col gap-1.5 rounded-[14px] px-3.5 py-2.5 text-[12.5px] leading-5 break-words"
        style={{
          backgroundColor: 'var(--surface-soft)',
          color: BRAND.textSecondary,
          border: `1px solid ${BRAND.border}`,
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
                  style={{ background: 'var(--surface-soft)', border: `1px solid ${BRAND.border}` }}
                >
                  <Bell className="h-3.5 w-3.5" style={{ color: BRAND.textSecondary }} />
                </div>
              )}
            />
          ) : (
            <div
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full"
              style={{ background: 'var(--surface-soft)', border: `1px solid ${BRAND.border}` }}
            >
              <Bell className="h-3.5 w-3.5" style={{ color: BRAND.textSecondary }} />
            </div>
          )}
          <div className="min-w-0 flex-1 text-left">
            {renderSystemMessageBody(message)}
            {reeditable ? (
              <button
                type="button"
                onClick={() => {
                  onReedit(recallOriginalContent(message).text)
                  // Hand focus to the composer so the user can edit / send
                  // immediately. Composer placeholders differ between mobile
                  // and desktop; the *contains* selector matches both.
                  setTimeout(() => {
                    const ta = document.querySelector<HTMLTextAreaElement>('textarea[placeholder*="输入消息"]')
                    if (ta) { ta.focus(); ta.scrollIntoView({ block: 'end' }) }
                  }, 30)
                }}
                className="ml-1.5 underline cursor-pointer"
                style={{ color: BRAND.primary, background: 'transparent', padding: 0 }}
              >
                重新编辑
              </button>
            ) : null}
          </div>
        </div>
        {/* 反撤回：胶囊下方塞原内容（仅本端存档可见，闲鱼/对方那边看不到） */}
        {showRecallOriginal ? (
          <div
            className="ml-[34px] rounded-[10px] px-2.5 py-2 text-[12px] leading-5"
            style={{
              backgroundColor: 'rgba(255,255,255,0.58)',
              color: BRAND.textMain,
              border: `1px dashed ${BRAND.border}`,
              whiteSpace: 'pre-wrap',
            }}
          >
            <div className="mb-1 text-[10.5px]" style={{ color: BRAND.textTertiary }}>
              撤回前内容
            </div>
            {original.imageUrl ? (
              <SafeRemoteImage
                src={original.imageUrl}
                alt="原消息图片"
                className="max-h-48 max-w-full rounded-md object-contain"
                style={{ border: `1px solid ${BRAND.border}` }}
                fallback={null}
              />
            ) : null}
            {original.text ? (
              <div className="break-words">{original.text}</div>
            ) : null}
          </div>
        ) : null}
      </div>
      <span className="text-[11px]" style={{ color: BRAND.textTertiary }}>
        {formatMessageTime(message?.ts)}
      </span>
    </div>
  )
}
