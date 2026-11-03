// @ts-nocheck
import { useEffect, useMemo, useRef, useState } from 'react'
import {
  accountEditableProfile,
  isSuccess,
  retMsg,
  updateAccountProfile,
} from '../../api/index'
import { uploadItemImage } from '../../api/market'
import { Modal } from '../../components/Modal'
import { useSafeNotify } from '../../components/Notify'

function valueOf(...values) {
  for (const value of values) {
    if (value != null && String(value).trim() !== '') return String(value)
  }
  return ''
}

function buildInitialForm(account, profile) {
  return {
    snsNick: valueOf(profile?.snsNick, profile?.displayNick, profile?.nick, account?.nick),
    signature: valueOf(profile?.signature),
    avatar: valueOf(profile?.avatar, profile?.avatarUrl, account?.avatar),
    gender: valueOf(profile?.gender),
    birthday: valueOf(profile?.birthday),
    city: valueOf(profile?.city),
    university: valueOf(profile?.university),
  }
}

function changedProfilePayload(uid, form, dirtyFields) {
  const payload = { uid }
  Object.entries(form).forEach(([key, value]) => {
    if (!dirtyFields[key]) return
    payload[key] = String(value ?? '').trim()
  })
  return payload
}

function uploadedImageUrl(payload) {
  return String(payload?.data?.url || payload?.data?.imageUrl || payload?.url || '').trim()
}

function ProfileInput({ label, value, onChange, placeholder = '', maxLength, type = 'text' }) {
  return (
    <label className="block text-[12px]" style={{ color: 'var(--text-secondary)' }}>
      <span className="mb-1.5 block">{label}</span>
      <input
        aria-label={label}
        className="w-full rounded-[12px] px-3 py-2 text-[13px] outline-none transition-colors"
        style={{
          background: 'var(--surface-soft)',
          border: '1px solid var(--control-border)',
          color: 'var(--text-main)',
        }}
        value={value}
        maxLength={maxLength}
        type={type}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
      />
    </label>
  )
}

