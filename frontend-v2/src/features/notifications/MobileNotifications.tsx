// @ts-nocheck
import { useEffect, useMemo, useRef, useState } from 'react'
import {
  BellRing,
  Building2,
  ChevronRight,
  Copy,
  Link2,
  Loader2,
  MessageCircle,
  Plus,
  RefreshCw,
  Save,
  Send,
  ShieldCheck,
  TestTube2,
  Trash2,
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
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { BottomSheet } from '../../components/Modal'
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
import { SensitiveValue, SensitiveVisibilityToggle } from '../../components/SensitiveValue'
import { filterBusinessAccounts } from '../../utils/accountAccess'

async function copyText(text: string, onOk?: () => void) {
  if (!text) return
  try {
    await navigator.clipboard?.writeText(text)
    onOk?.()
  } catch { /* noop */ }
}

function Toggle({ on }: { on: boolean }) {
  return (
    <div
      className="relative h-6 w-11 shrink-0 rounded-full transition-colors"
      style={{ backgroundColor: on ? 'var(--success)' : 'var(--status-neutral-bg)' }}
    >
      <div
        className="absolute top-0.5 h-5 w-5 rounded-full transition-transform"
        style={{
          backgroundColor: 'var(--liquid-highlight)',
          boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.64), 0 2px 6px rgba(25,35,55,0.12)',
          transform: on ? 'translateX(22px)' : 'translateX(2px)',
        }}
      />
    </div>
  )
}

const ICONS: any = {
  wechat: Building2,
  dingtalk: Send,
  feishu: BellRing,
  openclaw_weixin: MessageCircle,
}

function iconFor(type: string) { return ICONS[type] || BellRing }

const emptyEditor = (type = 'feishu') => ({
  id: null,
  name: `${getChannelDefinition(type).label} 通知`,
  type,
  enabled: true,
  configDraft: buildChannelConfigDraft(type),
})

