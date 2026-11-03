// @ts-nocheck
/**
 * Pure presentational panel components for AggregateMessages.
 * Each panel receives all needed data/callbacks as props — no internal state.
 */
import { Inbox, MessageSquare, Package, RefreshCw, SendHorizontal } from 'lucide-react'

import { EmptyState } from '../../components/EmptyState'
import { SensitiveValue, SensitiveVisibilityToggle, maskSensitiveText, useSensitiveAvatarInitial, useSensitiveHidden, useSensitiveText } from '../../components/SensitiveValue'
import { ComposerActionStrip } from './ComposerActionStrip'
import { SystemMessageBubble } from './SystemMessageBubble'
import { colorForAccount } from './accountColor'
import {
  BRAND,
  PANEL_STYLE,
  STICKY_GROUP_STYLE,
  accountLabel,
  formatMessageTime,
  peerLabel,
  resolveMessageIsMine,
  sessionKey,
  sessionType,
} from './messagesConstants'
import {
  isStickerMessage,
  isSystemStyleMessage,
  renderInteractionItem,
  renderMessageContent,
  renderNotificationItem,
} from './MessageRenderers'
import { MessageAvatar, resolvePeerAvatarUrl, resolveSelfAvatarUrl } from './MessageAvatar'
import { RichComposerTextarea } from './RichComposerTextarea'
import { SafeRemoteImage } from './SafeRemoteImage'
import {
  canRecallMessage,
  computeReadHint,
  getReplyQuote,
  isOptimisticMessage,
} from './messageBubbleLogic'
import { countAccountUnread } from './platformUnread'
import { SessionRow, renderSessionAvatar } from './SessionRow'

// ─── Left panel: account list ──────────────────────────────────────────────

export function AccountsPanel({
  activeAccounts,
  selectedAccountUid,
  setSelectedAccountUid,
  sessionsByUid,
  platformMessagesEnabled,
  totalUnread,
}: {
  activeAccounts: any[]
  selectedAccountUid: string
  setSelectedAccountUid: (uid: string) => void
  sessionsByUid: Record<string, any[]>
  platformMessagesEnabled: boolean
  totalUnread: number
}) {
  return (
    <aside
      className="flex min-h-0 flex-col overflow-hidden rounded-[24px] border"
      style={PANEL_STYLE}
    >
      <div className="h-1.5" style={{ background: BRAND.gradient }} />

      <div className="border-b px-4 py-4" style={{ borderColor: BRAND.border }}>
        <p className="text-[12px] font-medium uppercase tracking-[0.18em]" style={{ color: BRAND.textTertiary }}>
          账号筛选
        </p>
        <button
          onClick={() => setSelectedAccountUid('all')}
              className="mt-3 flex w-full cursor-pointer items-center justify-between rounded-2xl px-3 py-3 text-left transition-colors hover:bg-[var(--surface-soft)]"
          style={selectedAccountUid === 'all'
            ? { backgroundColor: BRAND.selected }
            : { backgroundColor: 'transparent' }}
        >
          <div className="flex items-center gap-3">
            <div
              className="flex h-11 w-11 items-center justify-center rounded-full"
              style={{ background: BRAND.gradient, color: BRAND.textMain }}
            >
              <Inbox className="h-4.5 w-4.5" />
            </div>
            <div className="min-w-0">
              <div className="truncate text-[14px] font-semibold" style={{ color: BRAND.textMain }}>全部账号</div>
              <div className="truncate text-[12px]" style={{ color: BRAND.textSecondary }}>{activeAccounts.length} 个可收消息账号</div>
            </div>
          </div>
          {totalUnread > 0 && (
            <span className="rounded-full px-2 py-0.5 text-[11px] font-semibold text-white" style={{ backgroundColor: BRAND.accent }}>
              {totalUnread > 99 ? '99+' : totalUnread}
            </span>
          )}
        </button>
      </div>

      <div className="desktop-scroll-hidden min-h-0 flex-1 overflow-y-auto px-3 py-3">
        <div className="space-y-2">
          {activeAccounts.map((account) => {
            const ringColor = colorForAccount(account.uid)
            const unread = countAccountUnread(
              sessionsByUid[account.uid] || [],
              platformMessagesEnabled,
            )
            const isActive = selectedAccountUid === account.uid

            return (
              <button
                key={account.uid}
                onClick={() => setSelectedAccountUid(account.uid)}
                className="flex w-full cursor-pointer items-center gap-3 rounded-2xl px-3 py-3 text-left transition-colors hover:bg-[var(--surface-soft)]"
                style={isActive ? { backgroundColor: BRAND.selected } : { backgroundColor: 'transparent' }}
              >
                <div className="rounded-full p-[2px]" style={{ backgroundColor: ringColor }}>
                  {account.avatar ? (
                    <SafeRemoteImage
                      src={account.avatar}
                      alt=""
                      className="h-10 w-10 rounded-full object-cover"
                      style={{ backgroundColor: 'var(--surface-card-strong)' }}
                      fallback={(
                        <AccountAvatarFallback value={account.nick || account.uid} color={ringColor} />
                      )}
                    />
                  ) : (
                    <AccountAvatarFallback value={account.nick || account.uid} color={ringColor} />
                  )}
                </div>

                <div className="min-w-0 flex-1">
                  <div className="truncate text-[14px] font-semibold" style={{ color: BRAND.textMain }}>
                    <SensitiveValue kind="account" value={account.nick || account.uid} />
                  </div>
                  <div className="truncate text-[12px]" style={{ color: BRAND.textSecondary }}>
                    <SensitiveValue kind="uid" value={account.uid} />
                  </div>
                </div>

                {unread > 0 && (
                  <span className="rounded-full px-2 py-0.5 text-[11px] font-semibold text-white" style={{ backgroundColor: BRAND.accent }}>
                    {unread > 99 ? '99+' : unread}
                  </span>
                )}
              </button>
            )
          })}
        </div>
      </div>

      {/* Filter bar removed */}
    </aside>
  )
}

