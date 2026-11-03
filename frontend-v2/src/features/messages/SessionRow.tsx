// @ts-nocheck
import { memo } from 'react'
import { Bell, Heart } from 'lucide-react'

import { colorForAccount } from './accountColor'
import {
  BRAND,
  accountLabel,
  formatSessionTime,
  peerLabel,
  resolveSessionItem,
  sessionType,
  summaryText,
  withAlpha,
} from './messagesConstants'
import { shouldShowUnreadBadge } from './sessionLayout'
import { SafeRemoteImage } from './SafeRemoteImage'
import { renderXianyuEmojiText } from './xianyuEmojiText'
import { SensitiveValue, useSensitiveAvatarInitial, useSensitiveText } from '../../components/SensitiveValue'

function SessionAvatar({ session, size }: { session: any; size: string }) {
  const fallback = useSensitiveAvatarInitial(peerLabel(session))
  const sharedClass = `${size} rounded-full shrink-0 flex items-center justify-center overflow-hidden`
  const accountColor = colorForAccount(session?._uid)

  if (session?.peerLogo) {
    return (
      <SafeRemoteImage
        src={session.peerLogo}
        alt=""
        className={`${sharedClass} object-cover`}
        fallback={<div className={sharedClass} style={{ backgroundColor: withAlpha(accountColor, '20'), color: accountColor }}><span className="text-sm font-semibold">{fallback}</span></div>}
      />
    )
  }

  if (sessionType(session) === 6) return <div className={sharedClass} style={{ background: 'var(--surface-soft)', border: `1px solid ${BRAND.border}` }}><Heart className="h-4.5 w-4.5" style={{ color: BRAND.textSecondary }} /></div>
  if (sessionType(session) === 3) return <div className={sharedClass} style={{ background: 'var(--surface-soft)', border: `1px solid ${BRAND.border}` }}><Bell className="h-4.5 w-4.5" style={{ color: BRAND.textSecondary }} /></div>
  return <div className={sharedClass} style={{ backgroundColor: withAlpha(accountColor, '20'), color: accountColor }}><span className="text-sm font-semibold">{fallback}</span></div>
}

export function renderSessionAvatar(session, size = 'h-11 w-11') {
  return <SessionAvatar session={session} size={size} />
}

function itemPicForSession(item: any) {
  return String(item?.pic || '').trim()
}

// Fields we actually render — if any of these change, re-render; else skip.
// This matters because `allSessions` uses `...session` spread which creates
// a new object ref each re-render, so default shallow-compare memo would
// invalidate all 717 rows every time. The field list must stay in sync with
// what SessionRow actually reads.
function sessionRowPropsEqual(a: any, b: any) {
  if (a.active !== b.active || a.onSelect !== b.onSelect) return false
  const sa = a.session
  const sb = b.session
  if (sa === sb) return true
  return (
    sa?.sessionId === sb?.sessionId &&
    sa?.sessionType === sb?.sessionType &&
    sa?._uid === sb?._uid &&
    sa?.peerNick === sb?.peerNick &&
    sa?.peerLogo === sb?.peerLogo &&
    sa?.peerUid === sb?.peerUid &&
    sa?.itemId === sb?.itemId &&
    sa?.itemTitle === sb?.itemTitle &&
    sa?.itemPic === sb?.itemPic &&
    sa?.itemPicUrl === sb?.itemPicUrl &&
    sa?.summary === sb?.summary &&
    sa?.unread === sb?.unread &&
    sa?.ts === sb?.ts
  )
}

