// @ts-nocheck
import { useState, useEffect, useMemo, useRef } from 'react'
import {
  BellRing, Building2, MessageCircle, Plus, RefreshCw, Save, Send,
  TestTube2, Trash2, Users, ChevronRight, Copy,
} from 'lucide-react'
import { useAccounts } from '../../hooks/useAccounts'
import { getAuthToken } from '../../api/client'
import {
  getNotificationChannels, createNotificationChannel, updateNotificationChannel,
  deleteNotificationChannel, testNotificationChannel,
  getFeishuChannelMembers, createFeishuChannelInvite, deleteFeishuChannelMember,
  getMessageNotifications, getMessageNotificationSettings, setMessageNotification,
  updateMessageNotificationSettings,
} from '../../api/index'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { EmptyState } from '../../components/EmptyState'
import { Modal } from '../../components/Modal'
import { useNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import {
  ADDABLE_CHANNEL_DEFINITIONS, getChannelDefinition,
  buildChannelConfigDraft, serializeChannelConfigDraft,
} from './channelCatalog'
import { buildCallbackUrls } from './callbackUrls'
import { presentNotificationRuntime } from './runtimePresentation'
import { WechatQrLoginModal } from './WechatQrLoginModal'
import { FeishuQrRegistrationModal } from './FeishuQrRegistrationModal'
import { FeishuCollaboratorPanel } from './FeishuCollaboratorPanel'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import { SensitiveValue, SensitiveVisibilityToggle, useSensitiveAvatarInitial } from '../../components/SensitiveValue'

const ICONS = {
  wechat: Building2, dingtalk: Send, feishu: BellRing,
  openclaw_weixin: MessageCircle,
}

function iconFor(type) { return ICONS[type] || BellRing }

function NotificationAccountAvatar({ value }: { value: string }) {
  const initial = useSensitiveAvatarInitial(value)
  return <div className="surface-soft w-10 h-10 rounded-full flex items-center justify-center text-[13px] font-bold shrink-0" style={{ color: 'var(--text-main)' }}>{initial}</div>
}

const emptyEditor = (type = 'feishu') => ({
  id: null, name: `${getChannelDefinition(type).label} 通知`,
  type, enabled: true, configDraft: buildChannelConfigDraft(type),
})

async function copyText(text, onOk) {
  if (!text) return
  try {
    await navigator.clipboard?.writeText(text)
    onOk?.()
  } catch { /* noop */ }
}

export default function DesktopNotifications() {
  const { accounts, loadAccounts } = useAccounts()
  const { showConfirm } = useAppDialog()
  const [channels, setChannels] = useState([])
  const [bindingsByUid, setBindingsByUid] = useState({})
  const [platformMessagesEnabled, setPlatformMessagesEnabled] = useState(true)
  const [loading, setLoading] = useState(false)
  const [settingsSaving, setSettingsSaving] = useState(false)

  const [editorOpen, setEditorOpen] = useState(false)
  const [bindOpen, setBindOpen] = useState(false)
  const [createPickerOpen, setCreatePickerOpen] = useState(false)
  const [qrOpen, setQrOpen] = useState(false)
  const [feishuQrOpen, setFeishuQrOpen] = useState(false)
  const [editor, setEditor] = useState(emptyEditor())
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [bindDraftByUid, setBindDraftByUid] = useState({})
  const [bindSaving, setBindSaving] = useState(false)
  const [feishuMembersLoading, setFeishuMembersLoading] = useState(false)
  const [feishuMembers, setFeishuMembers] = useState([])
  const [feishuActiveInvite, setFeishuActiveInvite] = useState(null)
  const [feishuInviteCreating, setFeishuInviteCreating] = useState(false)
  const [feishuRemovingSenderId, setFeishuRemovingSenderId] = useState('')
  const notify = useNotify()

  const activeAccounts = useMemo(
    () => filterBusinessAccounts(accounts),
    [accounts],
  )
  const [authToken, setAuthToken] = useState(() => getAuthToken())
  const activeAccountKey = useMemo(
    () => activeAccounts.map((account) => account.uid).join('|'),
    [activeAccounts],
  )
  const activeAccountsRef = useRef(activeAccounts)
  const activeAccountKeyRef = useRef(activeAccountKey)
  const editorIdRef = useRef(editor.id)
  activeAccountsRef.current = activeAccounts
  activeAccountKeyRef.current = activeAccountKey
  editorIdRef.current = editor.id

  useEffect(() => {
    const syncToken = () => setAuthToken(getAuthToken())
    window.addEventListener('xy:authChanged', syncToken)
    window.addEventListener('storage', syncToken)
    return () => {
      window.removeEventListener('xy:authChanged', syncToken)
      window.removeEventListener('storage', syncToken)
    }
  }, [])

  useEffect(() => {
    clearNotificationState()
    void reload()
  }, [authToken])

  function clearNotificationState() {
    setChannels([])
    setBindingsByUid({})
    setPlatformMessagesEnabled(true)
    setBindDraftByUid({})
    setBindSaving(false)
    setSettingsSaving(false)
    setFeishuMembersLoading(false)
    setFeishuMembers([])
    setFeishuActiveInvite(null)
    setFeishuInviteCreating(false)
    setFeishuRemovingSenderId('')
    setEditorOpen(false)
    setBindOpen(false)
    setCreatePickerOpen(false)
    setQrOpen(false)
    setFeishuQrOpen(false)
    setLoading(false)
  }

  function isScopeCurrent(requestToken, requestAccountKey = activeAccountKeyRef.current) {
    return getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey
  }

  function pruneBindings(bindings, scopedAccounts = activeAccountsRef.current) {
    const activeUidSet = new Set(filterBusinessAccounts(scopedAccounts || []).map((account) => account.uid))
    return Object.fromEntries(
      Object.entries(bindings || {}).filter(([uid]) => activeUidSet.has(uid)),
    )
  }

  function buildBindDraft(channelId, scopedAccounts = activeAccountsRef.current, scopedBindings = bindingsByUid) {
    const draft = {}
    for (const account of scopedAccounts || []) {
      draft[account.uid] = Boolean((scopedBindings?.[account.uid] || []).find((binding) => (
        binding.channel_id === channelId && binding.enabled
      )))
    }
    return draft
  }

  function clearFeishuCollaboratorState() {
    setFeishuMembersLoading(false)
    setFeishuMembers([])
    setFeishuActiveInvite(null)
    setFeishuInviteCreating(false)
    setFeishuRemovingSenderId('')
  }

  async function loadFeishuCollaborators(channelId, requestToken = getAuthToken(), requestAccountKey = activeAccountKeyRef.current) {
    if (!channelId) return
    setFeishuMembersLoading(true)
    try {
      const response = await getFeishuChannelMembers(channelId)
      if (!isScopeCurrent(requestToken, requestAccountKey) || editorIdRef.current !== channelId) return
      setFeishuMembers(Array.isArray(response?.members) ? response.members : [])
      setFeishuActiveInvite(response?.active_invite || null)
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey) && editorIdRef.current === channelId) {
        notify.error(e.message || '协作者加载失败')
      }
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey) && editorIdRef.current === channelId) {
        setFeishuMembersLoading(false)
      }
    }
  }

  async function reload() {
    const requestToken = getAuthToken()
    setLoading(true)
    try {
      const [loadedAccounts, list, bindings, settings] = await Promise.all([
        loadAccounts(),
        getNotificationChannels(),
        getMessageNotifications(),
        getMessageNotificationSettings(),
      ])
      if (getAuthToken() !== requestToken) {
        clearNotificationState()
        return
      }
      const nextChannels = Array.isArray(list) ? list : []
      const nextBindings = pruneBindings(bindings, Array.isArray(loadedAccounts) ? loadedAccounts : activeAccountsRef.current)
      setPlatformMessagesEnabled(settings?.platformMessagesEnabled !== false)
      setChannels(nextChannels)
      setBindingsByUid(nextBindings)
      return { channels: nextChannels, bindings: nextBindings }
    } catch (e) {
      if (getAuthToken() === requestToken) notify.error(e.message || '加载失败')
    }
    finally {
      if (getAuthToken() === requestToken) setLoading(false)
    }
  }

  useEffect(() => {
    const editorIsFeishu = Boolean(editorOpen && editor.id && (editor.type === 'feishu' || editor.type === 'lark'))
    if (!editorIsFeishu) {
      clearFeishuCollaboratorState()
      return
    }
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    void loadFeishuCollaborators(editor.id, requestToken, requestAccountKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editorOpen, editor.id, editor.type])

  function openCreate() {
    setCreatePickerOpen(true)
  }

  async function togglePlatformMessages(nextEnabled) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setSettingsSaving(true)
    try {
      await updateMessageNotificationSettings(nextEnabled)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setPlatformMessagesEnabled(nextEnabled)
      notify.success(nextEnabled ? '已开启通知消息和互动消息' : '已关闭通知消息和互动消息')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '保存失败')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setSettingsSaving(false)
    }
  }

  function startCreate(type) {
    setCreatePickerOpen(false)
    if (type === 'openclaw_weixin') {
      setQrOpen(true)
      return
    }
    if (type === 'feishu') {
      setFeishuQrOpen(true)
      return
    }
    setEditor(emptyEditor(type))
    setEditorOpen(true)
  }

  function openEdit(channel) {
    setEditor({
      id: channel.id,
      name: channel.name || '',
      type: channel.type || 'feishu',
      enabled: Boolean(channel.enabled),
      configDraft: buildChannelConfigDraft(channel.type, channel.config, channel.runtime),
    })
    setEditorOpen(true)
  }

  function openBind(channel) {
    setEditor({
      id: channel.id, name: channel.name, type: channel.type,
      enabled: channel.enabled, configDraft: {},
    })
    setBindDraftByUid(buildBindDraft(channel.id))
    setBindOpen(true)
  }

  function openFeishuRebind() {
    setEditorOpen(false)
    setFeishuQrOpen(true)
  }

  function setField(key, value) {
    setEditor((e) => ({ ...e, configDraft: { ...e.configDraft, [key]: value } }))
  }

  // Backend auto-creates the WeChat channel on confirm — we just close the
  // modal, show a toast, and refresh the channel list so the new row shows up.
  async function onWechatChannelCreated(_channel) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setQrOpen(false)
    if (!isScopeCurrent(requestToken, requestAccountKey)) return
    notify.success('微信渠道已创建，已绑定所有账号')
    await reload()
  }

  function normalizeCreatedFeishuChannel(channel) {
    if (!channel?.id) return null
    return {
      id: channel.id,
      name: channel.name || '飞书扫码',
      type: channel.type || 'feishu',
      enabled: channel.enabled !== false,
      config: channel.config || {},
      runtime: channel.runtime || {},
    }
  }

  async function onFeishuChannelCreated(channel) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setFeishuQrOpen(false)
    if (!isScopeCurrent(requestToken, requestAccountKey)) return
    notify.success('飞书渠道已创建')
    const result = await reload()
    if (!isScopeCurrent(requestToken, requestAccountKey)) return
    const createdId = channel?.id
    const freshChannel = createdId
      ? result?.channels?.find((item) => String(item.id) === String(createdId))
      : null
    const nextChannel = freshChannel || normalizeCreatedFeishuChannel(channel)
    if (nextChannel) openEdit(nextChannel)
  }

  async function createFeishuInvite() {
    if (!editor.id) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setFeishuInviteCreating(true)
    try {
      const response = await createFeishuChannelInvite(editor.id)
      if (!isScopeCurrent(requestToken, requestAccountKey) || editor.id == null) return
      setFeishuActiveInvite(response?.invite || null)
      notify.success('邀请码已生成')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '邀请码生成失败')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setFeishuInviteCreating(false)
    }
  }

  async function removeFeishuMember(member) {
    if (!editor.id || !member?.sender_id) return
    const confirmed = await showConfirm(`确认移除协作者「${member.sender_name || member.sender_id}」？`, {
      title: '移除飞书协作者',
      confirmLabel: '移除',
      danger: true,
      width: 460,
    })
    if (!confirmed) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setFeishuRemovingSenderId(member.sender_id)
    try {
      await deleteFeishuChannelMember(editor.id, member.sender_id)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      notify.success('协作者已移除')
      await loadFeishuCollaborators(editor.id, requestToken, requestAccountKey)
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '移除失败')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setFeishuRemovingSenderId('')
    }
  }

  async function save() {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setSaving(true)
    try {
      const def = getChannelDefinition(editor.type)
      const payload = {
        name: editor.name.trim() || `${def.label} 通知`,
        type: editor.type,
        enabled: Boolean(editor.enabled),
        config: serializeChannelConfigDraft(editor.type, editor.configDraft),
      }
      if (editor.id) {
        await updateNotificationChannel(editor.id, payload)
        if (!isScopeCurrent(requestToken, requestAccountKey)) return
        notify.success('已更新')
      } else {
        const r = await createNotificationChannel(payload)
        if (!isScopeCurrent(requestToken, requestAccountKey)) return
        setEditor((e) => ({ ...e, id: r.channel?.id ?? e.id }))
        notify.success('已创建')
      }
      setEditorOpen(false)
      await reload()
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '保存失败')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setSaving(false)
    }
  }

  async function remove() {
    if (!editor.id) return
    const confirmed = await showConfirm('确认删除该通知渠道？账号绑定也会被清除。', {
      title: '删除通知渠道',
      confirmLabel: '删除',
      danger: true,
      width: 460,
    })
    if (!confirmed) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    try {
      await deleteNotificationChannel(editor.id)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setEditorOpen(false)
      await reload()
      notify.success('已删除')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '删除失败')
    }
  }

  async function runTest() {
    if (!editor.id) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setTesting(true)
    try {
      await testNotificationChannel(editor.id, '')
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      notify.success('测试通知已发送')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '测试失败')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setTesting(false)
    }
  }

  function isBound(uid, channelId) {
    return Boolean((bindingsByUid[uid] || []).find((b) => b.channel_id === channelId && b.enabled))
  }

  function countBound(channelId) {
    let n = 0
    for (const list of Object.values(bindingsByUid)) {
      if ((list || []).some((b) => b.channel_id === channelId && b.enabled)) n++
    }
    return n
  }

  function setBindDraft(uid, enabled) {
    setBindDraftByUid((current) => ({ ...current, [uid]: enabled }))
  }

  function setAllBindDraft(enabled) {
    setBindDraftByUid((current) => {
      const next = { ...current }
      for (const account of activeAccountsRef.current) {
        next[account.uid] = enabled
      }
      return next
    })
  }

  async function saveBindings() {
    if (!editor.id) return
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    const channelId = editor.id
    const changes = activeAccountsRef.current
      .map((account) => {
        const nextEnabled = Boolean(bindDraftByUid[account.uid])
        const currentEnabled = isBound(account.uid, channelId)
        return nextEnabled === currentEnabled ? null : {
          uid: account.uid,
          enabled: nextEnabled,
        }
      })
      .filter(Boolean)

    if (!changes.length) {
      setBindOpen(false)
      return
    }

    setBindSaving(true)
    try {
      await Promise.all(changes.map((change) => (
        setMessageNotification(change.uid, channelId, change.enabled)
      )))
      const next = await getMessageNotifications()
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setBindingsByUid(pruneBindings(next))
      setBindOpen(false)
      notify.success('账号绑定已保存')
    } catch (e) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '绑定失败')
    }
    finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setBindSaving(false)
    }
  }

  const def = getChannelDefinition(editor.type)
  const editorChannel = channels.find((c) => c.id === editor.id)
  const editorIsFeishu = Boolean(editor.id && (editor.type === 'feishu' || editor.type === 'lark'))
  const bindDraftSelectedCount = activeAccounts.filter((account) => Boolean(bindDraftByUid[account.uid])).length
  const bindDraftAllSelected = activeAccounts.length > 0 && bindDraftSelectedCount === activeAccounts.length
  const callbackUrls = editorChannel && typeof window !== 'undefined'
    ? buildCallbackUrls(window.location.origin, editorChannel) : []

  return (
    <>
      <HubTabs
        tabs={HUB_PRESETS.messages}
        right={
          <div className="flex items-center gap-1">
            <SensitiveVisibilityToggle compact className="btn-ghost btn-sm" />
            <button onClick={reload} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40">
              <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
            </button>
            <button onClick={openCreate} className="btn-primary btn-sm">
              <Plus className="w-3.5 h-3.5" /> 新增
            </button>
          </div>
        }
      />

      <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6">
        <div className="mb-3 card p-4">
          <label className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                通知消息 / 互动消息
              </div>
              <div className="mt-1 text-[12px]" style={{ color: 'var(--text-muted)' }}>
                关闭后不再展示平台通知会话，也不会向外部渠道推送这类消息。
              </div>
            </div>
            <input
              type="checkbox"
              aria-label="通知消息 / 互动消息"
              checked={platformMessagesEnabled}
              disabled={settingsSaving}
              onChange={(e) => togglePlatformMessages(e.target.checked)}
              className="mt-0.5 h-4 w-4 rounded accent-[var(--primary)] disabled:opacity-50"
            />
          </label>
        </div>

        {!channels.length && !loading ? (
          <EmptyState icon={BellRing} title="暂无通知渠道" description="点击右上角新增" />
        ) : (
          <div className="space-y-2.5">
            {channels.map((channel) => {
              const def = getChannelDefinition(channel.type)
              const Icon = iconFor(channel.type)
              const isWechat = channel.type === 'openclaw_weixin'
              const isFeishu = channel.type === 'feishu' || channel.type === 'lark'
              const bound = countBound(channel.id)
              const runtime = presentNotificationRuntime(channel)
              return (
                <div key={channel.id} className="card overflow-hidden">
                  {/* Header — clicks open the editor for non-WeChat rows. WeChat rows
                      are opaque (no config the user needs to see), so the click is a no-op. */}
                  <div
                    onClick={isWechat ? undefined : () => openEdit(channel)}
                    className={`w-full flex items-center gap-3 px-4 py-3 text-left transition-colors ${isWechat ? '' : 'cursor-pointer'}`}
                  >
                    <div className="w-10 h-10 rounded-2xl shrink-0 flex items-center justify-center text-white"
                      style={{ background: def.accent }}>
                      <Icon className="w-5 h-5" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-[14px] font-semibold truncate" style={{ color: 'var(--text-main)' }}>
                          {channel.name}
                        </span>
                        <span className="status-badge shrink-0 px-1.5 py-0.5 rounded text-[10px] font-bold"
                          style={channel.enabled ? { background: 'var(--status-success-bg)', color: 'var(--status-success-text)' } : { background: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' }}>
                          {channel.enabled ? (isWechat ? '在线' : '启用') : '停用'}
                        </span>
                      </div>
                      <div className="mt-1 flex items-center gap-2 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                        <span>{def.label}</span>
                        <span className="inline-flex items-center gap-1">
                          <span className="w-1.5 h-1.5 rounded-full" style={{ background: runtime.dot }} />
                          {runtime.label}
                        </span>
                        <span>· {isWechat ? '已绑定所有账号' : isFeishu ? '支持多成员协作' : `${bound} 个账号`}</span>
                      </div>
                      {!!runtime.facts.length && (
                        <div className="mt-2 flex flex-wrap gap-1.5" data-testid={`runtime-facts-${channel.id}`}>
                          {runtime.facts.map((fact) => (
                            <span
                              key={fact.key}
                              className="status-badge inline-flex max-w-full items-center rounded-full px-2 py-1 text-[10px]"
                              style={fact.tone === 'danger' ? { background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' } : { background: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' }}
                              title={fact.detail || fact.value}
                            >
                              <span className="font-semibold">{fact.label}：</span>
                              <span className="ml-1 truncate">{fact.value}</span>
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                    {!isWechat && <ChevronRight className="w-4 h-4 shrink-0" style={{ color: 'var(--text-muted)' }} />}
                  </div>
                  {/* Footer — WeChat rows expose only delete (re-add = re-scan QR). */}
                  {isWechat ? (
                    <div className="flex border-t" style={{ borderColor: 'var(--border-light)' }}>
                      <button
                        onClick={async () => {
                          const confirmed = await showConfirm('确认删除该微信通知渠道？', {
                            title: '删除微信通知渠道',
                            confirmLabel: '删除',
                            danger: true,
                          })
                          if (!confirmed) return
                          const requestToken = getAuthToken()
                          const requestAccountKey = activeAccountKeyRef.current
                          try {
                            await deleteNotificationChannel(channel.id)
                            if (!isScopeCurrent(requestToken, requestAccountKey)) return
                            await reload()
                            notify.success('已删除')
                          } catch (e) {
                            if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '删除失败')
                          }
                        }}
                        className="flex-1 py-2.5 text-[12px] font-medium flex items-center justify-center gap-1 cursor-pointer"
                        style={{ color: 'var(--danger)' }}
                      >
                        <Trash2 className="w-3.5 h-3.5" /> 删除
                      </button>
                    </div>
                  ) : (
                    <div className="flex border-t" style={{ borderColor: 'var(--border-light)' }}>
                      <button
                        onClick={() => (isFeishu ? openEdit(channel) : openBind(channel))}
                        className="flex-1 py-2.5 text-[12px] font-medium flex items-center justify-center gap-1 cursor-pointer"
                        style={{ color: 'var(--primary)' }}
                      >
                        <Users className="w-3.5 h-3.5" /> {isFeishu ? '协作者' : '绑定账号'}
                      </button>
                      <div className="w-px" style={{ background: 'var(--border-light)' }} />
                      <button
                        onClick={async () => {
                          const requestToken = getAuthToken()
                          const requestAccountKey = activeAccountKeyRef.current
                          try {
                            await testNotificationChannel(channel.id, '')
                            if (!isScopeCurrent(requestToken, requestAccountKey)) return
                            notify.success('测试已发送')
                          } catch (e) {
                            if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(e.message || '测试失败')
                          }
                        }}
                        className="flex-1 py-2.5 text-[12px] font-medium flex items-center justify-center gap-1 cursor-pointer"
                        style={{ color: 'var(--text-secondary)' }}
                      >
                        <TestTube2 className="w-3.5 h-3.5" /> 测试
                      </button>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>

      <Modal
        open={createPickerOpen}
        onClose={() => setCreatePickerOpen(false)}
        title="选择渠道类型"
        width={560}
      >
        <div className="grid grid-cols-2 gap-3">
          {ADDABLE_CHANNEL_DEFINITIONS.map((channelDefinition) => {
            const Icon = iconFor(channelDefinition.type)
            return (
              <button
                key={channelDefinition.type}
                type="button"
                aria-label={channelDefinition.label}
                onClick={() => startCreate(channelDefinition.type)}
                className="surface-card flex cursor-pointer items-center gap-3 rounded-2xl border px-4 py-3 text-left transition-colors hover:border-[var(--primary)]"
                style={{ borderColor: 'var(--border-light)' }}
              >
                <span
                  className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl text-white"
                  style={{ background: channelDefinition.accent }}
                >
                  <Icon className="h-5 w-5" />
                </span>
                <span className="min-w-0">
                  <span className="block text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                    {channelDefinition.label}
                  </span>
                  <span className="mt-1 line-clamp-2 block text-[11px]" style={{ color: 'var(--text-muted)' }}>
                    {channelDefinition.description}
                  </span>
                </span>
              </button>
            )
          })}
        </div>
      </Modal>

      {/* 编辑/新增渠道 */}
      <Modal
        open={editorOpen} onClose={() => setEditorOpen(false)}
        title={editor.id ? '编辑渠道' : `新增 ${def.label}`}
      >
        <div className="space-y-4">
          {editorIsFeishu ? (
            <div className="space-y-4" data-testid="desktop-feishu-compact-panel">
              <div className="surface-soft rounded-2xl p-4">
                <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{editor.name || '飞书机器人'}</div>
                <div className="mt-1 text-[12px]" style={{ color: 'var(--text-muted)' }}>
                  扫码接入已保存机器人凭证。飞书侧使用菜单或卡片按钮操作，无需在这里维护 App 配置。
                </div>
                {!!editorChannel && (() => {
                  const runtime = presentNotificationRuntime(editorChannel)
                  return (
                    <div className="mt-3 grid gap-2 sm:grid-cols-2" data-testid="desktop-runtime-panel">
                      {runtime.facts.length ? runtime.facts.map((fact) => (
                        <div key={fact.key} className="surface-card rounded-xl px-3 py-2">
                          <div className="text-[11px] font-medium" style={{ color: 'var(--text-secondary)' }}>{fact.label}</div>
                          <div className="mt-1 text-[11px] break-words" style={{ color: fact.tone === 'danger' ? 'var(--status-danger-text)' : 'var(--text-main)' }}>{fact.value}</div>
                        </div>
                      )) : (
                        <div className="surface-card rounded-xl px-3 py-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                          当前运行状态：{runtime.label}
                        </div>
                      )}
                    </div>
                  )
                })()}
              </div>

              <FeishuCollaboratorPanel
                variant="desktop"
                loading={feishuMembersLoading}
                members={feishuMembers}
                activeInvite={feishuActiveInvite}
                inviteBusy={feishuInviteCreating}
                removingSenderId={feishuRemovingSenderId}
                onGenerateInvite={createFeishuInvite}
                onRemoveMember={removeFeishuMember}
                onCopyText={(text, message) => copyText(text, () => notify.success(message || '已复制'))}
              />
            </div>
          ) : (
            <>
              <label className="block">
                <span className="label">渠道名称</span>
                <input
                  className="input"
                  value={editor.name}
                  onChange={(e) => setEditor((s) => ({ ...s, name: e.target.value }))}
                  placeholder="便于识别的名字"
                />
              </label>

              <label className="flex items-center justify-between rounded-2xl border px-4 py-3 text-[13px] cursor-pointer"
                style={{ borderColor: 'var(--border-light)' }}>
                <span style={{ color: 'var(--text-main)' }}>启用此渠道</span>
                <input type="checkbox" checked={editor.enabled}
                  onChange={(e) => setEditor((s) => ({ ...s, enabled: e.target.checked }))}
                  className="h-4 w-4 rounded accent-[var(--primary)]" />
              </label>

              {/* 字段 */}
              <div className="space-y-3">
                {def.fields.map((f) => {
                  const val = editor.configDraft[f.key] ?? ''
                  if (f.input === 'boolean') {
                    return (
                      <label key={f.key} className="flex items-center justify-between rounded-2xl border px-4 py-3 text-[13px] cursor-pointer"
                        style={{ borderColor: 'var(--border-light)' }}>
                        <span style={{ color: 'var(--text-main)' }}>{f.label}</span>
                        <input type="checkbox" checked={Boolean(val)}
                          onChange={(e) => setField(f.key, e.target.checked)}
                          className="h-4 w-4 rounded accent-[var(--primary)]" />
                      </label>
                    )
                  }
                  if (f.input === 'textarea') {
                    return (
                      <label key={f.key} className="block">
                        <span className="label">{f.label}</span>
                        <textarea
                          className={`input min-h-[110px] resize-y ${f.monospace ? 'font-mono text-[12px]' : ''}`}
                          value={val}
                          onChange={(e) => setField(f.key, e.target.value)}
                          placeholder={f.placeholder || ''}
                        />
                      </label>
                    )
                  }
                  if (f.input === 'select') {
                    return (
                      <label key={f.key} className="block">
                        <span className="label">{f.label}</span>
                        <select
                          className="input"
                          value={val}
                          onChange={(e) => setField(f.key, e.target.value)}
                        >
                          {(f.options || []).map((option) => (
                            <option key={option.value} value={option.value}>
                              {option.label}
                            </option>
                          ))}
                        </select>
                      </label>
                    )
                  }
                  return (
                    <label key={f.key} className="block">
                      <span className="label">{f.label}</span>
                      <input
                        type={f.input === 'password' ? 'password' : 'text'}
                        className="input"
                        value={val}
                        onChange={(e) => setField(f.key, e.target.value)}
                        placeholder={f.placeholder || ''}
                      />
                    </label>
                  )
                })}
              </div>
            </>
          )}

          {/* Callback URL（已存在的渠道） */}
          {!editorIsFeishu && !!callbackUrls.length && (
            <div className="surface-soft rounded-2xl p-3 space-y-3">
              <div>
                <div className="text-[12px] font-semibold mb-2" style={{ color: 'var(--text-main)' }}>
                  Callback 地址（配置到渠道后台）
                </div>
                <div className="space-y-2">
                  {callbackUrls.map((u) => (
                    <div key={`${u.method}-${u.url}`} className="surface-card rounded-xl px-3 py-2">
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-[11px] font-medium" style={{ color: 'var(--text-secondary)' }}>{u.label}</span>
                        <button
                          onClick={() => copyText(u.url, () => notify.success('已复制'))}
                          className="p-1 cursor-pointer" aria-label="复制"
                        >
                          <Copy className="w-3.5 h-3.5" style={{ color: 'var(--primary)' }} />
                        </button>
                      </div>
                      <div className="mt-1 font-mono text-[11px] break-all" style={{ color: 'var(--text-muted)' }}>{u.url}</div>
                    </div>
                  ))}
                </div>
              </div>

              {!!editorChannel && (() => {
                const runtime = presentNotificationRuntime(editorChannel)
                return (
                  <div className="space-y-2" data-testid="desktop-runtime-panel">
                    <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                      运行时诊断
                    </div>
                    <div className="grid gap-2 sm:grid-cols-2">
                      {runtime.facts.map((fact) => (
                        <div key={fact.key} className="surface-card rounded-xl px-3 py-2">
                          <div className="text-[11px] font-medium" style={{ color: 'var(--text-secondary)' }}>{fact.label}</div>
                          <div className="mt-1 text-[11px] break-words" style={{ color: fact.tone === 'danger' ? 'var(--status-danger-text)' : 'var(--text-main)' }}>{fact.value}</div>
                          {!!fact.detail && (
                            <div className="mt-1 text-[10px] break-words" style={{ color: 'var(--text-muted)' }}>{fact.detail}</div>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )
              })()}
            </div>
          )}

          {/* 操作按钮 */}
          {editorIsFeishu ? (
            <div className="flex flex-wrap gap-2 pt-2">
              <button onClick={runTest} disabled={testing}
                className="btn-primary flex-1 disabled:opacity-40">
                <TestTube2 className={`w-3.5 h-3.5 ${testing ? 'animate-spin' : ''}`} /> 测试
              </button>
              <button onClick={openFeishuRebind} className="btn-secondary flex-1">
                <RefreshCw className="w-3.5 h-3.5" /> 重新扫码绑定
              </button>
              <button onClick={remove} className="btn-secondary" style={{ color: 'var(--danger)' }} aria-label="删除">
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          ) : (
            <div className="flex flex-wrap gap-2 pt-2">
              <button onClick={save} disabled={saving}
                className="btn-primary flex-1 disabled:opacity-40">
                <Save className={`w-3.5 h-3.5 ${saving ? 'animate-spin' : ''}`} /> {editor.id ? '保存' : '创建'}
              </button>
              {editor.id && (
                <>
                  <button onClick={runTest} disabled={testing}
                    className="btn-secondary disabled:opacity-40">
                    <TestTube2 className={`w-3.5 h-3.5 ${testing ? 'animate-spin' : ''}`} /> 测试
                  </button>
                  <button onClick={remove} className="btn-secondary" style={{ color: 'var(--danger)' }}>
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </>
              )}
            </div>
          )}
        </div>
      </Modal>

      {/* 微信扫码登录弹窗 — 扫码确认后由后端自动建渠道 */}
      <WechatQrLoginModal
        open={qrOpen}
        onClose={() => setQrOpen(false)}
        onChannelCreated={onWechatChannelCreated}
      />

      <FeishuQrRegistrationModal
        open={feishuQrOpen}
        onClose={() => setFeishuQrOpen(false)}
        onChannelCreated={onFeishuChannelCreated}
      />

      {/* 绑定账号弹窗 */}
      <Modal
        open={bindOpen} onClose={() => setBindOpen(false)}
        title={`绑定账号 — ${editor.name}`}
        footer={
          <div className="flex w-full items-center justify-end gap-2">
            <button
              type="button"
              onClick={() => setBindOpen(false)}
              disabled={bindSaving}
              className="btn-secondary disabled:opacity-40"
            >
              取消
            </button>
            <button
              type="button"
              onClick={saveBindings}
              disabled={bindSaving}
              className="btn-primary disabled:opacity-40"
            >
              <Save className={`w-3.5 h-3.5 ${bindSaving ? 'animate-spin' : ''}`} /> 保存
            </button>
          </div>
        }
      >
        {!activeAccounts.length ? (
          <EmptyState icon={Users} title="暂无活跃账号" description="先登录账号" />
        ) : (
          <div className="space-y-3">
            <div className="surface-soft flex items-center justify-between gap-3 rounded-2xl px-3 py-2">
              <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                已选择 <span className="font-semibold" style={{ color: 'var(--text-main)' }}>{bindDraftSelectedCount}</span> / {activeAccounts.length} 个账号
              </div>
              <button
                type="button"
                onClick={() => setAllBindDraft(!bindDraftAllSelected)}
                disabled={bindSaving}
                className="btn-secondary btn-sm disabled:opacity-40"
              >
                {bindDraftAllSelected ? '全部取消' : '全选'}
              </button>
            </div>
            {activeAccounts.map((a) => {
              const bound = Boolean(bindDraftByUid[a.uid])
              return (
                <label key={a.uid}
                  className="flex items-center gap-3 rounded-2xl border px-3 py-2.5 cursor-pointer"
                  style={{ borderColor: 'var(--border-light)' }}>
                  {a.avatar ? (
                    <img src={a.avatar} alt="" className="w-10 h-10 rounded-full object-cover shrink-0" />
                  ) : (
                    <NotificationAccountAvatar value={a.nick || a.uid} />
                  )}
                  <div className="flex-1 min-w-0">
                    <div className="text-[13px] font-medium truncate" style={{ color: 'var(--text-main)' }}>
                      <SensitiveValue kind="account" value={a.nick || a.uid} />
                    </div>
                    <div className="text-[11px] truncate font-mono" style={{ color: 'var(--text-muted)' }}><SensitiveValue kind="uid" value={a.uid} /></div>
                  </div>
                  <input type="checkbox" disabled={bindSaving} checked={bound}
                    onChange={(e) => setBindDraft(a.uid, e.target.checked)}
                    className="h-5 w-5 rounded accent-[var(--primary)]" />
                </label>
              )
            })}
          </div>
        )}
      </Modal>
    </>
  )
}