export function AccountProfileEditor({
  uid,
  account,
  profile,
  open = false,
  onClose,
  isMobile = false,
  onProfileUpdated,
}) {
  const notify = useSafeNotify()
  const initialForm = useMemo(() => buildInitialForm(account, profile), [account, profile])
  const [form, setForm] = useState(initialForm)
  const [dirtyFields, setDirtyFields] = useState({})
  const [saving, setSaving] = useState(false)
  const [uploadingAvatar, setUploadingAvatar] = useState(false)
  const [hydratingProfile, setHydratingProfile] = useState(false)
  const hydrateRequestRef = useRef(0)
  const dirtyFieldsRef = useRef({})

  useEffect(() => {
    setForm(initialForm)
    setDirtyFields({})
    setUploadingAvatar(false)
    setHydratingProfile(false)
    dirtyFieldsRef.current = {}
  }, [uid, initialForm, open])

  useEffect(() => {
    dirtyFieldsRef.current = dirtyFields
  }, [dirtyFields])

  useEffect(() => {
    if (!open || !uid) return
    const requestId = ++hydrateRequestRef.current
    setHydratingProfile(true)
    void (async () => {
      try {
        const liveProfile = await accountEditableProfile(uid, true)
        if (hydrateRequestRef.current !== requestId) return
        const hydratedForm = buildInitialForm(account, {
          ...(profile || {}),
          ...(liveProfile || {}),
        })
        setForm((current) => {
          const next = { ...current }
          Object.entries(hydratedForm).forEach(([key, value]) => {
            if (dirtyFieldsRef.current[key]) return
            next[key] = value
          })
          return next
        })
      } catch {
        // 保留当前可用资料，避免弹窗打开失败。
      } finally {
        if (hydrateRequestRef.current === requestId) {
          setHydratingProfile(false)
        }
      }
    })()
  }, [open, uid, account, profile])

  function setField(key, value) {
    setForm((current) => ({ ...current, [key]: value }))
    setDirtyFields((current) => ({ ...current, [key]: true }))
  }

  async function handleAvatarChange(event) {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (!file || !uid || uploadingAvatar) return

    setUploadingAvatar(true)
    try {
      const uploaded = await uploadItemImage(uid, file)
      const url = uploadedImageUrl(uploaded)
      if (!url) throw new Error(retMsg(uploaded, '头像上传失败'))
      setField('avatar', url)
      notify('头像已上传', 'success')
    } catch (error) {
      notify(error?.message || '头像上传失败', 'error')
    } finally {
      setUploadingAvatar(false)
    }
  }

  async function handleSubmit(event) {
    event.preventDefault()
    if (!uid || saving || uploadingAvatar || hydratingProfile) return

    const payload = changedProfilePayload(uid, form, dirtyFields)
    if (Object.keys(payload).length <= 1) {
      notify('请至少填写一项资料', 'error')
      return
    }

    setSaving(true)
    try {
      const resp = await updateAccountProfile(payload)
      if (!isSuccess(resp)) {
        notify(retMsg(resp, '资料保存失败'), 'error')
        return
      }

      notify('资料已保存', 'success')
      try {
        const refreshedProfile = await accountEditableProfile(uid, true)
        onProfileUpdated?.({
          ...(profile || {}),
          ...(refreshedProfile || {}),
          ...payload,
        })
      } catch {
        onProfileUpdated?.({ ...(profile || {}), ...payload })
      }
      onClose?.()
    } catch (error) {
      notify(error?.message || '资料保存失败', 'error')
    } finally {
      setSaving(false)
    }
  }

  const avatarPreview = String(form.avatar || '').trim()

  return (
    <Modal
      open={open}
      onClose={() => {
        if (saving || uploadingAvatar || hydratingProfile) return
        onClose?.()
      }}
      title="编辑资料"
      width={isMobile ? 520 : 680}
      footer={(
        <>
          <button onClick={onClose} className="btn-secondary btn-sm" disabled={saving || uploadingAvatar || hydratingProfile}>取消</button>
          <button
            type="submit"
            form={`account-profile-editor-${uid}`}
            disabled={saving || uploadingAvatar || hydratingProfile}
            className="btn-primary btn-sm disabled:opacity-60"
          >
            {hydratingProfile ? '正在读取…' : saving ? '保存中…' : '保存资料'}
          </button>
        </>
      )}
    >
      <form id={`account-profile-editor-${uid}`} className="space-y-4" onSubmit={handleSubmit}>
        <div className="text-[12px]" style={{ color: 'var(--text-muted)' }}>
          昵称、简介、头像、性别、生日与学校会通过闲鱼资料接口保存
        </div>
        {hydratingProfile ? (
          <div className="text-[12px]" style={{ color: 'var(--primary)' }}>
            正在读取当前账号资料…
          </div>
        ) : null}

        <div className={isMobile ? 'space-y-3' : 'grid grid-cols-2 gap-3'}>
          <ProfileInput
            label="昵称"
            value={form.snsNick}
            maxLength={30}
            onChange={(value) => setField('snsNick', value)}
            placeholder="请输入昵称"
          />
          <label className={isMobile ? 'block text-[12px]' : 'block text-[12px] col-span-2'} style={{ color: 'var(--text-secondary)' }}>
            <span className="mb-1.5 block">简介</span>
            <textarea
              aria-label="简介"
              className="w-full min-h-[82px] resize-y rounded-[12px] px-3 py-2 text-[13px] outline-none transition-colors"
              style={{
                background: 'var(--surface-soft)',
                border: '1px solid var(--control-border)',
                color: 'var(--text-main)',
              }}
              value={form.signature}
              maxLength={200}
              onChange={(event) => setField('signature', event.target.value)}
              placeholder="写一段主页简介"
            />
          </label>
          <label className={isMobile ? 'block text-[12px]' : 'block text-[12px] col-span-2'} style={{ color: 'var(--text-secondary)' }}>
            <span className="mb-1.5 block">头像</span>
            <div
              className={isMobile ? 'space-y-3' : 'flex items-center gap-3 rounded-[14px] p-3'}
              style={isMobile ? undefined : { background: 'var(--surface-soft)', border: '1px solid var(--control-border)' }}
            >
              <div className="flex items-center gap-3">
                {avatarPreview ? (
                  <img
                    src={avatarPreview}
                    alt=""
                    className="w-14 h-14 rounded-[16px] object-cover"
                    style={{ border: '1px solid var(--control-border)' }}
                  />
                ) : (
                  <div className="liquid-avatar w-14 h-14 rounded-[16px] flex items-center justify-center text-[20px] font-semibold">
                    {(form.snsNick || account?.uid || '?').charAt(0).toUpperCase()}
                  </div>
                )}
                <div className="min-w-0">
                  <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
                    {uploadingAvatar ? '头像上传中…' : '上传头像图片'}
                  </div>
                  <div className="mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                    选择本地图片后会先上传，再随资料一起保存
                  </div>
                </div>
              </div>
              <div className={isMobile ? '' : 'ml-auto'}>
                <input
                  aria-label="头像"
                  type="file"
                  accept="image/*"
                  onChange={handleAvatarChange}
                  disabled={uploadingAvatar || saving}
                  className="block w-full text-[12px]"
                />
              </div>
            </div>
          </label>
          <label className="block text-[12px]" style={{ color: 'var(--text-secondary)' }}>
            <span className="mb-1.5 block">性别</span>
            <select
              aria-label="性别"
              className="w-full rounded-[12px] px-3 py-2 text-[13px] outline-none transition-colors"
              style={{
                background: 'var(--surface-soft)',
                border: '1px solid var(--control-border)',
                color: 'var(--text-main)',
              }}
              value={form.gender}
              onChange={(event) => setField('gender', event.target.value)}
            >
              <option value="">不修改</option>
              <option value="0">保密</option>
              <option value="1">男</option>
              <option value="2">女</option>
            </select>
          </label>
          <ProfileInput
            label="生日"
            value={form.birthday}
            type="date"
            onChange={(value) => setField('birthday', value)}
          />
          <ProfileInput
            label="城市"
            value={form.city}
            maxLength={30}
            onChange={(value) => setField('city', value)}
            placeholder="例如 杭州"
          />
          <ProfileInput
            label="学校"
            value={form.university}
            maxLength={40}
            onChange={(value) => setField('university', value)}
            placeholder="例如 湖南财政经济学院"
          />
        </div>
      </form>
    </Modal>
  )
}