// React.memo'd row so clicking a session only re-renders 2 rows (prev + new
// selection) instead of re-doing work for all 717 sessions on every poll,
// mark-read, itemPrice fetch, etc. Props only change when the session object
// itself changes (poll merges) or the active flag flips.
export const SessionRow = memo(function SessionRow({
  session,
  active,
  onSelect,
  onTouchStart,
  onTouchMove,
  onTouchEnd,
  onTouchCancel,
  onContextMenu,
  variant = 'desktop',
}: {
  session: any
  active: boolean
  onSelect: (session: any) => void
  onTouchStart?: (event: any) => void
  onTouchMove?: (event: any) => void
  onTouchEnd?: (event: any) => void
  onTouchCancel?: (event: any) => void
  onContextMenu?: (event: any) => void
  variant?: 'desktop' | 'mobile'
}) {
  const ringColor = colorForAccount(session._uid)
  const unread = Number(session?.unread || 0)
  const showUnreadBadge = unread > 0 && shouldShowUnreadBadge(session)
  const item = resolveSessionItem(session)
  const itemLabel = item?.title || item?.itemId || '关联商品'
  const isMobile = variant === 'mobile'
  const safePeerLabel = useSensitiveText('buyer', peerLabel(session))
  const safeAccountLabel = useSensitiveText('account', accountLabel(session))
  const safeItemLabel = useSensitiveText(item?.title ? 'item-title' : 'order-id', itemLabel)

  return (
    <button
      onClick={() => onSelect(session)}
      onTouchStart={onTouchStart}
      onTouchMove={onTouchMove}
      onTouchEnd={onTouchEnd}
      onTouchCancel={onTouchCancel}
      onContextMenu={onContextMenu}
      className={isMobile
        ? 'xianyu-mobile-session-row relative flex w-full cursor-pointer items-center text-left transition-colors'
        : 'xianyu-desktop-session-row relative flex w-full cursor-pointer items-center gap-3 border-b px-4 py-3 text-left transition-colors'}
      style={{
        backgroundColor: isMobile
          ? 'transparent'
          : active
            ? 'rgba(255,255,255,0.92)'
            : 'rgba(255,255,255,0.72)',
        borderColor: isMobile ? undefined : 'rgba(15, 23, 42, 0.06)',
      }}
    >
      {active && !isMobile ? (
        <span
          className="absolute bottom-3 left-0 top-3 w-[3px] rounded-r-full"
          style={{ backgroundColor: BRAND.accent }}
        />
      ) : null}

      <div className="relative shrink-0">
        <div className="rounded-full p-[2px]" style={{ backgroundColor: withAlpha(ringColor, isMobile ? '8' : '12') }}>
          {renderSessionAvatar(session, isMobile ? 'h-[48px] w-[48px]' : 'h-[42px] w-[42px]')}
        </div>
        {showUnreadBadge && (
          <span
            className="absolute -right-0.5 -top-0.5 min-w-[18px] rounded-full px-1.5 py-0.5 text-center text-[10px] font-semibold leading-none text-white"
            style={{ backgroundColor: BRAND.accent }}
          >
            {unread > 99 ? '99+' : unread}
          </span>
        )}
      </div>

      <div className={`min-w-0 flex-1 ${isMobile ? 'py-0.5' : ''}`}>
        <div className="flex min-w-0 items-start gap-2">
          <span
            className={isMobile
              ? 'truncate text-[17px] font-semibold leading-[1.08]'
              : 'truncate text-[14px] font-semibold leading-tight'}
            style={{ color: BRAND.textMain }}
          >
            {safePeerLabel}
          </span>
          {Number(session?.sessionType || 1) !== 1 ? (
            <span className="shrink-0 rounded-full px-2 py-0.5 text-[10px] font-semibold" style={{ backgroundColor: BRAND.selected, color: BRAND.textSecondary }}>
              {safeAccountLabel}
            </span>
          ) : null}
        </div>
        <p
          className={`${isMobile ? 'mt-1 text-[14px]' : 'mt-1 text-[12.5px]'} truncate leading-tight ${unread > 0 ? 'font-medium' : ''}`}
          style={{ color: isMobile ? '#8e97a7' : BRAND.textTertiary }}
        >
          {renderXianyuEmojiText(summaryText(session))}
        </p>
        <div className={`${isMobile ? 'mt-2' : 'mt-1.5'} flex items-center gap-2`}>
          <span className={`${isMobile ? 'text-[11px]' : 'text-[12px]'} font-medium`} style={{ color: isMobile ? '#a9b3c1' : BRAND.textTertiary }}>
            {formatSessionTime(session.ts)}
          </span>
        </div>
      </div>

      {item ? (
        <div
          className={isMobile
            ? 'ml-2.5 flex h-[54px] w-[72px] shrink-0 items-center justify-center overflow-hidden rounded-[12px]'
            : 'flex h-[42px] w-[42px] shrink-0 items-center justify-center overflow-hidden rounded-[10px]'}
          title={safeItemLabel}
          aria-label={safeItemLabel}
          style={{
            background: 'rgba(255,255,255,0.72)',
            boxShadow: isMobile ? '0 8px 20px rgba(25,35,55,0.06)' : 'none',
          }}
        >
          {itemPicForSession(item) ? (
            <SafeRemoteImage
              src={itemPicForSession(item)}
              alt={safeItemLabel}
              className="h-full w-full object-cover"
              sensitiveItem
              loading="lazy"
              fallback={(
                <span className="line-clamp-3 px-2 text-center text-[10px] font-medium leading-tight" style={{ color: BRAND.textTertiary }}>
                  {safeItemLabel}
                </span>
              )}
            />
          ) : (
            <span className="line-clamp-3 px-2 text-center text-[10px] font-medium leading-tight" style={{ color: BRAND.textTertiary }}>
              {safeItemLabel}
            </span>
          )}
        </div>
      ) : null}
    </button>
  )
}, sessionRowPropsEqual)
