// @ts-nocheck
// 消息气泡两侧的头像 — desktop / mobile 共用。
// 有图优先走 App 内图片代理；失败时渲染色块 + 首字符。
// `kind`：'peer'（对方，灰底）| 'self'（自己，主色淡背景）。
import { avatarFallback } from './messagesConstants'
import { SafeRemoteImage } from './SafeRemoteImage'

const BRAND_PRIMARY = 'var(--primary)'
const BRAND_BORDER = 'var(--control-border)'

interface Props {
  src?: string
  label?: string
  kind: 'peer' | 'self'
  size?: number
}

export function MessageAvatar({ src, label = '', kind, size = 32 }: Props) {
  const url = String(src || '').trim()
  const cls = `shrink-0 rounded-full overflow-hidden flex items-center justify-center`
  const style = { width: size, height: size }
  const fontSize = size <= 28 ? 10 : 11

  if (kind === 'self') {
    const fallback = (
      <div
        className={cls}
        style={{
          ...style,
          background: `${BRAND_PRIMARY}22`,
          border: `1px solid ${BRAND_PRIMARY}44`,
        }}
      >
        <span style={{ color: BRAND_PRIMARY, fontSize, fontWeight: 500 }}>
          {avatarFallback(String(label))}
        </span>
      </div>
    )
    if (url) {
      return (
        <SafeRemoteImage
          src={url}
          alt=""
          className={`${cls} object-cover`}
          style={style}
          fallback={fallback}
        />
      )
    }
    return fallback
  }

  const fallback = (
    <div
      className={cls}
      style={{
        ...style,
        background: 'var(--surface-soft)',
        border: `1px solid ${BRAND_BORDER}`,
      }}
    >
      <span style={{ color: 'var(--text-secondary)', fontSize, fontWeight: 500 }}>
        {avatarFallback(String(label))}
      </span>
    </div>
  )
  if (url) {
    return (
      <SafeRemoteImage
        src={url}
        alt=""
        className={`${cls} object-cover`}
        style={style}
        fallback={fallback}
      />
    )
  }
  return fallback
}

/// 提取 peer 头像 URL，桌面/手机 fallback 顺序一致。
export function resolvePeerAvatarUrl(message: any, session: any): string {
  return String(message?.senderLogo || message?.sender_logo || session?.peerLogo || '').trim()
}

/// 提取 self 头像 URL（当前账号的 avatar）。
export function resolveSelfAvatarUrl(session: any): string {
  return String(session?._accountAvatar || '').trim()
}
