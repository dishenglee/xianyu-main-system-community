// @ts-nocheck
import { useEffect, useRef, useState } from 'react'
import { ExternalLink, Loader2, QrCode, RefreshCw } from 'lucide-react'
import { Modal } from '../../components/Modal'
import { pollFeishuQrRegistration, startFeishuQrRegistration } from '../../api/notifications'

type Props = {
  open: boolean
  onClose: () => void
  onChannelCreated?: (channel: Record<string, unknown>) => void
}

const TERMINAL_STATUSES = new Set(['confirmed', 'expired', 'failed', 'denied'])
const DEFAULT_REQUIRED_SCOPES = [
  'im:message.group_msg',
  'im:message.group_at_msg:readonly',
  'im:message.p2p_msg:readonly',
  'im:message:send_as_bot',
]

const STATUS_LABELS: Record<string, string> = {
  pending: '等待扫码绑定',
  authorization_pending: '等待确认',
  slow_down: '等待确认',
  scope_setup_required: '等待扫码授权',
  scope_apply_pending: '等待授权生效',
  confirmed: '已创建',
  expired: '二维码已过期',
  failed: '注册失败',
  denied: '已取消',
}

function statusLabel(status: string) {
  return STATUS_LABELS[status] || status || '初始化中'
}

function nextPollDelayMs(intervalMs: number, status: string) {
  if (status === 'slow_down') return Math.max(intervalMs + 1000, 3000)
  return Math.max(intervalMs || 2000, 1200)
}

