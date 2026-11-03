// @ts-nocheck
//
// iLink (WeChat) QR-code login modal.
//
// Drives the two-call dance with the backend:
//   1. POST /api/notification-channels/wechat/qr-start   → render QR image
//   2. POST /api/notification-channels/wechat/qr-poll    → long-poll status
//
// Status handling mirrors the Tencent plugin:
//   wait | scaned                       → keep polling
//   scaned_but_redirect                 → switch host, keep polling
//   need_verifycode                     → ask user for code, re-poll w/ it
//   verify_code_blocked | expired       → terminal failure
//   binded_redirect                     → already bound, treat as redirect
//   confirmed                           → success
//
// On `confirmed`, the backend's qr-poll endpoint also auto-creates the
// notification channel (with bind_all_accounts: true) and embeds it as
// `channel` on the poll response. The modal fires `onChannelCreated(channel)`
// so the parent can just refresh the channel list — no form, no extra step.
// `onConfirmed` is kept as a legacy callback in case anyone still needs the
// raw credentials.
//
import { useEffect, useRef, useState } from 'react'
import { Loader2, QrCode, RefreshCw } from 'lucide-react'
import { Modal } from '../../components/Modal'
import { startWechatQrLogin, pollWechatQrLogin } from '../../api/notifications'

type Confirmed = {
  bot_token: string
  baseurl: string
  ilink_user_id: string
  ilink_bot_id?: string
}

type Props = {
  open: boolean
  onClose: () => void
  // Legacy hook for callers that still want raw creds (e.g. the old
  // "fill the form" flow). Optional — the new shortcut flow ignores it.
  onConfirmed?: (creds: Confirmed) => void
  // Fired with the server-auto-created channel row when status hits
  // `confirmed`. Parent should refresh its channel list and close the modal.
  onChannelCreated?: (channel: Record<string, unknown>) => void
}

const STATUS_LABELS: Record<string, string> = {
  wait: '等待扫码…',
  scaned: '已扫码，请在微信中确认',
  scaned_but_redirect: '正在切换登录节点…',
  need_verifycode: '请输入验证码',
  verify_code_blocked: '验证码已被锁定，请稍后再试',
  expired: '二维码已过期，请刷新',
  binded_redirect: '已绑定，切换节点中…',
  confirmed: '登录成功',
}

function statusLabel(s: string) {
  return STATUS_LABELS[s] || s || '初始化中…'
}

// Decide gap before next poll. Short for redirect (chained calls), longer for
// wait/scaned where the backend itself long-polls ~35s.
function gapMsFor(status: string) {
  if (status === 'scaned_but_redirect' || status === 'binded_redirect') return 500
  if (status === 'need_verifycode') return 1500
  return 2500
}

// `qrcode_img_content` can be either a raw base64 PNG or a full `data:` URL —
// detect both shapes.
function asImageSrc(payload: string) {
  if (!payload) return ''
  if (payload.startsWith('data:')) return payload
  return `data:image/png;base64,${payload}`
}