function AccountAvatarFallback({ value, color }: { value: string; color: string }) {
  const initial = useSensitiveAvatarInitial(value)
  return <div className="liquid-avatar flex h-10 w-10 items-center justify-center rounded-full text-[14px] font-semibold" style={{ color }}>{initial}</div>
}

// ─── Middle panel: session list ─────────────────────────────────────────────

export function SessionListPanel({
  activeAccounts,
  selectedAccountUid,
  totalUnread,
  filteredSessions,
  pinnedSessions,
  groupedSessions,
  selectedSessionId,
  selectSession,
  refreshing,
  onRefresh,
}: {
  activeAccounts: any[]
  selectedAccountUid: string
  totalUnread: number
  filteredSessions: any[]
  pinnedSessions: any[]
  groupedSessions: any[]
  selectedSessionId: string
  selectSession: (session: any) => void
  refreshing: boolean
  onRefresh: () => void
}) {
  return (
    <section
      className="flex min-h-0 flex-col overflow-hidden rounded-[24px] border"
      style={PANEL_STYLE}
    >
      <div className="h-1.5" style={{ background: BRAND.gradient }} />

      <div className="border-b px-5 py-4" style={{ borderColor: BRAND.border }}>
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-[17px] font-semibold" style={{ color: BRAND.textMain }}>
              {selectedAccountUid === 'all' ? '全部消息' : (activeAccounts.find((a) => a.uid === selectedAccountUid)?.nick || selectedAccountUid)}
              {totalUnread > 0 ? (
                <span className="ml-2 align-middle rounded-full px-2 py-0.5 text-[11px] font-semibold text-white" style={{ backgroundColor: BRAND.accent }}>
                  {totalUnread > 99 ? '99+' : totalUnread}
                </span>
              ) : null}
            </h2>
            <p className="mt-1 text-[12px]" style={{ color: BRAND.textSecondary }}>
              {selectedAccountUid === 'all' ? '跨账号汇总最近消息' : '当前筛选账号的消息'}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <SensitiveVisibilityToggle
              compact
              className="inline-flex h-8 w-8 items-center justify-center rounded-full border transition-opacity hover:opacity-80"
            />
            <button
              onClick={onRefresh}
              disabled={refreshing}
              className="inline-flex cursor-pointer items-center gap-1.5 rounded-full border px-3 py-1.5 text-[12px] font-semibold transition-opacity disabled:opacity-50"
              style={{
                backgroundColor: BRAND.card,
                borderColor: BRAND.border,
                color: BRAND.textMain,
              }}
              aria-label="刷新"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? 'animate-spin' : ''}`} />
              刷新
            </button>
          </div>
        </div>
      </div>

      <div className="desktop-scroll-hidden min-h-0 flex-1 overflow-y-auto">
        {!filteredSessions.length ? (
          <div className="flex h-full items-center justify-center px-6">
            <EmptyState icon={Inbox} title="暂无会话" description="调整账号或类型筛选后再试" />
          </div>
        ) : (
          <div className="pb-3">
            {pinnedSessions.length > 0 && (
              <div>
                {pinnedSessions.map((session) => (
                  <SessionRow
                    key={sessionKey(session)}
                    session={session}
                    active={selectedSessionId === sessionKey(session)}
                    onSelect={selectSession}
                  />
                ))}
              </div>
            )}
            {groupedSessions.map((group) => (
              <div key={group.label}>
                <div
                  className="sticky top-0 z-10 border-y px-5 py-2 text-[11px] font-semibold uppercase tracking-[0.18em]"
                  style={STICKY_GROUP_STYLE}
                >
                  {group.label}
                </div>

                {group.sessions.map((session) => (
                  <SessionRow
                    key={sessionKey(session)}
                    session={session}
                    active={selectedSessionId === sessionKey(session)}
                    onSelect={selectSession}
                  />
                ))}
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  )
}

// 回复按钮闸门：撤回过 / 贴纸 / 卡片（位置/商品）/ 已被 dxCard 包成系统消息的不让回复——
// 卡片信息没法被合理 quote（位置发出去是张地图，商品是 itemId+pic），用户也
// 没需求引用一张位置或商品过去。只允许文本 / 图片回复。
function messageKindGuard(message: any): boolean {
  const kind = String(message?.kind || '')
  if (kind === 'recalled') return false
  if (kind === 'sticker') return false
  if (kind === 'card') return false
  // contentType 7/8 (item), 30 (location) 即使被某些路径 normalize 没塞 kind=card
  // 也得拦掉——直接看 cardMeta.kind 兜底。
  const cardMeta: any = (message && typeof message.cardMeta === 'object') ? message.cardMeta : null
  if (cardMeta && (cardMeta.kind === 'item' || cardMeta.kind === 'location')) return false
  const ct = Number(message?.contentType ?? message?.content_type ?? 0)
  if (ct === 7 || ct === 8 || ct === 30) return false
  return true
}

// ─── Right panel: conversation detail ──────────────────────────────────────

export function ConversationPanel({
  selectedSession,
  selectedItem,
  selectedMessages,
  selectedHistory,
  selectedLoadingState,
  headinfoBySessionId,
  itemPriceByItemId,
  detailScrollRef,
  imageInputRef,
  composerText,
  setComposerText,
  sending,
  onImagePick,
  onSend,
  onSendLocation,
  onSendItem,
  onRecallMessage,
  replyTarget,
  setReplyTarget,
}: {
  selectedSession: any
  selectedItem: any
  selectedMessages: any[]
  selectedHistory: any
  selectedLoadingState: any
  headinfoBySessionId: Record<string, any>
  itemPriceByItemId: Record<string, string>
  detailScrollRef: any
  imageInputRef: any
  composerText: string
  setComposerText: (t: string) => void
  sending: boolean
  onImagePick: (e: React.ChangeEvent<HTMLInputElement>) => void
  onSend: () => void
  onSendLocation?: (poi: { latitude: string; longitude: string; title: string; content: string }) => Promise<void> | void
  onSendItem?: (item: { itemId: string; title: string; mainPic: string; price: string }) => Promise<void> | void
  onRecallMessage?: (message: any) => void
  replyTarget?: { messageId: string; text: string } | null
  setReplyTarget?: (v: { messageId: string; text: string } | null) => void
}) {
  const isSensitiveHidden = useSensitiveHidden()
  const safePeerLabel = useSensitiveText('buyer', peerLabel(selectedSession || {}))
  const safeAccountLabel = useSensitiveText('account', accountLabel(selectedSession || {}))
  // Peer-read indicator comes from `message.unreadCount` on each message
  // (confirmed via packet capture): for outgoing messages, 0 = peer 已读,
  // >0 = peer 未读. Incoming messages also carry this field but it represents
  // OUR unread state, so we only render the chip for outgoing bubbles.
  if (!selectedSession) {
    return (
      <section
        className="flex min-h-0 flex-col overflow-hidden rounded-[24px] border"
        style={PANEL_STYLE}
      >
        <div className="h-1.5" style={{ background: BRAND.gradient }} />
        <div className="flex h-full items-center justify-center px-6">
          <EmptyState icon={MessageSquare} title="选择会话" description="从中栏消息列表中打开一个会话" />
        </div>
      </section>
    )
  }

  const hi = selectedSession?.sessionId ? headinfoBySessionId[selectedSession.sessionId] : null
  const itemPic = (hi?.picUrl || selectedItem?.pic) || ''
  const itemTitle = (hi?.desc || selectedItem?.title) || '关联商品'
  const itemId = selectedItem?.itemId || String(selectedSession?.itemId ?? '').trim()
  const safeItemTitle = isSensitiveHidden ? maskSensitiveText('item-title', itemTitle) : itemTitle

  return (
    <section
      className="flex min-h-0 flex-col overflow-hidden rounded-[24px] border"
      style={PANEL_STYLE}
    >
      <div className="h-1.5" style={{ background: BRAND.gradient }} />

      {/* Header: peer info + item card */}
      <div className="border-b px-5 py-4" style={{ borderColor: BRAND.border }}>
        <div className="flex items-start gap-3">
          <div className="rounded-full p-[2px]" style={{ backgroundColor: colorForAccount(selectedSession._uid) }}>
            {renderSessionAvatar(selectedSession, 'h-12 w-12')}
          </div>

          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="truncate text-[18px] font-semibold" style={{ color: BRAND.textMain }}>
                {safePeerLabel}
              </h2>
              <span
                className="max-w-[160px] truncate rounded-full px-2.5 py-0.5 text-[11px] font-semibold text-white"
                style={{ backgroundColor: colorForAccount(selectedSession._uid) }}
              >
                {safeAccountLabel}
              </span>
            </div>

            {(selectedItem || hi) && (
              <div
                className="mt-3 rounded-xl border p-3 transition-colors hover:bg-black/[0.03]"
                style={{ backgroundColor: BRAND.card, borderColor: BRAND.border }}
              >
                <div className="flex items-center gap-3">
                  {itemPic ? (
                    <SafeRemoteImage
                      src={itemPic}
                      alt={safeItemTitle}
                      className="h-14 w-14 rounded-xl border object-cover"
                      sensitiveItem
                      style={{ borderColor: BRAND.border }}
                      fallback={(
                        <div
                          className="flex h-14 w-14 items-center justify-center rounded-xl"
                          style={{ backgroundColor: BRAND.selected, color: BRAND.accent }}
                        >
                          <Package className="h-5 w-5" />
                        </div>
                      )}
                    />
                  ) : (
                    <div
                      className="flex h-14 w-14 items-center justify-center rounded-xl"
                      style={{ backgroundColor: BRAND.selected, color: BRAND.accent }}
                    >
                      <Package className="h-5 w-5" />
                    </div>
                  )}

                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[14px] font-semibold" style={{ color: BRAND.textMain }}>
                      <SensitiveValue kind="item-title" value={itemTitle} />
                    </div>
                    {(() => {
                      const resolvedPrice = (hi?.price)
                        || selectedItem?.price
                        || (itemId ? itemPriceByItemId[itemId] : '')
                      if (resolvedPrice) {
                        const display = /^\d+(\.\d+)?$/.test(resolvedPrice)
                          ? `¥${resolvedPrice}`
                          : resolvedPrice
                        return (
                          <div className="mt-1 flex items-center gap-2">
                            <span className="text-[14px] font-semibold" style={{ color: BRAND.accent }}>
                              <SensitiveValue kind="money" value={display} />
                            </span>
                            {hi?.promotionPrice ? (
                              <span
                                className="rounded px-1.5 py-0.5 text-[11px] font-semibold"
                                style={{ backgroundColor: BRAND.accent, color: 'var(--liquid-highlight)' }}
                              >
                                <SensitiveValue kind="money" value={hi.promotionPrice} />
                              </span>
                            ) : null}
                          </div>
                        )
                      }
                      return null
                    })()}
                    {hi?.subTitle || hi?.tips ? (
                      <div className="mt-1 truncate text-[12px]" style={{ color: BRAND.textTertiary }}>
                        {[hi.subTitle, hi.tips].filter(Boolean).join(' · ')}
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Message scroll area */}
      <div
        ref={detailScrollRef}
        className="desktop-scroll-hidden min-h-0 flex-1 overflow-y-auto px-4 py-4"
        style={{ backgroundColor: BRAND.page, overflowAnchor: 'none' }}
      >
        {selectedLoadingState?.loading ? (
          <div
            className="mb-3 rounded-full px-3 py-1.5 text-[12px] font-medium"
            style={{ backgroundColor: BRAND.selected, color: BRAND.textSecondary }}
          >
            加载中 · 已获取 {selectedLoadingState?.count || 0} 条
          </div>
        ) : null}

        {selectedHistory?.notice ? (
          <div
            className="mb-3 rounded-2xl border px-3 py-2 text-[12px]"
            style={{ backgroundColor: BRAND.selected, borderColor: BRAND.border, color: BRAND.textSecondary }}
          >
            {selectedHistory.notice}
          </div>
        ) : null}

        {!selectedMessages.length && selectedLoadingState?.loading ? (
          <div className="py-12 text-center text-[13px]" style={{ color: BRAND.textSecondary }}>正在加载消息…</div>
        ) : null}

        {!selectedMessages.length && !selectedLoadingState?.loading ? (
          <div className="py-12 text-center text-[13px]" style={{ color: BRAND.textSecondary }}>暂无历史消息</div>
        ) : null}

        <div className="space-y-3">
          {selectedMessages.map((message, index) => {
            const key = message?.messageId || `${message?.ts || 0}_${index}`
            const st = sessionType(selectedSession)

            if (st === 3) {
              return (
                <div key={key} className="flex w-full flex-col gap-1">
                  <span className="text-[11px] text-center" style={{ color: BRAND.textTertiary }}>
                    {formatMessageTime(message?.ts)}
                  </span>
                  {renderNotificationItem(message, selectedSession)}
                </div>
              )
            }

            if (st === 6) {
              return (
                <div key={key} className="flex w-full flex-col gap-1">
                  <span className="text-[11px]" style={{ color: BRAND.textTertiary }}>
                    {formatMessageTime(message?.ts)}
                  </span>
                  {renderInteractionItem(message, selectedSession)}
                </div>
              )
            }

            if (isSystemStyleMessage(message, selectedSession)) {
              return (
                <SystemMessageBubble
                  key={key}
                  message={message}
                  onReedit={(text) => setComposerText(text)}
                />
              )
            }

            const mine = resolveMessageIsMine(message, selectedSession._uid)
            const isSticker = isStickerMessage(message)
            const bubbleClass = isSticker
              ? 'break-words'
              : 'rounded-[22px] px-3.5 py-2.5 text-[14px] leading-6 break-words'
            const bubbleStyle = isSticker
              ? { whiteSpace: 'pre-wrap' as const }
              : (mine
                  ? { backgroundColor: BRAND.primary, color: 'var(--liquid-highlight)', whiteSpace: 'pre-wrap' as const }
                  : {
                      backgroundColor: BRAND.card,
                      color: BRAND.textMain,
                      border: `1px solid ${BRAND.border}`,
                      whiteSpace: 'pre-wrap' as const,
                    })
            const peerAvatarUrl = resolvePeerAvatarUrl(message, selectedSession)
            const selfAvatarUrl = resolveSelfAvatarUrl(selectedSession)

            // 撤回 / 已读未读 走共享 helper —— mobile / desktop 用同一份规则。
            // 改成「悬停旁边按钮」之后，右键 / 长按全部砍掉。原来用右键 contextmenu
            // 一堆坑（Tailwind 4 类不编译、Ctrl-Click 关闭闸门、portal 渲染时机），
            // 这条路径既看不见 affordance，又不便于在不同操作系统统一行为。
            const canRecall = Boolean(onRecallMessage) && canRecallMessage(message, mine)
            const isOptimistic = isOptimisticMessage(message)
            const { show: showReadHint, isRead } = computeReadHint({
              message,
              mine,
              sessionType: sessionType(selectedSession),
            })

            const recallButton = canRecall ? (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  onRecallMessage?.(message)
                }}
                aria-label="撤回这条消息"
                style={{
                  alignSelf: 'center',
                  padding: '4px 10px',
                  fontSize: 12,
                  color: BRAND.textSecondary,
                  background: BRAND.card,
                  border: `1px solid ${BRAND.border}`,
                  borderRadius: 999,
                  cursor: 'pointer',
                  opacity: 0,
                  transition: 'opacity 120ms ease',
                  whiteSpace: 'nowrap',
                }}
                className="row-action-btn"
              >
                撤回
              </button>
            ) : null

            // 回复按钮：对所有非乐观、非撤回、非贴纸消息可用（不限 2 分钟、
            // 不限我方对方）。点击 → 把这条的 PNM + 文本写到 replyTarget，
            // composer 上方就会出现 quote 头。
            const canReply = Boolean(setReplyTarget)
              && !isOptimistic
              && messageKindGuard(message)
              && Boolean(message?.upstreamMessageId || message?.upstream_message_id)
            const replyButton = canReply ? (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  const pnm = String(message?.upstreamMessageId || message?.upstream_message_id || '').trim()
                  if (!pnm) return
                  const text = String(
                    message?.text ?? message?.preview ?? message?.title ?? '',
                  ).trim() || '[图片]'
                  setReplyTarget?.({ messageId: pnm, text })
                }}
                aria-label="引用回复这条消息"
                style={{
                  alignSelf: 'center',
                  padding: '4px 10px',
                  fontSize: 12,
                  color: BRAND.textSecondary,
                  background: BRAND.card,
                  border: `1px solid ${BRAND.border}`,
                  borderRadius: 999,
                  cursor: 'pointer',
                  opacity: 0,
                  transition: 'opacity 120ms ease',
                  whiteSpace: 'nowrap',
                }}
                className="row-action-btn"
              >
                回复
              </button>
            ) : null

            // 引用气泡：在主气泡 ABOVE 渲染一个浅灰小块。`selectedMessages`
            // 当作 lookup 池，所以接收侧拿到只带 PNM 的 push 也能回扫历史
            // 找到 quote 文本。
            const replyQuote = getReplyQuote(message, selectedMessages)

            return (
              <div
                key={key}
                className={`flex ${mine ? 'justify-end' : 'justify-start'} items-end gap-2 message-row`}
                style={{ position: 'relative' }}
                onMouseEnter={(e) => {
                  e.currentTarget.querySelectorAll<HTMLButtonElement>('.row-action-btn').forEach((btn) => {
                    btn.style.opacity = '1'
                  })
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.querySelectorAll<HTMLButtonElement>('.row-action-btn').forEach((btn) => {
                    btn.style.opacity = '0'
                  })
                }}
              >
                {!mine && (
                  <MessageAvatar src={peerAvatarUrl} label={String(selectedSession?.peerNick ?? '')} kind="peer" />
                )}
                {/* 自己一侧：按钮在气泡左边（朝向中线），撤回 + 回复 并排 */}
                {mine && replyButton}
                {mine && recallButton}
                <div className={`flex max-w-[70%] flex-col gap-1 ${mine ? 'items-end' : 'items-start'}`}>
                  {replyQuote ? (
                    <div
                      style={{
                        maxWidth: '100%',
                        padding: '4px 8px 4px 10px',
                        fontSize: 12,
                        lineHeight: 1.3,
                        color: BRAND.textSecondary,
                        background: 'var(--surface-soft)',
                        borderLeft: `3px solid ${BRAND.border}`,
                        borderRadius: 6,
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
                  >
                    {renderMessageContent(message, selectedSession)}
                  </div>
                  <div className="flex items-center gap-1.5 px-1">
                    <span className="text-[11px]" style={{ color: BRAND.textTertiary }}>
                      {formatMessageTime(message?.ts)}
                    </span>
                    {showReadHint ? (
                      <span
                        className="text-[10.5px]"
                        style={{ color: isRead ? BRAND.textTertiary : BRAND.accent }}
                        aria-label={isRead ? '对方已读' : '对方未读'}
                      >
                        {isRead ? '已读' : '未读'}
                      </span>
                    ) : null}
                  </div>
                </div>
                {/* 对方一侧：按钮在气泡右边（朝向中线），跟自己一侧对称 */}
                {!mine && replyButton}
                {mine && (
                  <MessageAvatar src={selfAvatarUrl} label={String(selectedSession?._accountNick ?? '')} kind="self" />
                )}
              </div>
            )
          })}
        </div>
      </div>

      {/* Composer */}
      {sessionType(selectedSession) !== 3 && sessionType(selectedSession) !== 6 && (
        <div className="shrink-0 border-t px-4 py-2" style={{ borderColor: BRAND.border, backgroundColor: BRAND.card }}>
          {replyTarget ? (
            <div
              className="mb-2 flex items-center gap-2 rounded-[8px] border px-2.5 py-1.5"
              style={{
                backgroundColor: 'var(--surface-soft)',
                borderColor: BRAND.border,
                color: BRAND.textSecondary,
                fontSize: 12,
              }}
            >
              <span style={{ color: BRAND.textTertiary, flexShrink: 0 }}>回复:</span>
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  color: BRAND.textMain,
                }}
                title={replyTarget.text}
              >
                {replyTarget.text}
              </span>
              <button
                type="button"
                onClick={() => setReplyTarget?.(null)}
                aria-label="取消引用"
                style={{
                  flexShrink: 0,
                  background: 'transparent',
                  border: 'none',
                  cursor: 'pointer',
                  color: BRAND.textSecondary,
                  fontSize: 14,
                  lineHeight: 1,
                  padding: 2,
                }}
              >
                ✕
              </button>
            </div>
          ) : null}
          {/* 闲鱼风格 composer：第一行 5 个图标，第二行 textarea + 发送按钮。
              图标条由 ComposerActionStrip 统一渲染（desktop / mobile 共用），
              本组件只负责文本输入和最终发送行为。 */}
          <ComposerActionStrip
            variant="desktop"
            uid={selectedSession?._uid || ''}
            sessionId={selectedSession?.sessionId || ''}
            disabled={sending}
            imageInputRef={imageInputRef}
            onImagePick={onImagePick}
            onInsertEmoji={(emoji) => setComposerText((composerText || '') + emoji)}
            onSendItem={async (it) => { if (onSendItem) await onSendItem(it) }}
            onSendLocation={async (c) => { if (onSendLocation) await onSendLocation(c) }}
          />
          <div className="mt-1 flex items-stretch gap-2">
            <RichComposerTextarea
              rows={1}
              value={composerText}
              onChange={setComposerText}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && !event.shiftKey) {
                  event.preventDefault()
                  onSend()
                }
              }}
              containerClassName="relative flex-1"
              previewClassName="px-3 py-2 text-[15px]"
              previewStyle={{
                lineHeight: 1.55,
              }}
              // 比原版（min-h 40 / max-h 112）放大 +60% → min-h 64 / max-h 179
              className="max-h-[179px] min-h-[64px] w-full resize-none rounded-[14px] border px-3 py-2 text-[15px] outline-none transition [&::-webkit-scrollbar]:hidden"
              style={{
                backgroundColor: BRAND.page,
                borderColor: BRAND.border,
                color: BRAND.textMain,
                scrollbarWidth: 'none',
                msOverflowStyle: 'none' as any,
                lineHeight: 1.55,
              }}
              placeholder="请输入消息，按Enter键发送或点击发送按钮发送"
            />

            <button
              onClick={onSend}
              disabled={sending || !composerText.trim()}
              className="flex h-11 w-11 shrink-0 self-center cursor-pointer items-center justify-center rounded-full text-white transition disabled:opacity-40"
              style={{ backgroundColor: BRAND.accent }}
              aria-label="发送消息"
            >
              <SendHorizontal className="h-5 w-5" />
            </button>
          </div>
        </div>
      )}
    </section>
  )
}
