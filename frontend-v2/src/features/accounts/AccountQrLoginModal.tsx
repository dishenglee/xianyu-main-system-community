import {
  Check,
  RefreshCw,
} from 'lucide-react'
import { Modal } from '../../components/Modal'

interface QrAccountConflict {
  accountUid?: string
  canApplyUnbind?: boolean
}

interface AccountQrLoginModalProps {
  open: boolean
  qrUrl: string
  qrStatusText: string
  qrScanned?: boolean
  qrConflict?: QrAccountConflict | null
  unbindApplying?: boolean
  onClose: () => void
  onRefreshQr: () => void
  onApplyAccountUnbind: () => void
}

export function AccountQrLoginModal({
  open,
  qrUrl,
  qrStatusText,
  qrScanned = false,
  qrConflict = null,
  unbindApplying = false,
  onClose,
  onRefreshQr,
  onApplyAccountUnbind,
}: AccountQrLoginModalProps) {
  return (
    <Modal open={open} onClose={onClose} title="扫码登录">
      <div className="text-center py-2">
        <QrImageFrame qrUrl={qrUrl} scanned={qrScanned} />
        <p className="mt-4 text-[14px]" style={{ color: 'var(--text-secondary)' }}>
          {qrStatusText}
        </p>

        {qrConflict?.canApplyUnbind && (
          <div
            className="mx-auto mt-4 max-w-[360px] rounded-2xl px-4 py-3 text-center text-[13px]"
            style={{
              background: 'rgba(255, 149, 0, 0.10)',
              border: '1px solid rgba(255, 149, 0, 0.28)',
              color: 'var(--text-primary)',
            }}
          >
            <div className="font-semibold">这个账号已绑定到其他用户</div>
            <div
              className="mt-2 rounded-xl px-3 py-2 text-[13px] font-semibold"
              style={{
                background: 'rgba(255, 149, 0, 0.16)',
                color: '#b45309',
              }}
            >
              为防止该账号多端登录频繁请求引起不必要的风控
            </div>
            <div className="mt-1" style={{ color: 'var(--text-secondary)' }}>
              账号 UID：{qrConflict.accountUid}。你可以提交解绑申请，管理员处理后再重新扫码添加。
            </div>
            <button
              type="button"
              className="mx-auto mt-3 rounded-full px-4 py-2 text-[13px] font-medium disabled:opacity-50"
              style={{ background: '#ff9500', color: '#fff' }}
              disabled={unbindApplying}
              onClick={onApplyAccountUnbind}
            >
              {unbindApplying ? '正在提交…' : '申请解绑'}
            </button>
          </div>
        )}

        <button
          type="button"
          className="mt-4 rounded-full px-4 py-2 text-[13px] font-medium disabled:opacity-50 disabled:cursor-not-allowed"
          style={{
            background: 'var(--control-bg)',
            border: '1px solid var(--control-border)',
            color: 'var(--text-primary)',
          }}
          onClick={onRefreshQr}
        >
          重新生成二维码
        </button>
      </div>
    </Modal>
  )
}

function QrImageFrame({ qrUrl, scanned }: { qrUrl: string; scanned: boolean }) {
  return (
    <div className="relative mx-auto h-48 w-48 overflow-hidden rounded-xl border" style={{ borderColor: 'var(--border-light)' }}>
      {qrUrl ? (
        <img src={qrUrl} className="h-full w-full object-cover" alt="二维码" />
      ) : (
        <div className="flex h-full w-full items-center justify-center">
          <RefreshCw className="h-8 w-8 animate-spin" style={{ color: 'var(--text-muted)' }} />
        </div>
      )}
      {scanned ? (
        <div
          className="account-qr-scanned-overlay absolute inset-0 flex flex-col items-center justify-center"
          aria-label="二维码已扫码"
          role="status"
        >
          <div className="account-qr-scan-check flex h-20 w-20 items-center justify-center rounded-full bg-white">
            <Check className="h-12 w-12" style={{ color: '#16a34a' }} />
          </div>
          <div className="mt-3 text-[15px] font-semibold text-white">已扫码</div>
        </div>
      ) : null}
    </div>
  )
}
