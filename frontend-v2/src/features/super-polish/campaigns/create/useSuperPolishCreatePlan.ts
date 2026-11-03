import { useCallback, useMemo, useState } from 'react'
import {
  createSuperPolishCampaign,
  estimateSuperPolishBids,
  getSuperPolishAccountInfo,
  getSuperPolishCampaignPrecheck,
  getSuperPolishRecommendedItems,
  querySuperPolishPackages,
  unwrapMtopData,
} from '../../../../api/superPolish'
import { useSuperPolishRequest } from '../../hooks/useSuperPolishRequest'
import { asRecord } from '../../model'
import { assertSuperPolishRequestSuccess } from '../../mtop'
import { buildCreateResultMetrics } from '../../presentation'
import { summarizeAreaTargetSupport } from '../areaTargetSupport'
import {
  ALL_HOURS,
  bidTargetLabel,
  buildAccountInfoPayload,
  buildBidTargetOrder,
  buildCreatePayload,
  buildTargetingEntityList,
  defaultPlanTitle,
  estimateDescription,
  firstCheckedItemIds,
  formatAdvancedSummary,
  formatCentMoney,
  formatDivisionSummary,
  formatFenInput,
  formatTimeSummary,
  limitedUniqueIds,
  mergeItems,
  normalizeBidEstimates,
  normalizePackageGroups,
  normalizeSelectableItems,
  packageOptionDescription,
  selectionKey,
  toFenAmount,
  upsertItems,
  type CustomBidMode,
  type DivisionSelection,
  type PromotionMode,
  type SuperPolishBidEstimate,
  type SuperPolishPackageGroup,
  type SuperPolishSelectableItem,
} from './helpers'

interface ConfirmDialog {
  showConfirm: (
    message: string,
    options: { confirmLabel: string; cancelLabel: string },
  ) => Promise<boolean>
}

type Notify = (message: string, type?: 'info' | 'success' | 'error') => void

interface CreateSuccessSnapshot {
  campaignIds: string[]
  createSuccess: boolean
  needRecharge: boolean
  title: string
}

function findEstimate(estimates: SuperPolishBidEstimate[], bidTarget: string) {
  return estimates.find((estimate) => estimate.bidTarget === bidTarget) ?? null
}

function findPackageGroup(groups: SuperPolishPackageGroup[], bidTarget: string) {
  return groups.find((group) => group.bidTarget === bidTarget) ?? null
}

function hasRechargeSignal(accountInfo: Record<string, unknown>) {
  return accountInfo.needReCharge !== undefined
    || accountInfo.usableBalance !== undefined
    || accountInfo.cashBalance !== undefined
}

function isRechargeRequired(accountInfo: Record<string, unknown>) {
  if (
    accountInfo.needReCharge === true
    || accountInfo.needReCharge === 'true'
    || accountInfo.needReCharge === 1
    || accountInfo.needReCharge === '1'
  ) {
    return true
  }

  const usableBalance = Number(accountInfo.usableBalance ?? accountInfo.cashBalance)
  return Number.isFinite(usableBalance) && usableBalance <= 0
}