export function WechatQrLoginModal({ open, onClose, onConfirmed, onChannelCreated }: Props) {
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [qrcode, setQrcode] = useState('')
  const [imgSrc, setImgSrc] = useState('')
  const [host, setHost] = useState('')
  const [status, setStatus] = useState('')
  const [verifyCode, setVerifyCode] = useState('')
  const aliveRef = useRef(false)

  // Refresh: kick off a new QR code.
  async function start() {
    setLoading(true)
    setError('')
    setStatus('')
    setVerifyCode('')
    try {
      const r = await startWechatQrLogin()
      setQrcode(r.qrcode || '')
      setImgSrc(asImageSrc(r.qrcode_img_content || ''))
      setHost(r.host || 'https://ilinkai.weixin.qq.com')
      setStatus('wait')
    } catch (e: any) {
      setError(e?.message || '获取二维码失败')
    } finally {
      setLoading(false)
    }
  }

  // Mount/open lifecycle — start once on open, stop polling on close.
  useEffect(() => {
    if (!open) {
      aliveRef.current = false
      return
    }
    aliveRef.current = true
    start()
    return () => { aliveRef.current = false }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  // Poll loop — re-runs whenever {qrcode, host, status, verifyCode} flips.
  useEffect(() => {
    if (!open || !qrcode || !host) return
    if (status === 'confirmed' || status === 'expired' || status === 'verify_code_blocked') return

    let cancelled = false
    const wait = gapMsFor(status)
    const handle = setTimeout(async () => {
      if (cancelled || !aliveRef.current) return
      try {
        const r = await pollWechatQrLogin({
          qrcode,
          host,
          verify_code: status === 'need_verifycode' && verifyCode ? verifyCode : undefined,
        })
        if (cancelled || !aliveRef.current) return
        const next = r.status || ''
        setStatus(next)

        if (next === 'scaned_but_redirect' || next === 'binded_redirect') {
          if (r.redirect_host) setHost(r.redirect_host)
        }
        if (next === 'confirmed') {
          // New flow: backend already created the channel. Hand the row
          // straight to the parent so it can refresh + close.
          if (r.channel && onChannelCreated) {
            onChannelCreated(r.channel)
          }
          // Legacy flow (in-form fill) — kept for backwards compat.
          if (onConfirmed) {
            onConfirmed({
              bot_token: r.bot_token || '',
              baseurl: r.baseurl || '',
              ilink_user_id: r.ilink_user_id || '',
              ilink_bot_id: r.ilink_bot_id,
            })
          }
        }
      } catch (e: any) {
        if (cancelled || !aliveRef.current) return
        setError(e?.message || '轮询失败')
      }
    }, wait)

    return () => { cancelled = true; clearTimeout(handle) }
  }, [open, qrcode, host, status, verifyCode, onConfirmed, onChannelCreated])

  return (
    <Modal open={open} onClose={onClose} title="微信扫码登录">
      <div className="space-y-4">
        <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
          使用微信扫描下方二维码，登录后将自动创建通知渠道并绑定所有账号。
        </p>

        <div className="flex flex-col items-center gap-3 py-2">
          {loading ? (
            <div className="surface-soft w-56 h-56 rounded-2xl flex items-center justify-center" style={{ color: 'var(--text-muted)' }}>
              <Loader2 className="w-6 h-6 animate-spin" />
            </div>
          ) : imgSrc ? (
            <img src={imgSrc} alt="QR" className="w-56 h-56 rounded-2xl border" style={{ borderColor: 'var(--border-light)' }} />
          ) : (
            <div className="surface-soft w-56 h-56 rounded-2xl flex items-center justify-center" style={{ color: 'var(--text-muted)' }}>
              <QrCode className="w-10 h-10" />
            </div>
          )}

          <div className="flex items-center gap-2 text-[12.5px]" style={{ color: 'var(--text-secondary)' }}>
            <span className="inline-block w-2 h-2 rounded-full"
              style={{ background: status === 'confirmed' ? 'var(--success)' : status === 'expired' || status === 'verify_code_blocked' ? 'var(--danger)' : 'var(--primary)' }} />
            <span>{statusLabel(status)}</span>
          </div>

          {status === 'need_verifycode' && (
            <input
              autoFocus
              placeholder="请输入微信发送的验证码"
              className="input"
              style={{ maxWidth: 240 }}
              value={verifyCode}
              onChange={(e) => setVerifyCode(e.target.value)}
            />
          )}

          {error && (
            <p className="liquid-notice liquid-notice-danger rounded-xl px-3 py-2 text-[12px]">{error}</p>
          )}
        </div>

        <div className="flex gap-2">
          <button onClick={start} disabled={loading} className="btn-secondary flex-1 disabled:opacity-40">
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} /> 刷新二维码
          </button>
          <button onClick={onClose} className="btn-secondary flex-1">关闭</button>
        </div>
      </div>
    </Modal>
  )
}
