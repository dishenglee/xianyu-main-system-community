// @ts-nocheck
import { Copy, Loader2, ShieldCheck, Trash2, UserRoundPlus, Users } from 'lucide-react'
import { SensitiveValue } from '../../components/SensitiveValue'

const ROLE_LABELS = {
  primary: '主用户',
  collaborator: '协作者',
}

const JOINED_VIA_LABELS = {
  qr_owner: '扫码创建',
  allowlist_auto_bind: '自动加入',
  manual_bind: '手动加入',
  invite_code: '邀请码加入',
}

function roleLabel(role) {
  return ROLE_LABELS[role] || '成员'
}

function joinedViaLabel(joinedVia) {
  return JOINED_VIA_LABELS[joinedVia] || '已加入'
}

function formatTimestamp(value) {
  const raw = Number(value)
  if (!Number.isFinite(raw) || raw <= 0) return ''
  const millis = raw > 1e12 ? raw : raw * 1000
  const date = new Date(millis)
  if (Number.isNaN(date.getTime())) return ''
  const pad = (input) => String(input).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

function inviteJoinCommand(code) {
  return `加入 ${code}`
}

function memberSummary(member) {
  const parts = [roleLabel(member.role), joinedViaLabel(member.joined_via)]
  if (member.invited_by_sender_name) {
    parts.push(`邀请人 ${member.invited_by_sender_name}`)
  }
  return parts.join(' · ')
}

export function FeishuCollaboratorPanel({
  variant = 'desktop',
  loading,
  members,
  activeInvite,
  inviteBusy,
  removingSenderId,
  onGenerateInvite,
  onRemoveMember,
  onCopyText,
}) {
  const isMobile = variant === 'mobile'
  const wrapperClass = isMobile
    ? 'rounded-2xl p-4'
    : 'surface-soft rounded-2xl p-4'
  const wrapperStyle = isMobile ? { background: 'var(--surface-card)' } : undefined
  const cardClass = isMobile ? 'liquid-muted-card rounded-xl px-3 py-3' : 'surface-card rounded-xl px-3 py-3'
  const subtleTextColor = isMobile ? 'var(--text-secondary)' : 'var(--text-muted)'
  const rowBorderColor = isMobile ? 'var(--glass-border)' : 'var(--border-light)'

  return (
    <div className={wrapperClass} style={wrapperStyle} data-testid={`${variant}-feishu-collaborator-panel`}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Users className="h-4 w-4" style={{ color: 'var(--primary)' }} />
            <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>协作者管理</div>
          </div>
          <div className="mt-1 text-[12px]" style={{ color: subtleTextColor }}>
            扫码创建者是主用户。协作者加入后可处理 IM、商品和订单，但不能管理渠道配置。
          </div>
        </div>
        <button
          type="button"
          onClick={onGenerateInvite}
          disabled={loading || inviteBusy}
          className="btn-secondary shrink-0 disabled:opacity-40"
        >
          {inviteBusy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <UserRoundPlus className="h-3.5 w-3.5" />}
          {activeInvite ? '刷新邀请码' : '生成邀请码'}
        </button>
      </div>

      {!!activeInvite && (
        <div className={`${cardClass} mt-3`}>
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[11px] font-medium" style={{ color: subtleTextColor }}>当前邀请码</div>
              <div className="mt-1 font-mono text-[18px] font-semibold tracking-[0.08em]" style={{ color: 'var(--text-main)' }}>
                {activeInvite.code}
              </div>
              <div className="mt-1 text-[11px]" style={{ color: subtleTextColor }}>
                飞书里发送「{inviteJoinCommand(activeInvite.code)}」即可加入
              </div>
              {!!formatTimestamp(activeInvite.expires_at) && (
                <div className="mt-1 text-[11px]" style={{ color: subtleTextColor }}>
                  有效期至 {formatTimestamp(activeInvite.expires_at)}
                </div>
              )}
            </div>
            <ShieldCheck className="h-4 w-4 shrink-0" style={{ color: 'var(--primary)' }} />
          </div>

          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              className="btn-secondary"
              onClick={() => onCopyText(activeInvite.code, '邀请码已复制')}
            >
              <Copy className="h-3.5 w-3.5" /> 复制邀请码
            </button>
            <button
              type="button"
              className="btn-secondary"
              onClick={() => onCopyText(inviteJoinCommand(activeInvite.code), '加入命令已复制')}
            >
              <Copy className="h-3.5 w-3.5" /> 复制加入命令
            </button>
          </div>
        </div>
      )}

      <div className="mt-3">
        {loading ? (
          <div className={`${cardClass} flex items-center justify-center gap-2 py-4 text-[12px]`} style={{ color: subtleTextColor }}>
            <Loader2 className="h-4 w-4 animate-spin" /> 加载协作者…
          </div>
        ) : members?.length ? (
          <div className="space-y-2">
            {members.map((member) => {
              const primary = member.role === 'primary'
              return (
                <div
                  key={member.sender_id || member.id}
                  className={`${cardClass} flex items-start justify-between gap-3`}
                  style={{ border: `1px solid ${rowBorderColor}` }}
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                        <SensitiveValue kind="account" value={member.sender_name || member.sender_id} />
                      </div>
                      <span
                        className="inline-flex shrink-0 rounded-full px-2 py-0.5 text-[10px] font-semibold"
                        style={primary
                          ? { background: 'color-mix(in srgb, var(--primary) 14%, transparent)', color: 'var(--primary)' }
                          : { background: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' }}
                      >
                        {roleLabel(member.role)}
                      </span>
                    </div>
                    <div className="mt-1 text-[11px]" style={{ color: subtleTextColor }}>
                      {memberSummary(member)}
                    </div>
                  </div>

                  {!primary && (
                    <button
                      type="button"
                      className="btn-secondary shrink-0 disabled:opacity-40"
                      disabled={removingSenderId === member.sender_id}
                      onClick={() => onRemoveMember(member)}
                    >
                      {removingSenderId === member.sender_id ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Trash2 className="h-3.5 w-3.5" />
                      )}
                      移除
                    </button>
                  )}
                </div>
              )
            })}
          </div>
        ) : (
          <div className={`${cardClass} text-[12px]`} style={{ color: subtleTextColor }}>
            暂无协作者，先生成邀请码邀请客服加入。
          </div>
        )}
      </div>
    </div>
  )
}