export default function MobileNotifications() {
  const { accounts, loadAccounts } = useAccounts()
  const { showConfirm } = useAppDialog()
  const [channels, setChannels] = useState<any[]>([])
  const [bindingsByUid, setBindingsByUid] = useState<any>({})
  const [platformMessagesEnabled, setPlatformMessagesEnabled] = useState(true)
  const [loading, setLoading] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)
  const [editor, setEditor] = useState<any>(emptyEditor())
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [bindDraftByUid, setBindDraftByUid] = useState<any>({})
  const [bindSaving, setBindSaving] = useState(false)
  const [createPickerOpen, setCreatePickerOpen] = useState(false)
  const [qrOpen, setQrOpen] = useState(false)
  const [feishuQrOpen, setFeishuQrOpen] = useState(false)
  const [settingsSaving, setSettingsSaving] = useState(false)
  const [feishuMembersLoading, setFeishuMembersLoading] = useState(false)
  const [feishuMembers, setFeishuMembers] = useState<any[]>([])
  const [feishuActiveInvite, setFeishuActiveInvite] = useState<any>(null)
  const [feishuInviteCreating, setFeishuInviteCreating] = useState(false)
  const [feishuRemovingSenderId, setFeishuRemovingSenderId] = useState('')
  const notify = useNotify()

  const activeAccounts = useMemo(() => filterBusinessAccounts(accounts), [accounts])
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
  const enabledChannels = useMemo(() => channels.filter((channel) => channel.enabled).length, [channels])
  const totalBindings = useMemo(() => {
    return activeAccounts.reduce((sum, account) => {
      const list = (bindingsByUid[account.uid] || []) as any[]
      return sum + list.filter((item) => item.enabled).length
    }, 0)
  }, [activeAccounts, bindingsByUid])

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
    setCreatePickerOpen(false)
    setQrOpen(false)
    setFeishuQrOpen(false)
    setLoading(false)
  }

  function isScopeCurrent(requestToken: string, requestAccountKey = activeAccountKeyRef.current) {
    return getAuthToken() === requestToken && activeAccountKeyRef.current === requestAccountKey
  }

  function pruneBindings(bindings: any, scopedAccounts = activeAccountsRef.current) {
    const activeUidSet = new Set(filterBusinessAccounts(scopedAccounts || []).map((account: any) => account.uid))
    return Object.fromEntries(
      Object.entries(bindings || {}).filter(([uid]) => activeUidSet.has(uid)),
    )
  }

  function buildBindDraft(channelId: any, scopedAccounts = activeAccountsRef.current, scopedBindings = bindingsByUid) {
    const draft: any = {}
    for (const account of scopedAccounts || []) {
      draft[account.uid] = Boolean(((scopedBindings?.[account.uid] || []) as any[]).find((binding: any) => (
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

  async function loadFeishuCollaborators(channelId: any, requestToken = getAuthToken(), requestAccountKey = activeAccountKeyRef.current) {
    if (!channelId) return
    setFeishuMembersLoading(true)
    try {
      const response: any = await getFeishuChannelMembers(channelId)
      if (!isScopeCurrent(requestToken, requestAccountKey) || editorIdRef.current !== channelId) return
      setFeishuMembers(Array.isArray(response?.members) ? response.members : [])
      setFeishuActiveInvite(response?.active_invite || null)
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey) && editorIdRef.current === channelId) {
        notify.error(error.message || '协作者加载失败')
      }
    } finally {
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
    } catch (error: any) {
      if (getAuthToken() === requestToken) notify.error(error.message || '加载失败')
    } finally {
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

  function openEdit(channel: any) {
    setEditor({
      id: channel.id,
      name: channel.name || '',
      type: channel.type || 'feishu',
      enabled: Boolean(channel.enabled),
      configDraft: buildChannelConfigDraft(channel.type, channel.config, channel.runtime),
    })
    setBindDraftByUid(buildBindDraft(channel.id))
    setEditorOpen(true)
  }

  async function togglePlatformMessages(nextEnabled: boolean) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setSettingsSaving(true)
    try {
      await updateMessageNotificationSettings(nextEnabled)
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      setPlatformMessagesEnabled(nextEnabled)
      notify.success(nextEnabled ? '已开启通知消息和互动消息' : '已关闭通知消息和互动消息')
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '保存失败')
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setSettingsSaving(false)
    }
  }

  function startCreate(type: string) {
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

  function openFeishuRebind() {
    setEditorOpen(false)
    setFeishuQrOpen(true)
  }

  function setField(key: string, value: any) {
    setEditor((current: any) => ({ ...current, configDraft: { ...current.configDraft, [key]: value } }))
  }

  async function onWechatChannelCreated(_channel: any) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setQrOpen(false)
    if (!isScopeCurrent(requestToken, requestAccountKey)) return
    notify.success('微信渠道已创建，已绑定所有账号')
    await reload()
  }

  function normalizeCreatedFeishuChannel(channel: any) {
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

  async function onFeishuChannelCreated(channel: any) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setFeishuQrOpen(false)
    if (!isScopeCurrent(requestToken, requestAccountKey)) return
    notify.success('飞书渠道已创建')
    const result = await reload()
    if (!isScopeCurrent(requestToken, requestAccountKey)) return
    const createdId = channel?.id
    const freshChannel = createdId
      ? result?.channels?.find((item: any) => String(item.id) === String(createdId))
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
      const response: any = await createFeishuChannelInvite(editor.id)
      if (!isScopeCurrent(requestToken, requestAccountKey) || editor.id == null) return
      setFeishuActiveInvite(response?.invite || null)
      notify.success('邀请码已生成')
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '邀请码生成失败')
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setFeishuInviteCreating(false)
    }
  }

  async function removeFeishuMember(member: any) {
    if (!editor.id || !member?.sender_id) return
    const confirmed = await showConfirm(`确认移除协作者「${member.sender_name || member.sender_id}」？`, {
      title: '移除飞书协作者',
      confirmLabel: '移除',
      danger: true,
      width: 420,
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
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '移除失败')
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setFeishuRemovingSenderId('')
    }
  }

  async function persistBindingDraft(channelId: any, requestToken: string, requestAccountKey: string) {
    const changes = activeAccountsRef.current
      .map((account: any) => {
        const nextEnabled = Boolean(bindDraftByUid[account.uid])
        const currentEnabled = isBound(account.uid, channelId)
        return nextEnabled === currentEnabled ? null : {
          uid: account.uid,
          enabled: nextEnabled,
        }
      })
      .filter(Boolean)

    if (!changes.length) return

    setBindSaving(true)
    await Promise.all(changes.map((change: any) => (
      setMessageNotification(change.uid, channelId, change.enabled)
    )))
    const next = await getMessageNotifications()
    if (!isScopeCurrent(requestToken, requestAccountKey)) return
    setBindingsByUid(pruneBindings(next))
  }

  async function save() {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    setSaving(true)
    try {
      const definition = getChannelDefinition(editor.type)
      const payload = {
        name: editor.name.trim() || `${definition.label} 通知`,
        type: editor.type,
        enabled: Boolean(editor.enabled),
        config: serializeChannelConfigDraft(editor.type, editor.configDraft),
      }
      if (editor.id) {
        await updateNotificationChannel(editor.id, payload)
        if (!isScopeCurrent(requestToken, requestAccountKey)) return
        await persistBindingDraft(editor.id, requestToken, requestAccountKey)
        if (!isScopeCurrent(requestToken, requestAccountKey)) return
        notify.success('已更新')
      } else {
        const response: any = await createNotificationChannel(payload)
        if (!isScopeCurrent(requestToken, requestAccountKey)) return
        setEditor((current: any) => ({ ...current, id: response.channel?.id ?? current.id }))
        notify.success('已创建')
      }
      setEditorOpen(false)
      await reload()
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '保存失败')
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) {
        setSaving(false)
        setBindSaving(false)
      }
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
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '删除失败')
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
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '测试失败')
    } finally {
      if (isScopeCurrent(requestToken, requestAccountKey)) setTesting(false)
    }
  }

  async function testFromCard(id: string) {
    const requestToken = getAuthToken()
    const requestAccountKey = activeAccountKeyRef.current
    try {
      await testNotificationChannel(id, '')
      if (!isScopeCurrent(requestToken, requestAccountKey)) return
      notify.success('测试已发送')
    } catch (error: any) {
      if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '测试失败')
    }
  }

  function isBound(uid: string, channelId: string) {
    return Boolean((bindingsByUid[uid] || []).find((binding: any) => binding.channel_id === channelId && binding.enabled))
  }

  function countBound(channelId: string) {
    let count = 0
    for (const list of Object.values(bindingsByUid)) {
      if (((list as any[]) || []).some((binding: any) => binding.channel_id === channelId && binding.enabled)) count++
    }
    return count
  }

  function setBindDraft(uid: string, enabled: boolean) {
    setBindDraftByUid((current: any) => ({ ...current, [uid]: enabled }))
  }

  function setAllBindDraft(enabled: boolean) {
    setBindDraftByUid((current: any) => {
      const next = { ...current }
      for (const account of activeAccountsRef.current) {
        next[account.uid] = enabled
      }
      return next
    })
  }

  const definition = getChannelDefinition(editor.type)
  const editorChannel = channels.find((channel) => channel.id === editor.id)
  const editorIsFeishu = Boolean(editor.id && (editor.type === 'feishu' || editor.type === 'lark'))
  const bindDraftSelectedCount = activeAccounts.filter((account) => Boolean(bindDraftByUid[account.uid])).length
  const bindDraftAllSelected = activeAccounts.length > 0 && bindDraftSelectedCount === activeAccounts.length
  const callbackUrls = editorChannel && typeof window !== 'undefined'
    ? buildCallbackUrls(window.location.origin, editorChannel)
    : []

  return (
    <div className="relative flex h-full flex-col mobile-page-shell overflow-hidden" style={{ backgroundColor: 'var(--bg-page)' }}>
      <div className="pointer-events-none absolute -top-12 right-12 h-40 w-40 rounded-full blur-[70px]" style={{ background: 'color-mix(in srgb, var(--primary) 18%, transparent)' }} />
      <div className="pointer-events-none absolute bottom-8 left-0 h-32 w-32 rounded-full blur-[60px]" style={{ background: 'color-mix(in srgb, var(--primary) 12%, transparent)' }} />

      <TopBar
        title="通知中心"
        right={(
          <div className="flex items-center gap-1">
            <SensitiveVisibilityToggle compact className="btn-ghost btn-sm tap-target" />
            <button onClick={reload} disabled={loading} className="btn-ghost btn-sm tap-target" aria-label="刷新">
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            </button>
          </div>
        )}
      />

      <div className="relative min-h-0 flex-1 overflow-y-auto mobile-page-body pt-3">
        <div className="mobile-page-stack pb-24">
          <section
            className="mobile-surface-card px-4 py-4"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <h2
                  className="text-[22px] font-semibold leading-none"
                  style={{ color: 'var(--text-main)' }}
                >
                  通知中心
                </h2>
              </div>

              <div
                className="shrink-0 rounded-[20px] border px-3 py-2 text-right"
                style={{ background: 'var(--surface-card)', borderColor: 'var(--glass-border)' }}
              >
                <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>渠道概况</div>
                <div className="mt-1 text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                  {enabledChannels}/{channels.length || 0} 已启用
                </div>
                <div className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                  共 {totalBindings} 条有效绑定
                </div>
              </div>
            </div>

            <div className="mt-4 grid grid-cols-3 gap-2">
              <MetricTile icon={BellRing} label="渠道" value={channels.length} />
              <MetricTile icon={ShieldCheck} label="启用" value={enabledChannels} tone="gold" />
              <MetricTile icon={Link2} label="绑定" value={totalBindings} />
            </div>
          </section>

          <section className="mobile-surface-card px-4 py-4">
            <button
              type="button"
              aria-label={`${platformMessagesEnabled ? '关闭' : '开启'}通知消息 / 互动消息`}
              onClick={() => togglePlatformMessages(!platformMessagesEnabled)}
              disabled={settingsSaving}
              className="flex w-full items-center justify-between gap-3 disabled:opacity-50"
            >
              <div className="min-w-0 text-left">
                <p className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>
                  通知消息 / 互动消息
                </p>
                <p className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  关闭后不再展示平台通知会话，也不会向外部渠道推送这类消息。
                </p>
              </div>
              <Toggle on={platformMessagesEnabled} />
            </button>
          </section>

          <section
            className="mobile-surface-card overflow-hidden"
          >
            <div className="border-b px-4 py-3" style={{ borderColor: 'var(--glass-border)' }}>
              <h3 className="mt-1 text-[18px] font-semibold" style={{ color: 'var(--text-main)' }}>
                渠道
              </h3>
            </div>

            {loading && channels.length === 0 ? (
              <div className="flex items-center justify-center gap-2 py-16" style={{ color: 'var(--text-secondary)' }}>
                <Loader2 className="h-4 w-4 animate-spin" /> 加载中…
              </div>
            ) : channels.length === 0 ? (
              <div className="px-3 py-4">
                <div className="mobile-soft-card">
                  <EmptyState icon={BellRing} title="暂无通知渠道" description="点击右下角创建" />
                </div>
              </div>
            ) : (
              <div className="space-y-2 px-3 py-3">
                {channels.map((channel) => {
                  const Icon = iconFor(channel.type)
                  const channelDefinition = getChannelDefinition(channel.type)
                  const isWechat = channel.type === 'openclaw_weixin'
                  const isFeishu = channel.type === 'feishu' || channel.type === 'lark'
                  const runtime = presentNotificationRuntime(channel)
                  return (
                    <div
                      key={channel.id}
                      className="overflow-hidden rounded-[24px] border"
                      style={{ background: 'var(--surface-card)', borderColor: 'var(--glass-border)' }}
                    >
                      <div
                        onClick={isWechat ? undefined : () => openEdit(channel)}
                        className={`w-full p-4 text-left ${isWechat ? '' : 'cursor-pointer'}`}
                      >
                        <div className="flex items-start gap-3">
                          <div
                            className="flex h-12 w-12 shrink-0 items-center justify-center rounded-[18px] text-white"
                            style={{ backgroundColor: channelDefinition.accent || 'var(--primary)' }}
                          >
                            <Icon className="h-5 w-5" />
                          </div>

                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <p className="truncate text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>
                                {channel.name || channelDefinition.label}
                              </p>
                              <span
                                className="shrink-0 rounded-full px-2 py-0.5 text-[10px] font-bold"
                                style={{
                                  background: channel.enabled ? 'color-mix(in srgb, var(--success) 12%, transparent)' : 'var(--mobile-glass-surface)',
                                  color: channel.enabled ? 'var(--success)' : 'var(--text-muted)',
                                }}
                              >
                                {channel.enabled ? (isWechat ? '在线' : '启用') : '停用'}
                              </span>
                            </div>

                            <div className="mt-1 flex items-center gap-1.5">
                              <div className="h-2 w-2 rounded-full" style={{ backgroundColor: runtime.dot }} />
                              <span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                                {runtime.label} · {isWechat ? '已绑定所有账号' : isFeishu ? '支持多成员协作' : `已绑 ${countBound(channel.id)} 账号`}
                              </span>
                            </div>

                            {!!runtime.facts.length && (
                              <div className="mt-3 flex flex-wrap gap-1.5" data-testid={`runtime-facts-${channel.id}`}>
                                {runtime.facts.slice(0, 3).map((fact) => (
                                  <span
                                    key={fact.key}
                                    className="inline-flex max-w-full items-center rounded-full px-2 py-1 text-[10px]"
                                    style={{
                                      background: fact.tone === 'danger'
                                        ? 'color-mix(in srgb, var(--danger) 10%, transparent)'
                                        : 'color-mix(in srgb, var(--primary) 8%, transparent)',
                                      color: fact.tone === 'danger' ? 'var(--danger)' : 'var(--text-secondary)',
                                    }}
                                    title={fact.detail || fact.value}
                                  >
                                    <span className="font-semibold">{fact.label}：</span>
                                    <span className="ml-1 truncate">{fact.value}</span>
                                  </span>
                                ))}
                              </div>
                            )}
                          </div>

                          {!isWechat && <ChevronRight className="h-4 w-4 shrink-0" style={{ color: 'var(--text-secondary)' }} />}
                        </div>
                      </div>

                      {isWechat ? (
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
                            } catch (error: any) {
                              if (isScopeCurrent(requestToken, requestAccountKey)) notify.error(error.message || '删除失败')
                            }
                          }}
                          className="flex w-full cursor-pointer items-center justify-center gap-1 border-t py-3 text-[12px] font-medium active:bg-[var(--status-danger-bg)]"
                          style={{ color: 'var(--danger)', borderColor: 'var(--glass-border)' }}
                        >
                          <Trash2 className="h-3.5 w-3.5" /> 删除
                        </button>
                      ) : (
                        <button
                          onClick={() => testFromCard(channel.id)}
                          className="flex w-full cursor-pointer items-center justify-center gap-1 border-t py-3 text-[12px] font-medium"
                          style={{ color: 'var(--text-secondary)', borderColor: 'var(--glass-border)' }}
                        >
                          <TestTube2 className="h-3.5 w-3.5" /> 发送测试通知
                        </button>
                      )}
                    </div>
                  )
                })}
              </div>
            )}
          </section>
        </div>
      </div>

      <button
        onClick={() => setCreatePickerOpen(true)}
        className="liquid-fab fixed right-4 z-[70] flex h-14 w-14 cursor-pointer items-center justify-center rounded-full transition-transform active:scale-95"
        style={{
          bottom: 'calc(env(safe-area-inset-bottom) + 96px)',
        }}
        aria-label="新建通知渠道"
      >
        <Plus className="h-6 w-6" />
      </button>

      <BottomSheet open={createPickerOpen} onClose={() => setCreatePickerOpen(false)} title="选择渠道类型">
        <div className="grid grid-cols-2 gap-2">
          {ADDABLE_CHANNEL_DEFINITIONS.map((channelDefinition) => {
            const Icon = iconFor(channelDefinition.type)
            return (
              <button
                key={channelDefinition.type}
                type="button"
                aria-label={channelDefinition.label}
                onClick={() => startCreate(channelDefinition.type)}
                className="flex cursor-pointer flex-col items-center gap-2 rounded-xl p-3 transition-colors"
                style={{ backgroundColor: 'var(--surface-card)' }}
              >
                <div className="flex h-12 w-12 items-center justify-center rounded-xl text-white" style={{ backgroundColor: channelDefinition.accent }}>
                  <Icon className="h-6 w-6" />
                </div>
                <span className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>{channelDefinition.label}</span>
                <span className="line-clamp-2 text-center text-[10px]" style={{ color: 'var(--text-muted)' }}>
                  {channelDefinition.description}
                </span>
              </button>
            )
          })}
        </div>
      </BottomSheet>

      <BottomSheet
        open={editorOpen}
        onClose={() => setEditorOpen(false)}
        title={editor.id ? '编辑通知渠道' : `新建 ${definition.label}`}
        footer={editorIsFeishu ? (
          <>
            <button className="btn-secondary px-3" onClick={remove} disabled={testing} title="删除" aria-label="删除">
              <Trash2 className="h-4 w-4" style={{ color: 'var(--danger)' }} />
            </button>
            <button className="btn-primary flex-1" onClick={runTest} disabled={testing}>
              {testing ? <Loader2 className="h-4 w-4 animate-spin" /> : <TestTube2 className="h-4 w-4" />} 测试
            </button>
            <button className="btn-secondary flex-1" onClick={openFeishuRebind} disabled={testing}>
              <RefreshCw className="h-4 w-4" /> 重新扫码绑定
            </button>
          </>
        ) : (
          <>
            {editor.id && (
              <>
                <button className="btn-secondary px-3" onClick={remove} disabled={saving || bindSaving} title="删除" aria-label="删除">
                  <Trash2 className="h-4 w-4" style={{ color: 'var(--danger)' }} />
                </button>
                <button className="btn-secondary flex-1" onClick={runTest} disabled={testing || saving || bindSaving}>
                  {testing ? <Loader2 className="h-4 w-4 animate-spin" /> : <TestTube2 className="h-4 w-4" />} 测试
                </button>
              </>
            )}
            <button className="btn-secondary flex-1" onClick={() => setEditorOpen(false)} disabled={saving || bindSaving || testing}>
              取消
            </button>
            <button className="btn-primary flex-1" onClick={save} disabled={saving || bindSaving}>
              {saving || bindSaving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />} 保存
            </button>
          </>
        )}
      >
        <div className="space-y-4">
          {editorIsFeishu ? (
            <div className="space-y-4" data-testid="mobile-feishu-compact-panel">
              <div className="rounded-2xl p-4" style={{ background: 'var(--surface-card)' }}>
                <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>{editor.name || '飞书机器人'}</div>
                <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  扫码接入已保存机器人凭证。飞书侧使用菜单或卡片按钮操作，无需在这里维护 App 配置。
                </div>
                {!!editorChannel && (() => {
                  const runtime = presentNotificationRuntime(editorChannel)
                  return (
                    <div className="mt-3 space-y-2" data-testid="mobile-runtime-panel">
                      {runtime.facts.length ? runtime.facts.map((fact) => (
                        <div key={fact.key} className="liquid-muted-card rounded-xl px-3 py-2">
                          <div className="text-[11px] font-medium" style={{ color: 'var(--text-secondary)' }}>{fact.label}</div>
                          <div className="mt-1 break-words text-[11px]" style={{ color: fact.tone === 'danger' ? 'var(--status-danger-text)' : 'var(--text-main)' }}>{fact.value}</div>
                        </div>
                      )) : (
                        <div className="liquid-muted-card rounded-xl px-3 py-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                          当前运行状态：{runtime.label}
                        </div>
                      )}
                    </div>
                  )
                })()}
              </div>

              <FeishuCollaboratorPanel
                variant="mobile"
                loading={feishuMembersLoading}
                members={feishuMembers}
                activeInvite={feishuActiveInvite}
                inviteBusy={feishuInviteCreating}
                removingSenderId={feishuRemovingSenderId}
                onGenerateInvite={createFeishuInvite}
                onRemoveMember={removeFeishuMember}
                onCopyText={(text: string, message?: string) => copyText(text, () => notify.success(message || '已复制'))}
              />
            </div>
          ) : (
            <>
              <div>
                <label className="label">名称</label>
                <input
                  className="input"
                  value={editor.name}
                  onChange={(event) => setEditor((current: any) => ({ ...current, name: event.target.value }))}
                  placeholder={`${definition.label} 通知`}
                />
              </div>

              <button
                onClick={() => setEditor((current: any) => ({ ...current, enabled: !current.enabled }))}
                className="flex w-full items-center justify-between rounded-xl p-3"
                style={{ backgroundColor: editor.enabled ? 'var(--primary-light)' : 'var(--surface-card)' }}
              >
                <div className="text-left">
                  <p className="text-[14px] font-medium" style={{ color: 'var(--text-main)' }}>启用此渠道</p>
                  <p className="mt-0.5 text-[12px]" style={{ color: 'var(--text-secondary)' }}>关闭后不会发送通知</p>
                </div>
                <Toggle on={editor.enabled} />
              </button>

              <p className="pt-2 text-[12px] font-semibold" style={{ color: 'var(--text-secondary)' }}>渠道配置</p>
              {definition.fields.map((field: any) => {
                const value = editor.configDraft?.[field.key] ?? ''
                if (field.input === 'boolean') {
                  return (
                    <button
                      key={field.key}
                      onClick={() => setField(field.key, !value)}
                      className="flex w-full items-center justify-between rounded-xl p-3"
                      style={{ backgroundColor: 'var(--surface-card)' }}
                    >
                      <span className="text-[14px]" style={{ color: 'var(--text-main)' }}>{field.label}</span>
                      <Toggle on={!!value} />
                    </button>
                  )
                }
                if (field.input === 'textarea') {
                  return (
                    <div key={field.key}>
                      <label className="label">{field.label}</label>
                      <textarea
                        className="input"
                        rows={3}
                        placeholder={field.placeholder}
                        value={value}
                        onChange={(event) => setField(field.key, event.target.value)}
                      />
                    </div>
                  )
                }
                if (field.input === 'select') {
                  return (
                    <div key={field.key}>
                      <label className="label">{field.label}</label>
                      <select className="input" value={value} onChange={(event) => setField(field.key, event.target.value)}>
                        {(field.options || []).map((option: any) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </div>
                  )
                }
                return (
                  <div key={field.key}>
                    <label className="label">{field.label}</label>
                    <input
                      type={field.input === 'password' ? 'password' : 'text'}
                      className="input"
                      placeholder={field.placeholder}
                      value={value}
                      onChange={(event) => setField(field.key, event.target.value)}
                      autoComplete="off"
                    />
                  </div>
                )
              })}
            </>
          )}

          {!editorIsFeishu && !!callbackUrls.length && (
            <div className="space-y-3 rounded-2xl p-3" style={{ background: 'var(--surface-card)' }}>
              <div>
                <div className="mb-2 text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                  Callback 地址（配置到渠道后台）
                </div>
                <div className="space-y-2">
                  {callbackUrls.map((item: any) => (
                    <div key={`${item.method}-${item.url}`} className="liquid-muted-card rounded-xl px-3 py-2">
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-[11px] font-medium" style={{ color: 'var(--text-secondary)' }}>{item.label}</span>
                        <button onClick={() => copyText(item.url, () => notify.success('已复制'))} className="cursor-pointer p-1" aria-label="复制">
                          <Copy className="h-3.5 w-3.5" style={{ color: 'var(--primary)' }} />
                        </button>
                      </div>
                      <div className="mt-1 break-all font-mono text-[11px]" style={{ color: 'var(--text-muted)' }}>{item.url}</div>
                    </div>
                  ))}
                </div>
              </div>

              {!!editorChannel && (() => {
                const runtime = presentNotificationRuntime(editorChannel)
                return (
                  <div className="space-y-2" data-testid="mobile-runtime-panel">
                    <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                      运行时诊断
                    </div>
                    <div className="space-y-2">
                      {runtime.facts.map((fact) => (
                        <div key={fact.key} className="liquid-muted-card rounded-xl px-3 py-2">
                          <div className="text-[11px] font-medium" style={{ color: 'var(--text-secondary)' }}>{fact.label}</div>
                          <div
                            className="mt-1 break-words text-[11px]"
                            style={{ color: fact.tone === 'danger' ? 'var(--status-danger-text)' : 'var(--text-main)' }}
                          >
                            {fact.value}
                          </div>
                          {!!fact.detail && (
                            <div className="mt-1 break-words text-[10px]" style={{ color: 'var(--text-muted)' }}>
                              {fact.detail}
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )
              })()}
            </div>
          )}

          {!editorIsFeishu && editor.id && (
            <>
              <div className="flex items-center justify-between gap-3 pt-2">
                <div>
                  <p className="text-[12px] font-semibold" style={{ color: 'var(--text-secondary)' }}>账号绑定</p>
                  <p className="mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                    已选择 {bindDraftSelectedCount} / {activeAccounts.length} 个账号
                  </p>
                </div>
                {!!activeAccounts.length && (
                  <button
                    type="button"
                    onClick={() => setAllBindDraft(!bindDraftAllSelected)}
                    disabled={saving || bindSaving}
                    className="btn-secondary btn-sm disabled:opacity-40"
                  >
                    {bindDraftAllSelected ? '全部取消' : '全选'}
                  </button>
                )}
              </div>
              {activeAccounts.length === 0 ? (
                <p className="py-3 text-[13px]" style={{ color: 'var(--text-muted)' }}>暂无活跃账号可绑定</p>
              ) : (
                <div className="space-y-2">
                  {activeAccounts.map((account) => {
                    const bound = Boolean(bindDraftByUid[account.uid])
                    return (
                      <button
                        key={account.uid}
                        onClick={() => setBindDraft(account.uid, !bound)}
                        disabled={saving || bindSaving}
                        className="flex w-full items-center justify-between rounded-xl p-3 disabled:opacity-50"
                        style={{ backgroundColor: bound ? 'var(--primary-light)' : 'var(--surface-card)' }}
                      >
                        <div className="min-w-0 flex-1 text-left">
                          <p className="truncate text-[14px] font-medium" style={{ color: 'var(--text-main)' }}>
                            <SensitiveValue kind="account" value={account.nick || account.uid} />
                          </p>
                          <p className="mt-0.5 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                            UID: <SensitiveValue kind="uid" value={account.uid} />
                          </p>
                        </div>
                        <Toggle on={bound} />
                      </button>
                    )
                  })}
                </div>
              )}
            </>
          )}
        </div>
      </BottomSheet>

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
    </div>
  )
}

function MetricTile({ icon: Icon, label, value, tone = 'blue' }) {
  const accent = 'var(--primary)'
  const accentSoft = tone === 'gold' ? 'var(--status-warning-bg)' : 'var(--primary-light)'

  return (
    <div className="mobile-soft-card px-3 py-3">
      <div className="flex items-center gap-2">
        <span
          className="inline-flex h-9 w-9 items-center justify-center rounded-[14px]"
          style={{ background: accentSoft, color: accent }}
        >
          <Icon className="h-4.5 w-4.5" />
        </span>
        <div className="min-w-0">
          <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{label}</div>
          <div className="text-[18px] font-semibold" style={{ color: 'var(--text-main)' }}>{value}</div>
        </div>
      </div>
    </div>
  )
}
