import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Bot, PlugZap, RefreshCw, Wand2 } from 'lucide-react'
import { discoverAiModels, getAiSettings, testAiSettingsConnection, updateAiSettings } from '../../api'
import { updateAccountSetting } from '../../api/accounts'
import type { AiSettings } from '../../api/types'
import { useAccounts } from '../../hooks/useAccounts'
import { useDevice } from '../../hooks/useDevice'
import { TopBar } from '../../components/TopBar'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { useNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import { emitAccountsSynced } from '../runtimeEvents'
import { AccountConfigurationSplit } from '../automation/AccountConfigurationSplit'
import { enabledCapabilityUids, sanitizeSelectableUids } from '../automation/capabilities'
import type { CapabilityAccount } from '../automation/capabilities'
import { useAuthScope } from '../../hooks/useAuthScope'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import {
  emitActionFailure,
  emitActionLog,
  emitActionStarted,
  emitActionSuccess,
} from '../../bootstrap/actionLogging'

const GLOBAL_SCOPE = '__global__'

const DEFAULT_PROMPTS = {
  system: '你是闲鱼卖家的自动客服。回复要简洁、自然、礼貌，优先直接解决问题。不要编造商品不存在的信息，不要承诺无法确认的售后或赠品，不要输出 Markdown，不要暴露你是程序。',
  price: '你是闲鱼卖家的自动客服，当前是询价或议价场景。先结合商品信息、当前议价次数、最大优惠比例和最大优惠金额来判断能否优惠。能优惠时给出明确但克制的口径；不能继续优惠时要礼貌说明；不要一次性把底价说死，不要承诺包邮、赠品或额外服务，除非商品信息里已经明确。',
  tech: '你是闲鱼卖家的自动客服，当前是商品功能、使用方式、成色或兼容性咨询场景。只基于现有商品信息和对话内容回答，不确定就明确说需要买家下单前再确认，不要编造参数或功能。',
  default: '你是闲鱼卖家的自动客服，当前是普通咨询场景。请直接回答买家的问题，语气自然、简洁、礼貌，优先推动成交，但不要过度营销，不要编造未提供的信息。',
  forbiddenWords: '',
}

type PromptKey = keyof typeof DEFAULT_PROMPTS

type AiFormState = {
  apiType: string
  apiKey: string
  baseUrl: string
  modelName: string
  maxBargainRounds: string | number
  maxDiscountPercent: string | number
  maxDiscountAmount: string | number
  systemPrompt: string
  pricePrompt: string
  techPrompt: string
  defaultPrompt: string
  forbiddenWords: string
}

type FetchModelsOptions = {
  silent?: boolean
  force?: boolean
}

function readPromptValue(settings: AiSettings | null | undefined, key: PromptKey) {
  const snakeValue = settings?.custom_prompts?.[key]
  if (typeof snakeValue === 'string') return snakeValue
  const camelValue = settings?.customPrompts?.[key]
  if (typeof camelValue === 'string') return camelValue
  return DEFAULT_PROMPTS[key]
}

const emptyState: AiFormState = {
  apiType: 'openai',
  apiKey: '',
  baseUrl: '',
  modelName: '',
  maxBargainRounds: 3,
  maxDiscountPercent: 10,
  maxDiscountAmount: 100,
  systemPrompt: DEFAULT_PROMPTS.system,
  pricePrompt: DEFAULT_PROMPTS.price,
  techPrompt: DEFAULT_PROMPTS.tech,
  defaultPrompt: DEFAULT_PROMPTS.default,
  forbiddenWords: DEFAULT_PROMPTS.forbiddenWords,
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback
}

export default function AiRepliesPage() {
  const { accounts, loadAccounts } = useAccounts()
  const { isMobile, isTablet } = useDevice()
  const notify = useNotify()
  const { showConfirm } = useAppDialog()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [form, setForm] = useState<AiFormState>({ ...emptyState })
  const [error, setError] = useState('')
  const [modelsLoading, setModelsLoading] = useState(false)
  const [, setModelsError] = useState('')
  const [modelOptions, setModelOptions] = useState<string[]>([])
  const [connectionTesting, setConnectionTesting] = useState(false)
  const [configuredUids, setConfiguredUids] = useState<string[]>([])
  const [selectedPendingUids, setSelectedPendingUids] = useState<string[]>([])
  const [selectionSaving, setSelectionSaving] = useState(false)
  const [removingConfiguredUid, setRemovingConfiguredUid] = useState('')
  const loadSeqRef = useRef(0)
  const modelFetchSeqRef = useRef(0)
  const modelFetchSignatureRef = useRef('')
  const locallyConfiguredUidsRef = useRef(new Set<string>())
  const removedConfiguredUidsRef = useRef(new Set<string>())
  const compact = isMobile || isTablet

  const activeAccounts = useMemo(() => filterBusinessAccounts(accounts), [accounts])
  const capabilityAccounts = activeAccounts as unknown as CapabilityAccount[]
  const realConfiguredUids = useMemo(
    () => enabledCapabilityUids(capabilityAccounts, 'aiEnabled'),
    [capabilityAccounts],
  )
  const activeAccountStateKey = useMemo(
    () => activeAccounts.map((account) => `${account.uid}:${realConfiguredUids.includes(account.uid) ? 'ai' : 'off'}`).join('|'),
    [activeAccounts, realConfiguredUids],
  )
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    modelFetchSeqRef.current += 1
    modelFetchSignatureRef.current = ''
    setForm({ ...emptyState })
    setError('')
    setModelsLoading(false)
    setModelsError('')
    setModelOptions([])
    setConnectionTesting(false)
    setConfiguredUids([])
    setSelectedPendingUids([])
    setLoading(false)
    setSaving(false)
    setSelectionSaving(false)
    setRemovingConfiguredUid('')
    locallyConfiguredUidsRef.current.clear()
    removedConfiguredUidsRef.current.clear()
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  const load = useCallback(async () => {
    const scope = captureScope()
    const loadSeq = ++loadSeqRef.current
    const isCurrent = () => isScopeCurrent(scope) && loadSeqRef.current === loadSeq
    setLoading(true)
    setError('')
    try {
      const settings = await getAiSettings(GLOBAL_SCOPE)
      if (!isCurrent()) return
      setForm({
        apiType: settings?.api_type || settings?.apiType || 'openai',
        apiKey: settings?.api_key || settings?.apiKey || '',
        baseUrl: settings?.base_url || settings?.baseUrl || '',
        modelName: settings?.model_name || settings?.modelName || '',
        maxBargainRounds: Number(settings?.max_bargain_rounds ?? settings?.maxBargainRounds ?? 3),
        maxDiscountPercent: Number(settings?.max_discount_percent ?? settings?.maxDiscountPercent ?? 10),
        maxDiscountAmount: Number(settings?.max_discount_amount ?? settings?.maxDiscountAmount ?? 100),
        systemPrompt: readPromptValue(settings, 'system'),
        pricePrompt: readPromptValue(settings, 'price'),
        techPrompt: readPromptValue(settings, 'tech'),
        defaultPrompt: readPromptValue(settings, 'default'),
        forbiddenWords: readPromptValue(settings, 'forbiddenWords'),
      })
    } catch (e) {
      if (isCurrent()) setError(errorMessage(e, '加载 AI 配置失败'))
    } finally {
      if (isCurrent()) setLoading(false)
    }
  }, [captureScope, isScopeCurrent])

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void loadAccounts()
      void load()
    }, 0)
    return () => window.clearTimeout(timer)
  }, [authToken, load, loadAccounts])

  useEffect(() => {
    const nextConfiguredUids = sanitizeSelectableUids(
      [...new Set([...realConfiguredUids, ...locallyConfiguredUidsRef.current])]
        .filter((uid) => !removedConfiguredUidsRef.current.has(uid)),
      capabilityAccounts,
      'aiEnabled',
      { allowEnableDisabledAccounts: true },
    )
    setConfiguredUids(nextConfiguredUids)
    setSelectedPendingUids((uids) => {
      const configuredSet = new Set(nextConfiguredUids)
      return sanitizeSelectableUids(uids, capabilityAccounts, 'aiEnabled', { allowEnableDisabledAccounts: true })
        .filter((uid) => !configuredSet.has(uid))
    })
  }, [activeAccountStateKey, activeAccounts, capabilityAccounts, realConfiguredUids])

  const setField = useCallback(<Key extends keyof AiFormState,>(key: Key, value: AiFormState[Key]) => {
    if (key === 'apiType' || key === 'apiKey' || key === 'baseUrl') {
      modelFetchSignatureRef.current = ''
      setModelsError('')
      setModelOptions([])
    }
    setForm((prev) => ({ ...prev, [key]: value }))
  }, [])

  function applyDefaultPromptTemplates() {
    setForm((prev) => ({
      ...prev,
      systemPrompt: DEFAULT_PROMPTS.system,
      pricePrompt: DEFAULT_PROMPTS.price,
      techPrompt: DEFAULT_PROMPTS.tech,
      defaultPrompt: DEFAULT_PROMPTS.default,
      forbiddenWords: DEFAULT_PROMPTS.forbiddenWords,
    }))
  }

  const fetchModels = useCallback(async ({ silent = false, force = false }: FetchModelsOptions = {}) => {
    const apiKey = form.apiKey.trim()
    const baseUrl = form.baseUrl.trim()
    if (!apiKey || !baseUrl) {
      if (!silent) {
        emitActionLog({
          module: 'ai-replies',
          action: 'discover_models_validation',
          outcome: 'warn',
          message: '获取 AI 模型校验失败：缺少 Base URL 或 API Key',
          context: { hasApiKey: Boolean(apiKey), hasBaseUrl: Boolean(baseUrl) },
          promoteToIncident: false,
        })
        setModelsError('请先填写 Base URL 和 API Key')
        notify.error('请先填写 Base URL 和 API Key')
      }
      setModelOptions([])
      modelFetchSignatureRef.current = ''
      return null
    }

    const signature = `${form.apiType}|${baseUrl}|${apiKey}`
    if (!force && signature === modelFetchSignatureRef.current) return null

    const fetchSeq = ++modelFetchSeqRef.current
    modelFetchSignatureRef.current = signature
    setModelsLoading(true)
    if (!silent) setModelsError('')
    if (!silent) {
      emitActionStarted({
        module: 'ai-replies',
        action: 'discover_models',
        message: '开始获取 AI 模型列表',
        context: { apiType: form.apiType, hasApiKey: Boolean(apiKey), hasBaseUrl: Boolean(baseUrl) },
      })
    }
    try {
      const result = await discoverAiModels({
        apiType: form.apiType,
        apiKey,
        baseUrl,
      })
      if (modelFetchSeqRef.current !== fetchSeq) return null
      const nextModels = Array.from(
        new Set((result?.models || []).map((model) => String(model || '').trim()).filter(Boolean)),
      )
      setModelOptions(nextModels)
      setForm((current) => {
        const currentModel = current.modelName.trim()
        const nextModel = nextModels.length > 0 && !nextModels.includes(currentModel)
          ? nextModels[0]
          : nextModels.length === 0
            ? ''
            : current.modelName
        return nextModel === current.modelName ? current : { ...current, modelName: nextModel }
      })
      if (!silent) {
        emitActionSuccess({
          module: 'ai-replies',
          action: 'discover_models',
          message: 'AI 模型列表获取成功',
          context: { apiType: form.apiType, modelCount: nextModels.length },
        })
        if (nextModels.length > 0) {
          notify.success(`已获取 ${nextModels.length} 个模型`)
        } else {
          notify.info('未获取到可用模型')
        }
      }
      setModelsError('')
      return { ...result, models: nextModels }
    } catch (e) {
      if (modelFetchSeqRef.current === fetchSeq) {
        setModelOptions([])
        if (!silent) {
          const message = errorMessage(e, '获取模型列表失败')
          setModelsError(message)
          emitActionFailure({
            module: 'ai-replies',
            action: 'discover_models',
            message: 'AI 模型列表获取失败',
            error: e,
            context: { apiType: form.apiType, hasApiKey: Boolean(apiKey), hasBaseUrl: Boolean(baseUrl) },
          })
          notify.error(message)
        }
      }
      return null
    } finally {
      if (modelFetchSeqRef.current === fetchSeq) setModelsLoading(false)
    }
  }, [form.apiKey, form.apiType, form.baseUrl, notify])

  useEffect(() => {
    const apiKey = form.apiKey.trim()
    const baseUrl = form.baseUrl.trim()
    if (!apiKey || !baseUrl) return undefined

    const timerId = window.setTimeout(() => {
      void fetchModels({ silent: true })
    }, 500)
    return () => window.clearTimeout(timerId)
  }, [fetchModels, form.apiKey, form.baseUrl])

  async function testConnection() {
    const apiKey = form.apiKey.trim()
    const baseUrl = form.baseUrl.trim()
    if (!apiKey || !baseUrl) {
      emitActionLog({
        module: 'ai-replies',
        action: 'test_connection_validation',
        outcome: 'warn',
        message: '测试 AI 连接校验失败：缺少 Base URL 或 API Key',
        context: { hasApiKey: Boolean(apiKey), hasBaseUrl: Boolean(baseUrl) },
        promoteToIncident: false,
      })
      notify.error('请先填写 Base URL 和 API Key')
      return
    }
    if (!form.modelName.trim()) {
      emitActionLog({
        module: 'ai-replies',
        action: 'test_connection_validation',
        outcome: 'warn',
        message: '测试 AI 连接校验失败：未选择模型',
        context: { apiType: form.apiType },
        promoteToIncident: false,
      })
      notify.error('请先选择模型')
      return
    }

    setConnectionTesting(true)
    emitActionStarted({
      module: 'ai-replies',
      action: 'test_connection',
      message: '开始测试 AI 连接',
      context: { apiType: form.apiType, hasModel: Boolean(form.modelName.trim()) },
    })
    try {
      const result = await testAiSettingsConnection({
        apiType: form.apiType,
        apiKey,
        baseUrl,
        modelName: form.modelName.trim(),
      })
      if (Array.isArray(result?.models) && result.models.length > 0) {
        setModelOptions(Array.from(new Set(result.models.map((model) => String(model || '').trim()).filter(Boolean))))
      }
      emitActionSuccess({
        module: 'ai-replies',
        action: 'test_connection',
        message: 'AI 连接测试成功',
        context: {
          apiType: form.apiType,
          returnedModelCount: Array.isArray(result?.models) ? result.models.length : 0,
        },
      })
      notify.success(result?.message || '连接成功')
    } catch (e) {
      emitActionFailure({
        module: 'ai-replies',
        action: 'test_connection',
        message: 'AI 连接测试失败',
        error: e,
        context: { apiType: form.apiType, hasModel: Boolean(form.modelName.trim()) },
      })
      notify.error(errorMessage(e, '测试连接失败'))
    } finally {
      setConnectionTesting(false)
    }
  }

  async function save() {
    setSaving(true)
    setError('')
    const scope = captureScope()
    emitActionStarted({
      module: 'ai-replies',
      action: 'save_global_settings',
      message: '开始保存全局 AI 配置',
      context: {
        apiType: form.apiType,
        hasApiKey: Boolean(form.apiKey.trim()),
        hasBaseUrl: Boolean(form.baseUrl.trim()),
        hasModel: Boolean(form.modelName.trim()),
        forbiddenWordsLength: form.forbiddenWords.length,
      },
    })
    try {
      await updateAiSettings(GLOBAL_SCOPE, {
        apiType: form.apiType,
        apiKey: form.apiKey,
        baseUrl: form.baseUrl,
        modelName: form.modelName,
        maxBargainRounds: Number(form.maxBargainRounds || 0),
        maxDiscountPercent: Number(form.maxDiscountPercent || 0),
        maxDiscountAmount: Number(form.maxDiscountAmount || 0),
        customPrompts: {
          system: form.systemPrompt,
          price: form.pricePrompt,
          tech: form.techPrompt,
          default: form.defaultPrompt,
          forbiddenWords: form.forbiddenWords,
        },
      })
      if (!isScopeCurrent(scope)) return
      await loadAccounts()
      emitAccountsSynced({ source: 'ai-replies-save-global' })
      emitActionSuccess({
        module: 'ai-replies',
        action: 'save_global_settings',
        message: '全局 AI 配置保存成功',
        context: { apiType: form.apiType, hasModel: Boolean(form.modelName.trim()) },
      })
    } catch (e) {
      emitActionFailure({
        module: 'ai-replies',
        action: 'save_global_settings',
        message: '全局 AI 配置保存失败',
        error: e,
        context: { apiType: form.apiType },
      })
      if (isScopeCurrent(scope)) setError(errorMessage(e, '保存 AI 配置失败'))
    } finally {
      if (isScopeCurrent(scope)) setSaving(false)
    }
  }

  function handleAccountSelection(nextUids: string[]) {
    const configuredSet = new Set(configuredUids)
    setSelectedPendingUids(
      sanitizeSelectableUids(nextUids, capabilityAccounts, 'aiEnabled', { allowEnableDisabledAccounts: true })
        .filter((uid) => !configuredSet.has(uid)),
    )
  }

  async function saveAccountSelection() {
    const configuredSet = new Set(configuredUids)
    const newUids = sanitizeSelectableUids(
      selectedPendingUids.filter((uid) => !configuredSet.has(uid)),
      capabilityAccounts,
      'aiEnabled',
      { allowEnableDisabledAccounts: true },
    )
    if (!newUids.length) return
    setSelectionSaving(true)
    setError('')
    const scope = captureScope('save-accounts')
    emitActionStarted({
      module: 'ai-replies',
      action: 'save_account_scope',
      message: '开始保存 AI 回复账号范围',
      context: { enableCount: newUids.length, disableCount: 0 },
    })
    try {
      const results = await Promise.allSettled(
        newUids.map((uid) => updateAccountSetting(uid, { replyMode: 'ai' })),
      )
      if (!isScopeCurrent(scope, 'save-accounts')) return

      const succeededUids = newUids.filter((_, index) => results[index].status === 'fulfilled')
      const failedUids = newUids.filter((_, index) => results[index].status === 'rejected')
      succeededUids.forEach((uid) => {
        locallyConfiguredUidsRef.current.add(uid)
        removedConfiguredUidsRef.current.delete(uid)
      })
      setConfiguredUids((uids) => [...new Set([...uids, ...succeededUids])])
      setSelectedPendingUids(failedUids)
      void loadAccounts({ force: true })
      if (succeededUids.length) {
        emitAccountsSynced({ uids: succeededUids, source: 'ai-replies-account-config' })
      }
      if (failedUids.length) {
        const firstFailure = results.find((result) => result.status === 'rejected')
        const message = succeededUids.length
          ? `已配置 ${succeededUids.length} 个账号，${failedUids.length} 个失败并保留待重试`
          : errorMessage(firstFailure?.reason, '保存账号应用范围失败')
        setError(message)
        notify.error(message)
        emitActionFailure({
          module: 'ai-replies',
          action: 'save_account_scope',
          message: succeededUids.length ? 'AI 回复账号范围部分保存失败' : 'AI 回复账号范围保存失败',
          error: firstFailure?.reason,
          context: { enableCount: newUids.length, succeededCount: succeededUids.length, failedCount: failedUids.length },
        })
      } else {
        emitActionSuccess({
          module: 'ai-replies',
          action: 'save_account_scope',
          message: 'AI 回复账号范围保存成功',
          context: { enableCount: succeededUids.length, disableCount: 0 },
        })
      }
    } catch (e) {
      if (isScopeCurrent(scope, 'save-accounts')) {
        const message = errorMessage(e, '保存账号应用范围失败')
        setError(message)
        notify.error(message)
        emitActionFailure({
          module: 'ai-replies',
          action: 'save_account_scope',
          message: 'AI 回复账号范围保存失败',
          error: e,
          context: { enableCount: newUids.length, disableCount: 0 },
        })
      }
    } finally {
      if (isScopeCurrent(scope, 'save-accounts')) setSelectionSaving(false)
    }
  }

  async function removeConfiguredAccount(uid: string) {
    if (removingConfiguredUid) return
    const account = activeAccounts.find((entry) => entry.uid === uid)
    const confirmed = await showConfirm(`确认移除“${account?.nick || uid}”的 AI 回复配置？`, {
      title: '移除 AI 回复账号',
      confirmLabel: '移除',
      danger: true,
    })
    if (!confirmed) return
    setRemovingConfiguredUid(uid)
    setError('')
    const scope = captureScope(`remove-account:${uid}`)
    emitActionStarted({
      module: 'ai-replies',
      action: 'remove_account_scope',
      message: '开始移除 AI 回复账号',
      context: { disableCount: 1 },
    })
    try {
      await updateAccountSetting(uid, { replyMode: 'off' })
      if (!isScopeCurrent(scope, `remove-account:${uid}`)) return

      locallyConfiguredUidsRef.current.delete(uid)
      removedConfiguredUidsRef.current.add(uid)
      setConfiguredUids((uids) => uids.filter((configuredUid) => configuredUid !== uid))
      setSelectedPendingUids((uids) => uids.filter((selectedUid) => selectedUid !== uid))
      void loadAccounts({ force: true })
      emitAccountsSynced({ uids: [uid], source: 'ai-replies-account-remove' })
      emitActionSuccess({
        module: 'ai-replies',
        action: 'remove_account_scope',
        message: 'AI 回复账号移除成功',
        context: { disableCount: 1 },
      })
    } catch (e) {
      if (isScopeCurrent(scope, `remove-account:${uid}`)) {
        const message = errorMessage(e, '移除 AI 回复账号失败')
        setError(message)
        notify.error(message)
        emitActionFailure({
          module: 'ai-replies',
          action: 'remove_account_scope',
          message: 'AI 回复账号移除失败',
          error: e,
          context: { disableCount: 1 },
        })
      }
    } finally {
      if (isScopeCurrent(scope, `remove-account:${uid}`)) setRemovingConfiguredUid('')
    }
  }

  return (
    <div className={compact ? 'h-full flex flex-col mobile-page-shell' : 'contents'} style={compact ? { backgroundColor: 'var(--bg-page)' } : undefined}>
      {compact ? (
        <TopBar
          title="AI 回复配置"
        />
      ) : null}

      {!compact && (
        <HubTabs
          tabs={HUB_PRESETS.automation}
        />
      )}

      <div className={compact ? 'flex-1 overflow-y-auto mobile-page-body' : 'flex-1 overflow-y-auto px-4 py-4 md:px-6'}>
        <div className={compact ? 'mobile-page-stack' : 'space-y-4'}>
          <div className={compact ? 'mobile-surface-card p-4 space-y-4' : 'rounded-[16px] border p-4 md:p-6 space-y-4'} style={compact ? undefined : { borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}>
            <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
              <div className="flex items-start gap-3">
                <div className={compact ? 'mobile-soft-card w-11 h-11 flex items-center justify-center shrink-0' : 'w-10 h-10 rounded-full flex items-center justify-center shrink-0'} style={{ background: 'var(--primary-light)', color: 'var(--primary)' }}>
                  <Bot className="w-5 h-5" />
                </div>
                <div className={compact ? 'mobile-section-copy' : undefined}>
                  <div className={compact ? 'mobile-section-title' : 'text-[15px] font-semibold'} style={{ color: compact ? undefined : 'var(--text-main)' }}>全局 AI 回复</div>
                </div>
              </div>
              {!compact ? (
                <button onClick={save} disabled={saving} className="btn-primary btn-sm disabled:opacity-40">
                  {saving ? '保存中…' : '保存全局 AI 配置'}
                </button>
              ) : null}
            </div>

            {compact ? (
              <button onClick={save} disabled={saving} className="btn-primary btn-sm w-full disabled:opacity-40">
                {saving ? '保存中…' : '保存全局 AI 配置'}
              </button>
            ) : null}

            {loading ? (
              <div className={compact ? 'mobile-soft-card px-4 py-12 text-center text-[13px]' : 'py-20 text-center text-[13px]'} style={{ color: 'var(--text-secondary)' }}>
                <RefreshCw className="w-4 h-4 animate-spin inline-block mr-2" />
                加载中…
              </div>
            ) : (
              <>
                <div data-testid="ai-connection-grid" className="grid gap-4 md:grid-cols-2">
                  <label className="block">
                    <span className="label">API 类型</span>
                    <select className="input" value={form.apiType} onChange={(e) => setField('apiType', e.target.value)}>
                      <option value="openai">OpenAI 兼容</option>
                      <option value="gemini">Gemini</option>
                      <option value="dashscope">DashScope</option>
                    </select>
                  </label>
                  <label className="block">
                    <span className="label">模型</span>
                    <select
                      className="input"
                      value={form.modelName}
                      onChange={(e) => setField('modelName', e.target.value)}
                      disabled={modelsLoading || modelOptions.length === 0}
                    >
                      <option value="">
                        {modelsLoading ? '模型加载中' : modelOptions.length > 0 ? '请选择模型' : '请先获取模型'}
                      </option>
                      {form.modelName && !modelOptions.includes(form.modelName) ? (
                        <option value={form.modelName} disabled>
                          当前配置：{form.modelName}
                        </option>
                      ) : null}
                      {modelOptions.map((model) => (
                        <option key={model} value={model}>{model}</option>
                      ))}
                    </select>
                  </label>
                  <label className="block">
                    <span className="label">Base URL</span>
                    <input className="input" value={form.baseUrl} onChange={(e) => setField('baseUrl', e.target.value)} placeholder="例如 https://api.openai.com/v1" />
                  </label>
                  <label className="block">
                    <span className="label">API Key</span>
                    <input className="input" value={form.apiKey} onChange={(e) => setField('apiKey', e.target.value)} placeholder="sk-..." />
                  </label>
                  <div className="md:col-span-2 flex flex-wrap items-center gap-2">
                    <button
                      onClick={() => void fetchModels({ force: true })}
                      disabled={modelsLoading}
                      className="btn-secondary btn-sm inline-flex items-center gap-1.5 disabled:opacity-40"
                    >
                      <Wand2 className="w-3.5 h-3.5" />
                      {modelsLoading ? '获取中…' : '获取模型'}
                    </button>
                    <button
                      onClick={() => void testConnection()}
                      disabled={connectionTesting || modelsLoading}
                      className="btn-secondary btn-sm inline-flex items-center gap-1.5 disabled:opacity-40"
                    >
                      <PlugZap className="w-3.5 h-3.5" />
                      {connectionTesting ? '测试中…' : '测试连接'}
                    </button>
                  </div>
                </div>

                <div className={compact ? 'mobile-grid-3' : 'grid gap-4 md:grid-cols-3'}>
                  <label className="block">
                    <span className="label">最大议价轮次</span>
                    <input type="number" min={0} max={20} className="input" value={form.maxBargainRounds} onChange={(e) => setField('maxBargainRounds', e.target.value)} />
                  </label>
                  <label className="block">
                    <span className="label">最大优惠百分比</span>
                    <input type="number" min={0} max={100} className="input" value={form.maxDiscountPercent} onChange={(e) => setField('maxDiscountPercent', e.target.value)} />
                  </label>
                  <label className="block">
                    <span className="label">最大优惠金额</span>
                    <input type="number" min={0} className="input" value={form.maxDiscountAmount} onChange={(e) => setField('maxDiscountAmount', e.target.value)} />
                  </label>
                </div>

                <div className="flex items-center justify-between gap-3">
                  <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
                    提示词模板
                  </div>
                  <button
                    onClick={applyDefaultPromptTemplates}
                    className="btn-ghost btn-sm inline-flex items-center gap-1.5"
                  >
                    <Wand2 className="w-3.5 h-3.5" />
                    恢复默认模板
                  </button>
                </div>

                <div data-testid="ai-prompt-grid" className={compact ? 'grid gap-4' : 'grid grid-cols-3 gap-4'}>
                  <label className="block">
                    <span className="label">系统提示词</span>
                    <textarea className="input min-h-[120px] resize-y" value={form.systemPrompt} onChange={(e) => setField('systemPrompt', e.target.value)} placeholder="定义 AI 的统一角色和禁区。" />
                  </label>
                  <label className="block">
                    <span className="label">价格意图提示词</span>
                    <textarea className="input min-h-[110px] resize-y" value={form.pricePrompt} onChange={(e) => setField('pricePrompt', e.target.value)} placeholder="用户询价、议价时优先使用。" />
                  </label>
                  <label className="block">
                    <span className="label">技术意图提示词</span>
                    <textarea className="input min-h-[110px] resize-y" value={form.techPrompt} onChange={(e) => setField('techPrompt', e.target.value)} placeholder="用户问功能、教程、兼容性时优先使用。" />
                  </label>
                  <label className="block">
                    <span className="label">通用意图提示词</span>
                    <textarea className="input min-h-[110px] resize-y" value={form.defaultPrompt} onChange={(e) => setField('defaultPrompt', e.target.value)} placeholder="不属于价格/技术的普通咨询走这里。" />
                  </label>
                  <label className="block">
                    <span className="label">违禁词配置</span>
                    <textarea
                      className="input min-h-[110px] resize-y"
                      value={form.forbiddenWords}
                      onChange={(e) => setField('forbiddenWords', e.target.value)}
                      placeholder="一行一个，或用逗号分隔。AI 回复会禁止使用这些词。"
                    />
                  </label>
                </div>

                {error ? (
                  <div className="liquid-notice liquid-notice-danger rounded-2xl px-4 py-3 text-[13px]">
                    {error}
                  </div>
                ) : null}
              </>
            )}
          </div>

          <section className={compact ? 'mobile-surface-card p-4 space-y-4' : 'card p-4 md:p-5 mb-4 space-y-4'}>
            {compact ? (
              <div className="mobile-section-head items-start">
                <div className="mobile-section-copy">
                  <h2 className="mobile-section-title">配置</h2>
                </div>
                <span className="mobile-pill mobile-muted-pill shrink-0">已配置 {configuredUids.length} 个</span>
              </div>
            ) : (
              <div className="flex flex-col gap-2">
                <div className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>配置</div>
                <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  保存只启用左侧本次勾选账号；右侧可逐个关闭 AI 回复。
                </div>
              </div>
            )}

            <AccountConfigurationSplit
              accounts={activeAccounts}
              configuredUids={configuredUids}
              selectedPendingUids={selectedPendingUids}
              onSelectionChange={handleAccountSelection}
              onSave={saveAccountSelection}
              onRemove={removeConfiguredAccount}
              saving={selectionSaving}
              disabled={loading}
              removingUid={removingConfiguredUid}
              pendingDescription="选择本次要启用 AI 回复的活跃账号。"
              configuredDescription="来源于账号真实 AI 开关或 AI 回复模式。"
              saveLabel="保存配置"
            />

            <div className={compact ? 'mobile-soft-card px-4 py-3 text-[12px]' : 'surface-card rounded-2xl px-4 py-3 text-[12px]'} style={{ color: 'var(--text-secondary)' }}>
              已勾选的账号会启用 AI 回复，并统一使用上面这套全局配置。
            </div>
          </section>
        </div>
      </div>
    </div>
  )
}