export function useSuperPolishCreatePlan({
  uid,
  notify,
  dialog,
  onCreateSuccess,
}: {
  uid: string
  notify: Notify
  dialog: ConfirmDialog
  onCreateSuccess?: (snapshot: CreateSuccessSnapshot) => void
}) {
  const precheck = useSuperPolishRequest((accountUid) => getSuperPolishCampaignPrecheck(accountUid), [uid], Boolean(uid))
  const recommend = useSuperPolishRequest((accountUid) => getSuperPolishRecommendedItems(accountUid), [uid], Boolean(uid))

  const precheckData = useMemo(() => asRecord(unwrapMtopData(precheck.data ?? {})), [precheck.data])
  const recommendData = useMemo(() => asRecord(unwrapMtopData(recommend.data ?? {})), [recommend.data])
  const recommendedItems = useMemo(
    () => normalizeSelectableItems(recommendData.result ?? recommendData),
    [recommendData],
  )

  const [rememberedItems, setRememberedItems] = useState<SuperPolishSelectableItem[]>([])
  const [selectedIds, setSelectedIds] = useState<string[] | null>(null)
  const [selectedBidTarget, setSelectedBidTarget] = useState('')
  const [promotionMode, setPromotionMode] = useState<PromotionMode>('order')
  const [title, setTitle] = useState(() => defaultPlanTitle())
  const [selectedPackageIndex, setSelectedPackageIndex] = useState<Record<string, number>>({})
  const [customBidMode, setCustomBidMode] = useState<CustomBidMode>('recommend')
  const [customBidInput, setCustomBidInput] = useState('')
  const [budgetInput, setBudgetInput] = useState('')
  const [budgetTouched, setBudgetTouched] = useState(false)
  const [selectedDivisions, setSelectedDivisions] = useState<DivisionSelection[]>([])
  const [selectedHours, setSelectedHours] = useState<number[]>(ALL_HOURS)
  const [creativeOptEnabled, setCreativeOptEnabled] = useState(true)
  const [creativeOptTouched, setCreativeOptTouched] = useState(false)
  const [createResult, setCreateResult] = useState<unknown>(null)
  const [submitBusy, setSubmitBusy] = useState(false)

  const itemMap = useMemo(() => mergeItems(recommendedItems, rememberedItems), [recommendedItems, rememberedItems])
  const defaultSelectedIds = useMemo(() => firstCheckedItemIds(recommendedItems), [recommendedItems])
  const effectiveSelectedIds = selectedIds ?? defaultSelectedIds
  const selectedItems = useMemo(
    () => effectiveSelectedIds.map((itemId) => itemMap.get(itemId)).filter(Boolean) as SuperPolishSelectableItem[],
    [effectiveSelectedIds, itemMap],
  )
  const areaTargetSupport = useMemo(
    () => summarizeAreaTargetSupport(selectedItems),
    [selectedItems],
  )
  const areaTargetAvailable = areaTargetSupport.available
  const effectiveSelectedDivisions = useMemo(
    () => (areaTargetAvailable ? selectedDivisions : []),
    [areaTargetAvailable, selectedDivisions],
  )
  const areaTargetForcesCustom = effectiveSelectedDivisions.length > 0
  const areaTargetEnabled = effectiveSelectedDivisions.length > 0

  const targetingEntityList = useMemo(
    () => buildTargetingEntityList(selectedItems, areaTargetEnabled),
    [areaTargetEnabled, selectedItems],
  )

  const packagePayload = useMemo(() => ({
    entityType: 1,
    entityList: targetingEntityList,
    isGoodsSelectedTabChange: true,
  }), [targetingEntityList])
  const estimatePayload = useMemo(() => ({
    entityType: 1,
    entityList: targetingEntityList,
    pcFirstRequest: true,
  }), [targetingEntityList])

  const packages = useSuperPolishRequest(
    (accountUid) => querySuperPolishPackages(accountUid, packagePayload),
    [packagePayload],
    Boolean(uid && targetingEntityList.length > 0),
  )
  const estimates = useSuperPolishRequest(
    (accountUid) => estimateSuperPolishBids(accountUid, estimatePayload),
    [estimatePayload],
    Boolean(uid && targetingEntityList.length > 0),
  )

  const packageGroups = useMemo(
    () => normalizePackageGroups(unwrapMtopData(packages.data ?? {})),
    [packages.data],
  )
  const estimateData = useMemo(() => asRecord(unwrapMtopData(estimates.data ?? {})), [estimates.data])
  const bidEstimates = useMemo(
    () => normalizeBidEstimates(estimateData),
    [estimateData],
  )
  const preferredBidTarget = String(estimateData.recBidTarget ?? '').trim()
  const bidTargets = useMemo(
    () => buildBidTargetOrder(packageGroups, bidEstimates, preferredBidTarget),
    [bidEstimates, packageGroups, preferredBidTarget],
  )
  const effectiveSelectedBidTarget = selectedBidTarget && bidTargets.includes(selectedBidTarget)
    ? selectedBidTarget
    : (preferredBidTarget && bidTargets.includes(preferredBidTarget) ? preferredBidTarget : (bidTargets[0] ?? ''))

  const activePackageGroup = useMemo(
    () => findPackageGroup(packageGroups, effectiveSelectedBidTarget),
    [effectiveSelectedBidTarget, packageGroups],
  )
  const activeEstimate = useMemo(
    () => findEstimate(bidEstimates, effectiveSelectedBidTarget),
    [bidEstimates, effectiveSelectedBidTarget],
  )

  const selectedPackageOption = useMemo(() => {
    const group = activePackageGroup
    if (!group || group.options.length === 0) return null
    const recommendedIndex = group.options.findIndex((option) => option.isRecommended)
    const fallbackIndex = recommendedIndex >= 0 ? recommendedIndex : 0
    const optionIndex = selectedPackageIndex[group.bidTarget] ?? fallbackIndex
    return group.options[optionIndex] ?? group.options[0] ?? null
  }, [activePackageGroup, selectedPackageIndex])
  const defaultBudget = activeEstimate?.targetRecommendBudget ?? activeEstimate?.recommendBudgetList[0] ?? 0
  const effectiveBudgetInput = budgetTouched ? budgetInput : formatFenInput(defaultBudget)
  const defaultManualBidInput = formatFenInput(activeEstimate?.customBidPrice ?? activeEstimate?.bidPrice)
  const effectiveCustomBidInput = customBidMode === 'recommend'
    ? formatFenInput(activeEstimate?.bidPrice)
    : (customBidInput || defaultManualBidInput)

  const creativeOptAvailable = Boolean(
    (precheckData?.extraFunctions && asRecord(precheckData.extraFunctions).showCreativeOptSwitch)
      || activeEstimate?.creativeOptimizationAvailable
      || selectedPackageOption?.creativeOptimizationAvailable,
  )
  const effectiveCreativeOptEnabled = creativeOptAvailable && (creativeOptTouched ? creativeOptEnabled : true)

  const resolvedBidType = selectedPackageOption?.bidType ?? activeEstimate?.bidType ?? 'smart_bid'
  const resolvedBidPrice = customBidMode === 'manual'
    ? toFenAmount(effectiveCustomBidInput)
    : (activeEstimate?.bidPrice ?? null)
  const resolvedBudget = toFenAmount(effectiveBudgetInput)

  const accountInfoPayload = useMemo(() => buildAccountInfoPayload({
    promotionMode,
    orderPrice: promotionMode === 'order'
      ? (selectedPackageOption?.orderPrice ?? null)
      : resolvedBudget,
    bidType: resolvedBidType,
  }), [promotionMode, resolvedBidType, resolvedBudget, selectedPackageOption?.orderPrice])

  const accountInfo = useSuperPolishRequest(
    (accountUid) => getSuperPolishAccountInfo(accountUid, accountInfoPayload ?? {}),
    [accountInfoPayload],
    Boolean(uid && accountInfoPayload),
  )
  const accountInfoData = useMemo(() => asRecord(unwrapMtopData(accountInfo.data ?? {})), [accountInfo.data])

  const createPayload = useMemo(() => buildCreatePayload({
    oldMember: precheckData.oldMember === false ? false : true,
    items: selectedItems,
    promotionMode,
    bidTarget: effectiveSelectedBidTarget,
    bidType: resolvedBidType,
    title: title.trim() || defaultPlanTitle(),
    packageOption: selectedPackageOption,
    bidPrice: resolvedBidPrice,
    maxDayBudget: resolvedBudget,
    budgetScheduleDurations: selectedHours,
    divisions: effectiveSelectedDivisions,
    creativeOptSwitch: effectiveCreativeOptEnabled,
  }), [
    effectiveCreativeOptEnabled,
    effectiveSelectedBidTarget,
    effectiveSelectedDivisions,
    precheckData.oldMember,
    promotionMode,
    resolvedBidPrice,
    resolvedBidType,
    resolvedBudget,
    selectedHours,
    selectedItems,
    selectedPackageOption,
    title,
  ])

  const createMetrics = useMemo(() => buildCreateResultMetrics(createResult), [createResult])
  const createdCampaignId = useMemo(() => {
    const ids = Array.isArray(asRecord(createResult).campaignIds)
      ? (asRecord(createResult).campaignIds as unknown[]).map((value) => String(value)).filter(Boolean)
      : []
    return ids[0] ?? ''
  }, [createResult])

  const precheckNotice = useMemo(() => {
    if (precheck.loading) {
      return '正在校验建计划能力，请稍候继续配置。'
    }
    if (precheck.error) {
      return '建计划前校验暂未完成，下面配置会继续按接口实时返回结果更新。'
    }

    return '计划能力已校验，可直接从平台推荐或搜索商品开始创建。'
  }, [precheck.error, precheck.loading])

  const summaryLines = useMemo(() => {
    const lines = [
      `当前将为 ${selectedItems.length} 个宝贝发起${promotionMode === 'order' ? '套餐擦亮' : '自定义擦亮'}`,
    ]
    if (effectiveSelectedBidTarget) {
      lines.push(`目标：${bidTargetLabel(effectiveSelectedBidTarget)}`)
    }
    lines.push(formatAdvancedSummary(effectiveSelectedDivisions, selectedHours))
    return lines
  }, [effectiveSelectedBidTarget, effectiveSelectedDivisions, promotionMode, selectedHours, selectedItems.length])

  const divisionSummary = useMemo(
    () => formatDivisionSummary(effectiveSelectedDivisions),
    [effectiveSelectedDivisions],
  )
  const timeSummary = useMemo(
    () => formatTimeSummary(selectedHours),
    [selectedHours],
  )

  const rememberItems = useCallback((items: SuperPolishSelectableItem[]) => {
    setRememberedItems((current) => upsertItems(current, items))
  }, [])

  function addItems(items: SuperPolishSelectableItem[]) {
    rememberItems(items)
    setSelectedIds((current) => limitedUniqueIds(current ?? defaultSelectedIds, items.map((item) => item.itemId)))
    setCreateResult(null)
  }

  function toggleItem(item: SuperPolishSelectableItem) {
    if (!item.supportAd) return
    setSelectedIds((current) => {
      const active = current ?? defaultSelectedIds
      if (active.includes(item.itemId)) {
        return active.filter((value) => value !== item.itemId)
      }
      return limitedUniqueIds(active, [item.itemId])
    })
    rememberItems([item])
    setCreateResult(null)
  }

  function removeSelectedItem(itemId: string) {
    setSelectedIds((current) => (current ?? defaultSelectedIds).filter((value) => value !== itemId))
    setCreateResult(null)
  }

  async function clearSelected() {
    const confirmed = await dialog.showConfirm('确认清空当前已添加的宝贝吗？', {
      confirmLabel: '确认清空',
      cancelLabel: '取消',
    })
    if (!confirmed) return
    setSelectedIds([])
    setCreateResult(null)
  }

  function changePromotionMode(mode: PromotionMode) {
    if (mode === 'order' && areaTargetForcesCustom) {
      notify('地域定向仅支持自定义擦亮，已保留当前地域设置。', 'info')
      return
    }
    setPromotionMode(mode)
    setCreateResult(null)
  }

  function choosePackageOption(index: number) {
    if (!effectiveSelectedBidTarget) return
    setSelectedPackageIndex((current) => ({ ...current, [effectiveSelectedBidTarget]: index }))
    setCreateResult(null)
  }

  function changeCustomBidMode(mode: CustomBidMode) {
    setCustomBidMode(mode)
    if (mode === 'manual' && activeEstimate) {
      setCustomBidInput(formatFenInput(activeEstimate.customBidPrice ?? activeEstimate.bidPrice))
    }
    if (mode === 'recommend') {
      setCustomBidInput('')
    }
    setCreateResult(null)
  }

  function updateCustomBidInput(value: string) {
    setCustomBidInput(value)
    setCreateResult(null)
  }

  function updateBudgetInput(value: string) {
    setBudgetTouched(true)
    setBudgetInput(value)
    setCreateResult(null)
  }

  function updateTitle(value: string) {
    setTitle(value)
    setCreateResult(null)
  }

  function applyAdvancedSettings(next: {
    divisions: DivisionSelection[]
    hours: number[]
    creativeOptEnabled: boolean
  }) {
    const dedupedDivisions = Array.from(new Map(next.divisions.map((entry) => [selectionKey(entry), entry])).values())
    setSelectedDivisions(dedupedDivisions)
    if (dedupedDivisions.length > 0) {
      setPromotionMode('custom')
    }
    setSelectedHours(next.hours.length > 0 ? next.hours.slice().sort((left, right) => left - right) : ALL_HOURS)
    setCreativeOptTouched(true)
    setCreativeOptEnabled(next.creativeOptEnabled)
    setCreateResult(null)
  }

  function updateCreativeOptEnabled(value: boolean) {
    setCreativeOptTouched(true)
    setCreativeOptEnabled(value)
    setCreateResult(null)
  }

  const submitDisabled = !uid
    || selectedItems.length === 0
    || !effectiveSelectedBidTarget
    || (promotionMode === 'order' && !selectedPackageOption)
    || (promotionMode === 'custom' && (!resolvedBidPrice || !resolvedBudget))

  async function submitCreate() {
    if (submitDisabled) return
    const confirmed = await dialog.showConfirm(`确认立即擦亮这 ${selectedItems.length} 个宝贝吗？`, {
      confirmLabel: '立即擦亮',
      cancelLabel: '取消',
    })
    if (!confirmed) return
    setSubmitBusy(true)
    try {
      const response = assertSuperPolishRequestSuccess(await createSuperPolishCampaign(uid, createPayload))
      const result = unwrapMtopData(response)
      setCreateResult(result)
      notify('擦亮计划已提交', 'success')
      const resultRecord = asRecord(result)
      const campaignIds = Array.isArray(resultRecord.campaignIds)
        ? (resultRecord.campaignIds as unknown[]).map((value) => String(value)).filter(Boolean)
        : []
      const createSuccess = resultRecord.createSuccess === true
        || resultRecord.createSuccess === 'true'
        || resultRecord.createSuccess === 1
        || resultRecord.createSuccess === '1'
        || campaignIds.length > 0
      if (createSuccess) {
        let latestAccountInfo = accountInfoData
        if (!hasRechargeSignal(latestAccountInfo)) {
          try {
            latestAccountInfo = asRecord(
              unwrapMtopData(
                assertSuperPolishRequestSuccess(
                  await getSuperPolishAccountInfo(uid, accountInfoPayload ?? {}),
                ),
              ),
            )
          } catch {
            latestAccountInfo = accountInfoData
          }
        }
        onCreateSuccess?.({
          campaignIds,
          createSuccess,
          needRecharge: isRechargeRequired(latestAccountInfo),
          title: title.trim() || defaultPlanTitle(),
        })
      }
    } catch (error) {
      notify(error instanceof Error ? error.message : '提交失败', 'error')
    } finally {
      setSubmitBusy(false)
    }
  }

  return {
    precheck,
    precheckNotice,
    recommend,
    recommendedItems,
    selectedIds: effectiveSelectedIds,
    selectedItems,
    rememberItems,
    addItems,
    toggleItem,
    removeSelectedItem,
    clearSelected,
    bidTargets,
    selectedBidTarget: effectiveSelectedBidTarget,
    setSelectedBidTarget: (value: string) => {
      setSelectedBidTarget(value)
      setCreateResult(null)
    },
    bidTargetLabel,
    promotionMode,
    changePromotionMode,
    title,
    setTitle: updateTitle,
    packages,
    estimates,
    activePackageGroup,
    selectedPackageOption,
    choosePackageOption,
    packageOptionDescription,
    activeEstimate,
    estimateDescription,
    customBidMode,
    changeCustomBidMode,
    customBidInput: effectiveCustomBidInput,
    updateCustomBidInput,
    budgetInput: effectiveBudgetInput,
    updateBudgetInput,
    creativeOptAvailable,
    creativeOptEnabled: effectiveCreativeOptEnabled,
    updateCreativeOptEnabled,
    selectedDivisions: effectiveSelectedDivisions,
    areaTargetForcesCustom,
    selectedHours,
    divisionSummary,
    timeSummary,
    applyAdvancedSettings,
    areaTargetAvailable,
    areaTargetSupport,
    accountInfo,
    accountInfoData,
    accountInfoPayload,
    submitBusy,
    submitDisabled,
    submitCreate,
    createMetrics,
    createdCampaignId,
    summaryLines,
    formatCentMoney,
    createPayload,
  }
}
