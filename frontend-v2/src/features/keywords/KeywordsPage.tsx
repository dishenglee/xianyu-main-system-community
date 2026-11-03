import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { CSSProperties } from 'react'
import { Image as ImageIcon, Plus, RefreshCw } from 'lucide-react'
import type { Account, Keyword } from '../../api/types'
import {
  createKeyword,
  deleteKeyword,
  getKeywords,
  updateAccountSetting,
  updateKeyword,
} from '../../api/index'
import { useAppDialog } from '../../components/AppDialog'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { Modal } from '../../components/Modal'
import { useSafeNotify } from '../../components/Notify'
import { StatusBadge } from '../../components/StatusBadge'
import { TopBar } from '../../components/TopBar'
import { useAccounts } from '../../hooks/useAccounts'
import { useAuthScope } from '../../hooks/useAuthScope'
import { useDevice } from '../../hooks/useDevice'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import { AccountConfigurationSplit } from '../automation/AccountConfigurationSplit'
import { loadStoredAccountSelection, saveStoredAccountSelection } from '../automation/accountSelectionStorage'
import { sanitizeSelectableUids } from '../automation/capabilities'
import type { CapabilityAccount } from '../automation/capabilities'
import {
  buildAccountKeywordPayloads,
  selectSavedKeywordTemplates,
} from './keywordTargets'

const GLOBAL_SCOPE = '__global__'
const ACCOUNT_SELECTION_STORAGE_KEY = 'keywords'

type KeywordForm = {
  keyword: string
  reply: string
  type: string
  itemId: string
  enabled: boolean
}

type NormalizedKeyword = Keyword & {
  id: number
  cookieId: string
  itemId: string
  reply: string
  type: string
  enabled: boolean
  taskGroupId: string
  createdAt: number
}

const emptyForm: KeywordForm = {
  keyword: '',
  reply: '',
  type: 'text',
  itemId: '',
  enabled: true,
}