export function FeishuQrRegistrationModal({ open, onClose, onChannelCreated }: Props) {
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [deviceCode, setDeviceCode] = useState('')
  const [domain, setDomain] = useState('feishu')
  const [qrImage, setQrImage] = useState('')
  const [qrUrl, setQrUrl] = useState('')
  const [pollIntervalMs, setPollIntervalMs] = useState(2000)
  const [status, setStatus] = useState('')
  const [requiredScopes, setRequiredScopes] = useState<string[]>(DEFAULT_REQUIRED_SCOPES)
  const [missingScopes, setMissingScopes] = useState<string[]>([])
  const [publishRequired, setPublishRequired] = useState(true)
  const [scopeHint, setScopeHint] = useState('')
  const aliveRef = useRef(false)
  const runIdRef = useRef(0)
  const [runId, setRunId] = useState(0)

  function isActiveRun(id: number) {
    return aliveRef.current && runIdRef.current === id
  }

  async function start() {
    const currentRunId = runIdRef.current + 1
    runIdRef.current = currentRunId
    setRunId(currentRunId)
    setLoading(true)
    setError('')
    setStatus('')
    setDeviceCode('')
    setQrImage('')
    setQrUrl('')
    setRequiredScopes(DEFAULT_REQUIRED_SCOPES)
    setMissingScopes([])
    setPublishRequired(true)
    setScopeHint('')
    try {
      const response = await startFeishuQrRegistration()
      if (!isActiveRun(currentRunId)) return
      setDeviceCode(response.device_code || '')
      setDomain(response.domain || 'feishu')
      setQrImage(response.qr_image || '')
      setQrUrl(response.qr_url || '')
      setPollIntervalMs(Number(response.poll_interval_ms) || 2000)
      setRequiredScopes(response.required_scopes?.length ? response.required_scopes : DEFAULT_REQUIRED_SCOPES)
      setPublishRequired(response.publish_required !== false)
      setStatus('pending')
    } catch (e: any) {
      if (!isActiveRun(currentRunId)) return
      setError(e?.message || '获取二维码失败')
    } finally {
      if (isActiveRun(currentRunId)) {
        setLoading(false)
      }
    }
  }

  useEffect(() => {
    if (!open) {
      aliveRef.current = false
      runIdRef.current += 1
      return
    }
    aliveRef.current = true
    start()
    return () => {
      aliveRef.current = false
      runIdRef.current += 1
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  useEffect(() => {
    if (!open || !deviceCode || TERMINAL_STATUSES.has(status)) return

    let cancelled = false
    let handle: ReturnType<typeof setTimeout> | undefined
    const currentRunId = runId
    const shouldStop = () => cancelled || !aliveRef.current || runIdRef.current !== currentRunId

    const scheduleNext = (nextStatus: string) => {
      if (shouldStop() || TERMINAL_STATUSES.has(nextStatus)) return
      handle = setTimeout(pollOnce, nextPollDelayMs(pollIntervalMs, nextStatus))
    }

    const pollOnce = async () => {
      if (shouldStop()) return
      try {
        const response = await pollFeishuQrRegistration({ device_code: deviceCode, domain })
        if (shouldStop()) return
        const nextStatus = response.status || 'pending'
        setStatus(nextStatus)
        if (response.qr_image) setQrImage(response.qr_image)
        if (response.qr_url) setQrUrl(response.qr_url)
        if (response.required_scopes?.length) setRequiredScopes(response.required_scopes)
        setMissingScopes(response.missing_scopes?.length ? response.missing_scopes : [])
        setPublishRequired(response.publish_required !== false)
        setScopeHint(response.scope_hint || '')
        if (nextStatus === 'confirmed' && response.channel && onChannelCreated) {
          onChannelCreated(response.channel)
        }
        scheduleNext(nextStatus)
      } catch (e: any) {
        if (shouldStop()) return
        setError(e?.message || '轮询失败')
        scheduleNext(status || 'pending')
      }
    }

    scheduleNext(status || 'pending')

    return () => {
      cancelled = true
      if (handle) clearTimeout(handle)
    }
  }, [open, deviceCode, domain, pollIntervalMs, status, runId, onChannelCreated])

  return (
    <Modal open={open} onClose={onClose} title="飞书扫码接入">
      <div className="space-y-4">
        <div className="grid gap-2 sm:grid-cols-2">
          <div
            className="rounded-2xl border px-3 py-3"
            style={{ borderColor: 'var(--border-light)', background: status === 'scope_setup_required' || status === 'scope_apply_pending' || status === 'confirmed' ? 'var(--surface-soft)' : 'var(--primary-soft)' }}
          >
            <div className="text-[12px] font-medium" style={{ color: 'var(--text-primary)' }}>第一步</div>
            <div className="mt-1 text-[14px] font-semibold" style={{ color: 'var(--text-primary)' }}>扫码绑定</div>
            <p className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              先创建并绑定飞书机器人。
            </p>
          </div>
          <div
            className="rounded-2xl border px-3 py-3"
            style={{ borderColor: 'var(--border-light)', background: status === 'scope_setup_required' || status === 'scope_apply_pending' ? 'var(--primary-soft)' : status === 'confirmed' ? 'var(--surface-soft)' : 'transparent' }}
          >
            <div className="text-[12px] font-medium" style={{ color: 'var(--text-primary)' }}>第二步</div>
            <div className="mt-1 text-[14px] font-semibold" style={{ color: 'var(--text-primary)' }}>扫码授权</div>
            <p className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              给机器人补齐接收群消息和发送回复所需权限。
            </p>
          </div>
        </div>

        <div className="flex flex-col items-center gap-3 py-2">
          {loading ? (
            <div className="surface-soft flex h-56 w-56 items-center justify-center rounded-2xl" style={{ color: 'var(--text-muted)' }}>
              <Loader2 className="h-6 w-6 animate-spin" />
            </div>
          ) : qrImage ? (
            <img src={qrImage} alt="飞书二维码" className="h-56 w-56 rounded-2xl border" style={{ borderColor: 'var(--border-light)' }} />
          ) : (
            <div className="surface-soft flex h-56 w-56 items-center justify-center rounded-2xl" style={{ color: 'var(--text-muted)' }}>
              <QrCode className="h-10 w-10" />
            </div>
          )}

          <div className="flex items-center gap-2 text-[12.5px]" style={{ color: 'var(--text-secondary)' }}>
            <span
              className="inline-block h-2 w-2 rounded-full"
              style={{ background: status === 'confirmed' ? 'var(--success)' : TERMINAL_STATUSES.has(status) ? 'var(--danger)' : 'var(--primary)' }}
            />
            <span>{statusLabel(status)}</span>
          </div>

          {!!qrUrl && (
            <a
              href={qrUrl}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-[12px]"
              style={{ color: 'var(--primary)' }}
            >
              <ExternalLink className="h-3.5 w-3.5" /> 打开授权页
            </a>
          )}

          {error && (
            <p className="liquid-notice liquid-notice-danger rounded-xl px-3 py-2 text-[12px]">{error}</p>
          )}
        </div>

        {status === 'scope_setup_required' || status === 'scope_apply_pending' || missingScopes.length ? (
          <div className="space-y-2 rounded-2xl border px-3 py-3 text-[12px]" style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}>
            <div className="font-medium" style={{ color: 'var(--text-primary)' }}>第二步授权说明</div>
            <ul className="space-y-1" style={{ color: 'var(--text-secondary)' }}>
              {requiredScopes.map((scope) => (
                <li key={scope}>- <code>{scope}</code></li>
              ))}
            </ul>
            {missingScopes.length ? (
              <>
                <div className="font-medium" style={{ color: 'var(--text-primary)' }}>当前还缺这些权限</div>
                <ul className="space-y-1" style={{ color: 'var(--warning)' }}>
                  {missingScopes.map((scope) => (
                    <li key={scope}>- <code>{scope}</code></li>
                  ))}
                </ul>
              </>
            ) : null}
            <p style={{ color: 'var(--text-secondary)' }}>
              <code>im:message.group_msg</code> 决定机器人能不能收到群里非 <code>@</code> 的普通消息。
            </p>
            {scopeHint ? (
              <p style={{ color: 'var(--primary)' }}>{scopeHint}</p>
            ) : null}
            {publishRequired ? (
              <p style={{ color: 'var(--warning)' }}>
                当前权限还没有完全生效；如果飞书侧要求发布或审批，完成后群里的免 @ 回复才会生效。
              </p>
            ) : null}
          </div>
        ) : null}

        <div className="flex gap-2">
          <button onClick={start} disabled={loading} className="btn-secondary flex-1 disabled:opacity-40">
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} /> 刷新二维码
          </button>
          <button onClick={onClose} className="btn-secondary flex-1">关闭</button>
        </div>
      </div>
    </Modal>
  )
}