function createTaskGroupId() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `keyword-task-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
}

function normalizeKeyword(rule: Keyword): NormalizedKeyword {
  return {
    ...rule,
    id: Number(rule.id ?? 0),
    cookieId: rule.cookieId || rule.cookie_id || '',
    itemId: rule.itemId || rule.item_id || '',
    keyword: rule.keyword || '',
    reply: rule.reply || '',
    type: rule.type || 'text',
    enabled: Boolean(rule.enabled),
    taskGroupId: String(rule.taskGroupId || rule.task_group_id || '').trim(),
    createdAt: Number(rule.createdAt ?? rule.created_at ?? 0) || 0,
  }
}

function accountReplyMode(account: Account) {
  return String(account.replyMode || '').trim()
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback
}

function previewKeywordReply(rule: NormalizedKeyword) {
  if (rule.type === 'image') {
    return (
      <div className="space-y-2">
        <div className="flex items-center gap-2 text-[12px] font-medium" style={{ color: 'var(--primary)' }}>
          <ImageIcon className="h-4 w-4" /> 图片回复
        </div>
        <img
          src={rule.reply}
          alt="关键词回复图片"
          className="max-h-[180px] rounded-2xl border object-contain bg-[var(--surface-soft)]"
          style={{ borderColor: 'var(--border-light)' }}
        />
      </div>
    )
  }

  return (
    <p className="whitespace-pre-wrap break-words text-[13px] leading-6 md:text-[14px]" style={{ color: 'var(--text-main)' }}>
      {rule.reply}
    </p>
  )
}

export default function KeywordsPage() {
  const { accounts, loadAccounts } = useAccounts()
  const { isMobile, isTablet } = useDevice()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const [loading, setLoading] = useState(false)
  const [savingTemplate, setSavingTemplate] = useState(false)
  const [globalKeywords, setGlobalKeywords] = useState<NormalizedKeyword[]>([])
  const [showModal, setShowModal] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)
  const [form, setForm] = useState<KeywordForm>({ ...emptyForm })
  const [error, setError] = useState('')
  const [configuredUids, setConfiguredUids] = useState<string[]>([])
  const [selectedPendingUids, setSelectedPendingUids] = useState<string[]>([])
  const [savingAccounts, setSavingAccounts] = useState(false)
  const [removingConfiguredUid, setRemovingConfiguredUid] = useState('')
  const [selectedTemplateIds, setSelectedTemplateIds] = useState<number[]>([])
  const loadSeqRef = useRef(0)
  const removedConfiguredUidsRef = useRef(new Set<string>())
  const locallyConfiguredUidsRef = useRef(new Set<string>())
  const compact = isMobile || isTablet

  const activeAccounts = useMemo(() => filterBusinessAccounts(accounts), [accounts])
  const capabilityAccounts = activeAccounts as unknown as CapabilityAccount[]
  const activeAccountKey = useMemo(
    () => activeAccounts.map((account) => `${account.uid}:${accountReplyMode(account)}`).join('|'),
    [activeAccounts],
  )
  const savedKeywordTemplates = useMemo(
    () => globalKeywords.filter((rule) => rule.enabled && rule.keyword.trim() && rule.reply.trim()),
    [globalKeywords],
  )
  const selectedTemplates = useMemo(
    () => selectSavedKeywordTemplates(savedKeywordTemplates, selectedTemplateIds),
    [savedKeywordTemplates, selectedTemplateIds],
  )

  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    setGlobalKeywords([])
    setConfiguredUids([])
    setSelectedPendingUids([])
    setSavingAccounts(false)
    setRemovingConfiguredUid('')
    removedConfiguredUidsRef.current.clear()
    locallyConfiguredUidsRef.current.clear()
    setSelectedTemplateIds([])
    setShowModal(false)
    setEditingId(null)
    setForm({ ...emptyForm })
    setError('')
    setLoading(false)
    setSavingTemplate(false)
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  useEffect(() => {
    void loadAccounts()
  }, [authToken, loadAccounts])

  useEffect(() => {
    const accountByUid = new Map(activeAccounts.map((account) => [account.uid, account]))
    const storedUids = loadStoredAccountSelection(ACCOUNT_SELECTION_STORAGE_KEY).filter((uid) => {
      const account = accountByUid.get(uid)
      if (!account) return false
      const mode = accountReplyMode(account)
      return mode === 'keyword'
    })
    const replyModeUids = activeAccounts
      .filter((account) => accountReplyMode(account) === 'keyword')
      .map((account) => account.uid)
    const nextConfiguredUids = sanitizeSelectableUids(
      [...new Set([...storedUids, ...replyModeUids, ...locallyConfiguredUidsRef.current])]
        .filter((uid) => !removedConfiguredUidsRef.current.has(uid)),
      capabilityAccounts,
      'autoReply',
      { allowEnableDisabledAccounts: true },
    )

    setConfiguredUids(nextConfiguredUids)
    setSelectedPendingUids((current) => {
      const configuredSet = new Set(nextConfiguredUids)
      return sanitizeSelectableUids(current, capabilityAccounts, 'autoReply', { allowEnableDisabledAccounts: true })
        .filter((uid) => !configuredSet.has(uid))
    })
  }, [activeAccountKey, activeAccounts, capabilityAccounts])

  const loadTemplates = useCallback(async () => {
    const scope = captureScope(GLOBAL_SCOPE)
    const loadSeq = ++loadSeqRef.current
    setLoading(true)
    try {
      const list = await getKeywords(GLOBAL_SCOPE)
      if (!isScopeCurrent(scope, GLOBAL_SCOPE) || loadSeqRef.current !== loadSeq) return
      setGlobalKeywords(
        (list || [])
          .map(normalizeKeyword)
          .filter((rule) => rule.cookieId === GLOBAL_SCOPE),
      )
    } catch (loadError) {
      if (isScopeCurrent(scope, GLOBAL_SCOPE) && loadSeqRef.current === loadSeq) {
        notify(errorMessage(loadError, '加载关键词失败'), 'error')
      }
    } finally {
      if (isScopeCurrent(scope, GLOBAL_SCOPE) && loadSeqRef.current === loadSeq) setLoading(false)
    }
  }, [captureScope, isScopeCurrent, notify])

  useEffect(() => {
    const timer = window.setTimeout(() => void loadTemplates(), 0)
    return () => window.clearTimeout(timer)
  }, [authToken, loadTemplates])

  function handlePendingSelection(nextUids: string[]) {
    const configuredSet = new Set(configuredUids)
    setSelectedPendingUids(
      sanitizeSelectableUids(nextUids, capabilityAccounts, 'autoReply', { allowEnableDisabledAccounts: true })
        .filter((uid) => !configuredSet.has(uid)),
    )
  }

  function toggleTemplate(ruleId: number) {
    if (!Number.isFinite(ruleId)) return
    setSelectedTemplateIds((ids) => (
      ids.includes(ruleId) ? ids.filter((id) => id !== ruleId) : [...ids, ruleId]
    ))
  }

  async function replaceAccountKeywords(uid: string, payloads: Partial<Keyword>[]) {
    const existing = await getKeywords(uid)
    const accountRules = (existing || [])
      .map(normalizeKeyword)
      .filter((rule) => rule.cookieId === uid && Number.isFinite(rule.id) && rule.id > 0)

    await Promise.all(accountRules.map((rule) => deleteKeyword(rule.id)))
    await Promise.all(payloads.map((payload) => createKeyword(uid, payload)))
  }

  async function saveAccountConfiguration() {
    const configuredSet = new Set(configuredUids)
    const newUids = sanitizeSelectableUids(
      selectedPendingUids.filter((uid) => !configuredSet.has(uid)),
      capabilityAccounts,
      'autoReply',
      { allowEnableDisabledAccounts: true },
    )
    if (!newUids.length) {
      notify('请选择待配置账号', 'error')
      return
    }

    setSavingAccounts(true)
    const scope = captureScope('save-accounts')
    const payloads = buildAccountKeywordPayloads(selectedTemplates, {
      taskGroupId: selectedTemplates.length ? createTaskGroupId() : '',
    })

    try {
      const results = await Promise.allSettled(newUids.map(async (uid) => {
        if (payloads.length) {
          await replaceAccountKeywords(uid, payloads)
          await updateAccountSetting(uid, { replyMode: 'keyword' })
          return
        }
        await updateAccountSetting(uid, { replyMode: 'keyword' })
      }))
      if (!isScopeCurrent(scope, 'save-accounts')) return

      const succeededUids = newUids.filter((_, index) => results[index].status === 'fulfilled')
      const failedUids = newUids.filter((_, index) => results[index].status === 'rejected')
      succeededUids.forEach((uid) => {
        locallyConfiguredUidsRef.current.add(uid)
        removedConfiguredUidsRef.current.delete(uid)
      })
      const nextConfigured = [...new Set([...configuredUids, ...succeededUids])]
      saveStoredAccountSelection(ACCOUNT_SELECTION_STORAGE_KEY, nextConfigured)
      setConfiguredUids(nextConfigured)
      setSelectedPendingUids(failedUids)
      void loadAccounts({ force: true })
      if (!failedUids.length) {
        setSelectedTemplateIds([])
        notify(
          payloads.length
            ? `已应用 ${payloads.length} 条关键词到 ${succeededUids.length} 个账号`
            : `已配置 ${succeededUids.length} 个关键词回复账号`,
          'success',
        )
      } else if (succeededUids.length) {
        notify(`已配置 ${succeededUids.length} 个账号，${failedUids.length} 个失败并保留待重试`, 'error')
      } else {
        const firstFailure = results.find((result) => result.status === 'rejected')
        notify(errorMessage(firstFailure?.reason, '保存关键词账号配置失败'), 'error')
      }
    } catch (saveError) {
      if (isScopeCurrent(scope, 'save-accounts')) {
        notify(errorMessage(saveError, '保存关键词账号配置失败'), 'error')
      }
    } finally {
      if (isScopeCurrent(scope, 'save-accounts')) setSavingAccounts(false)
    }
  }

  async function removeConfiguredAccount(uid: string) {
    if (removingConfiguredUid) return
    const account = activeAccounts.find((entry) => entry.uid === uid)
    const confirmed = await showConfirm(`确认移除“${account?.nick || uid}”的关键词回复配置？`, {
      title: '移除关键词回复账号',
      confirmLabel: '移除并删除',
      danger: true,
    })
    if (!confirmed) return
    setRemovingConfiguredUid(uid)
    const scope = captureScope(`remove-${uid}`)
    try {
      const existing = await getKeywords(uid)
      const ruleIds = (existing || [])
        .map(normalizeKeyword)
        .filter((rule) => rule.cookieId === uid && Number.isFinite(rule.id) && rule.id > 0)
        .map((rule) => rule.id)
      let pendingRuleIds = ruleIds
      for (let attempt = 0; attempt < 2 && pendingRuleIds.length; attempt += 1) {
        const deletionResults = await Promise.allSettled(pendingRuleIds.map((id) => deleteKeyword(id)))
        pendingRuleIds = pendingRuleIds.filter((_, index) => deletionResults[index].status === 'rejected')
      }
      if (pendingRuleIds.length) {
        throw new Error(`仍有 ${pendingRuleIds.length} 条关键词规则删除失败`)
      }
      await updateAccountSetting(uid, { replyMode: 'off' })
      if (!isScopeCurrent(scope, `remove-${uid}`)) return

      removedConfiguredUidsRef.current.add(uid)
      locallyConfiguredUidsRef.current.delete(uid)
      const nextConfigured = configuredUids.filter((configuredUid) => configuredUid !== uid)
      saveStoredAccountSelection(ACCOUNT_SELECTION_STORAGE_KEY, nextConfigured)
      setConfiguredUids(nextConfigured)
      setSelectedPendingUids((current) => current.filter((selectedUid) => selectedUid !== uid))
      void loadAccounts({ force: true })
      notify('关键词回复账号已移除', 'success')
    } catch (removeError) {
      if (isScopeCurrent(scope, `remove-${uid}`)) {
        void loadAccounts({ force: true })
        notify(errorMessage(removeError, '移除关键词回复账号失败，已重新同步账号状态'), 'error')
      }
    } finally {
      if (isScopeCurrent(scope, `remove-${uid}`)) setRemovingConfiguredUid('')
    }
  }

  function openCreate() {
    setForm({ ...emptyForm })
    setEditingId(null)
    setError('')
    setShowModal(true)
  }

  function openEditTemplate(rule: NormalizedKeyword) {
    setForm({
      keyword: rule.keyword,
      reply: rule.reply,
      type: rule.type,
      itemId: rule.itemId,
      enabled: rule.enabled,
    })
    setEditingId(rule.id)
    setError('')
    setShowModal(true)
  }

  function closeModal() {
    setShowModal(false)
    setEditingId(null)
    setForm({ ...emptyForm })
    setError('')
  }

  async function submitTemplate() {
    setError('')
    if (!form.keyword.trim()) {
      setError('关键词不能为空')
      return
    }
    if (!form.reply.trim()) {
      setError(form.type === 'image' ? '图片地址不能为空' : '回复内容不能为空')
      return
    }

    setSavingTemplate(true)
    const scope = captureScope('save-template')
    const payload: Partial<Keyword> = {
      keyword: form.keyword.trim(),
      reply: form.reply.trim(),
      type: form.type,
      itemId: form.itemId.trim(),
      enabled: form.enabled,
    }

    try {
      if (editingId === null) {
        await createKeyword(GLOBAL_SCOPE, payload)
      } else {
        await updateKeyword(editingId, payload)
      }
      if (!isScopeCurrent(scope, 'save-template')) return
      const message = editingId === null ? '关键词模板已新增' : '关键词模板已更新'
      closeModal()
      await loadTemplates()
      notify(message, 'success')
    } catch (saveError) {
      if (isScopeCurrent(scope, 'save-template')) {
        setError(errorMessage(saveError, '保存关键词失败'))
      }
    } finally {
      if (isScopeCurrent(scope, 'save-template')) setSavingTemplate(false)
    }
  }

  async function removeTemplate(rule: NormalizedKeyword) {
    const confirmed = await showConfirm(`确认删除关键词“${rule.keyword}”模板？`, {
      title: '删除关键词模板',
      confirmLabel: '删除',
      danger: true,
    })
    if (!confirmed) return
    const scope = captureScope('remove-template')
    try {
      await deleteKeyword(rule.id)
      if (!isScopeCurrent(scope, 'remove-template')) return
      await loadTemplates()
      notify('关键词模板已删除', 'success')
    } catch (removeError) {
      if (isScopeCurrent(scope, 'remove-template')) {
        notify(errorMessage(removeError, '删除关键词模板失败'), 'error')
      }
    }
  }

  function renderTemplateOption(rule: NormalizedKeyword) {
    const checked = selectedTemplateIds.includes(rule.id)
    return (
      <div
        key={rule.id}
        className={`${compact ? 'mobile-soft-card p-3.5' : 'input'} ${checked ? 'ring-2' : ''}`}
        style={checked ? { '--tw-ring-color': 'var(--primary-light)' } as CSSProperties : undefined}
      >
        <div className="flex items-start gap-3">
          <input
            type="checkbox"
            aria-label={`选择关键词模板 ${rule.keyword}`}
            className="mt-1 h-4 w-4 shrink-0 rounded"
            style={{ accentColor: 'var(--primary)' }}
            checked={checked}
            onChange={() => toggleTemplate(rule.id)}
          />
          <div className="min-w-0 flex-1">
            <div className="mb-2 flex flex-wrap items-center gap-2">
              <span className="rounded-full px-2 py-0.5 text-[11px] font-semibold" style={{ background: 'var(--primary-light)', color: 'var(--primary)' }}>
                {rule.keyword}
              </span>
              <span className="rounded-full px-2 py-0.5 text-[11px] font-medium" style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>
                {rule.itemId ? `商品 ${rule.itemId}` : '全部商品'}
              </span>
              <span className="rounded-full px-2 py-0.5 text-[11px] font-medium" style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}>
                {rule.type === 'image' ? '图片' : '文字'}
              </span>
              <StatusBadge status={rule.enabled ? 'info' : 'neutral'} label={rule.enabled ? '启用中' : '已关闭'} />
            </div>
            {previewKeywordReply(rule)}
          </div>
        </div>
        <div
          className={compact ? 'mt-3 mobile-soft-card flex items-center gap-2 p-2.5' : 'mt-3 flex items-center gap-2 border-t pt-3'}
          style={compact ? undefined : { borderColor: 'var(--border-light)' }}
        >
          <button type="button" onClick={() => openEditTemplate(rule)} className="btn-ghost btn-sm flex-1 md:flex-initial">编辑</button>
          <button type="button" onClick={() => removeTemplate(rule)} className="btn-sm flex-1 md:flex-initial" style={{ color: 'var(--danger)' }}>删除</button>
        </div>
      </div>
    )
  }

  return (
    <div className={compact ? 'h-full flex flex-col mobile-page-shell' : 'contents'} style={compact ? { backgroundColor: 'var(--bg-page)' } : undefined}>
      {compact ? <TopBar title="关键词规则" /> : null}
      {!compact ? <HubTabs tabs={HUB_PRESETS.automation} /> : null}

      <div className={compact ? 'flex-1 overflow-y-auto mobile-page-body' : 'flex-1 overflow-y-auto px-4 py-4 md:px-6'}>
        <div className={compact ? 'mobile-page-stack' : 'space-y-6'}>
          <section className={compact ? 'mobile-surface-card p-4 space-y-4' : 'card p-4 md:p-5 mb-4 space-y-4'}>
            <div className={compact ? 'mobile-section-head items-start' : 'flex items-start justify-between gap-3'}>
              <h2 className={compact ? 'mobile-section-title' : 'text-[15px] font-semibold'} style={{ color: compact ? undefined : 'var(--text-main)' }}>
                关键词配置
              </h2>
              {compact ? <span className="mobile-pill mobile-muted-pill shrink-0">{globalKeywords.length} 条模板</span> : null}
            </div>

            <div className={compact ? 'mobile-soft-card p-3.5 space-y-3' : 'surface-card rounded-2xl p-4 space-y-3'}>
              <div className={compact ? 'mobile-section-head items-start' : 'flex flex-col gap-2 md:flex-row md:items-start md:justify-between'}>
                <div>
                  <div className={compact ? 'mobile-section-title text-[1.05rem]' : 'text-[14px] font-semibold'} style={{ color: compact ? undefined : 'var(--text-primary)' }}>
                    已保存关键词模板
                  </div>
                  {!compact ? <p className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>模板可单独维护；配置账号时可选择模板覆盖写入本次新增账号。</p> : null}
                </div>
                <div className="flex flex-wrap items-center gap-2 md:justify-end">
                  <button onClick={openCreate} className="btn-primary btn-sm flex items-center gap-1"><Plus className="h-4 w-4" /> 新增关键词模板</button>
                  <span className={compact ? 'mobile-pill mobile-muted-pill shrink-0' : 'text-[12px]'} style={{ color: selectedTemplates.length ? 'var(--primary)' : 'var(--text-secondary)' }}>
                    已选择 {selectedTemplates.length} 条
                  </span>
                </div>
              </div>
              {loading ? (
                <div className="flex items-center justify-center gap-2 py-8 text-[13px]" style={{ color: 'var(--text-secondary)' }}><RefreshCw className="h-4 w-4 animate-spin" /> 加载中…</div>
              ) : globalKeywords.length ? (
                <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-5">{globalKeywords.map(renderTemplateOption)}</div>
              ) : (
                <div className="rounded-xl border border-dashed p-4 text-[13px]" style={{ borderColor: 'var(--border-light)', color: 'var(--text-secondary)' }}>
                  还没有关键词模板。无模板时也可以先启用账号的关键词回复模式。
                </div>
              )}
            </div>

            <div className={compact ? 'mobile-soft-card p-3.5 space-y-3' : 'surface-card rounded-2xl p-4 space-y-3'}>
              <div>
                <div className={compact ? 'mobile-section-title text-[1.05rem]' : 'text-[14px] font-semibold'} style={{ color: compact ? undefined : 'var(--text-primary)' }}>账号配置</div>
                {!compact ? <p className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>保存只处理左侧本次勾选账号；移除会清理该账号全部关键词规则并关闭关键词回复。</p> : null}
              </div>
              <AccountConfigurationSplit
                accounts={activeAccounts}
                configuredUids={configuredUids}
                selectedPendingUids={selectedPendingUids}
                onSelectionChange={handlePendingSelection}
                onSave={saveAccountConfiguration}
                onRemove={removeConfiguredAccount}
                saving={savingAccounts}
                removingUid={removingConfiguredUid}
                pendingDescription="选择本次要新增关键词回复配置的活跃账号。"
                configuredDescription="来源于账号真实关键词模式和本地保存范围。"
                saveLabel={selectedTemplates.length ? '应用模板并配置' : '保存配置'}
              />
            </div>
          </section>
        </div>
      </div>

      <Modal open={showModal} onClose={closeModal} title={editingId === null ? '新增关键词模板' : '编辑关键词模板'}>
        <div className="space-y-4 py-2">
          <label className="block">
            <span className="label">关键词</span>
            <input className="input" value={form.keyword} onChange={(event) => setForm((current) => ({ ...current, keyword: event.target.value }))} placeholder="例如：发货、能便宜点吗" />
          </label>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <label className="block">
              <span className="label">回复类型</span>
              <select className="input" value={form.type} onChange={(event) => setForm((current) => ({ ...current, type: event.target.value }))}>
                <option value="text">文本</option>
                <option value="image">图片 URL</option>
              </select>
            </label>
            <label className="block">
              <span className="label">商品 ID</span>
              <input className="input" value={form.itemId} onChange={(event) => setForm((current) => ({ ...current, itemId: event.target.value }))} placeholder="留空表示全部商品" />
            </label>
          </div>
          <label className="block">
            <span className="label">{form.type === 'image' ? '图片地址' : '回复内容'}</span>
            <textarea className="input min-h-[120px] resize-y" value={form.reply} onChange={(event) => setForm((current) => ({ ...current, reply: event.target.value }))} placeholder={form.type === 'image' ? 'https://...' : '输入自动回复文案'} />
          </label>
          {form.type === 'image' && form.reply.trim() ? (
            <div className="rounded-2xl border bg-[var(--surface-soft)] p-3" style={{ borderColor: 'var(--border-light)' }}>
              <div className="mb-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>图片预览</div>
              <img src={form.reply} alt="预览" className="max-h-[200px] rounded-xl object-contain" />
            </div>
          ) : null}
          <label className={`flex cursor-pointer items-center gap-3 rounded-2xl border px-4 py-3 text-[13px] ${compact ? 'mobile-soft-card border-0' : ''}`} style={{ borderColor: 'var(--border-light)' }}>
            <input type="checkbox" checked={form.enabled} onChange={(event) => setForm((current) => ({ ...current, enabled: event.target.checked }))} className="h-4 w-4 rounded" style={{ accentColor: 'var(--primary)' }} />
            启用后可被选择并应用到账号
          </label>
          {editingId === null ? (
            <div className="rounded-2xl border px-4 py-3 text-[12px]" style={{ borderColor: 'var(--control-border)', backgroundColor: 'var(--primary-light)', color: 'var(--text-secondary)' }}>
              保存后进入关键词模板池；只有配置账号时选择模板，才会写入账号规则。
            </div>
          ) : null}
          {error ? <p className="liquid-notice liquid-notice-danger rounded-2xl px-4 py-3 text-[13px]">{error}</p> : null}
          <div className="flex flex-col-reverse items-stretch gap-2 pt-2 sm:flex-row sm:items-center sm:justify-end sm:gap-3">
            <button onClick={closeModal} className="btn-secondary btn-sm w-full sm:w-auto">取消</button>
            <button onClick={submitTemplate} disabled={savingTemplate} className="btn-primary btn-sm w-full disabled:opacity-40 sm:w-auto">{savingTemplate ? '保存中…' : '保存'}</button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
