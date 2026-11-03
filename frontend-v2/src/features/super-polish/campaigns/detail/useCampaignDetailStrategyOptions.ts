import { useMemo } from 'react'
import {
  estimateSuperPolishBids,
  getSuperPolishAccountInfo,
  querySuperPolishPackages,
  unwrapMtopData,
} from '../../../../api/superPolish'
import { useSuperPolishRequest } from '../../hooks/useSuperPolishRequest'
import { asRecord } from '../../model'
import {
  buildAccountInfoPayload,
  buildBidTargetOrder,
  normalizeBidEstimates,
  normalizePackageGroups,
  toFenAmount,
  type SuperPolishBidEstimate,
  type SuperPolishPackageGroup,
  type SuperPolishPackageOption,
} from '../create/helpers'
import {
  buildCampaignDetailTargetingEntityList,
  resolveCampaignPackageSelection,
} from './helpers'
import type {
  CampaignDetailPackageSelection,
  SuperPolishCampaignDetailItem,
  SuperPolishCampaignSettingsDraft,
} from './types'

function findEstimate(estimates: SuperPolishBidEstimate[], bidTarget: string) {
  return estimates.find((estimate) => estimate.bidTarget === bidTarget) ?? null
}

function findPackageGroup(groups: SuperPolishPackageGroup[], bidTarget: string) {
  return groups.find((group) => group.bidTarget === bidTarget) ?? null
}

function findPackageOption(
  group: SuperPolishPackageGroup | null,
  draft: Pick<
    SuperPolishCampaignSettingsDraft,
    'selectedPackageByBidTarget' | 'selectedPackageOrderPriceByBidTarget'
  >,
  bidTarget: string,
) {
  const options = group?.options ?? []
  const rawOrderPrice = draft.selectedPackageOrderPriceByBidTarget[bidTarget]
    ?? draft.selectedPackageByBidTarget[bidTarget]?.orderPrice
  const savedOrderPrice = rawOrderPrice === null || rawOrderPrice === undefined
    ? NaN
    : Number(rawOrderPrice)
  if (Number.isFinite(savedOrderPrice)) {
    const matched = options.find((option) => option.orderPrice === savedOrderPrice)
    if (matched) return matched
  }
  return options.find((option) => option.isRecommended) ?? options[0] ?? null
}

export function useCampaignDetailStrategyOptions({
  items,
  draft,
  areaTargetEnabled,
}: {
  items: SuperPolishCampaignDetailItem[]
  draft: Pick<
    SuperPolishCampaignSettingsDraft,
    | 'bidTarget'
    | 'promotionMode'
    | 'budgetInput'
    | 'selectedPackageByBidTarget'
    | 'selectedPackageOrderPriceByBidTarget'
  >
  areaTargetEnabled: boolean
}) {
  const targetingEntityList = useMemo(
    () => buildCampaignDetailTargetingEntityList(items, areaTargetEnabled),
    [areaTargetEnabled, items],
  )
  const targetingEntityKey = JSON.stringify(targetingEntityList)

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
    (uid) => querySuperPolishPackages(uid, packagePayload),
    [targetingEntityKey, 'packages'],
    targetingEntityList.length > 0,
  )
  const estimates = useSuperPolishRequest(
    (uid) => estimateSuperPolishBids(uid, estimatePayload),
    [targetingEntityKey, 'estimates'],
    targetingEntityList.length > 0,
  )

  const packageGroups = useMemo(
    () => normalizePackageGroups(unwrapMtopData(packages.data ?? {})),
    [packages.data],
  )
  const estimateData = useMemo(
    () => asRecord(unwrapMtopData(estimates.data ?? {})),
    [estimates.data],
  )
  const bidEstimates = useMemo(
    () => normalizeBidEstimates(estimateData),
    [estimateData],
  )
  const preferredBidTarget = String(estimateData.recBidTarget ?? '').trim()
  const bidTargets = useMemo(
    () => buildBidTargetOrder(packageGroups, bidEstimates, preferredBidTarget),
    [bidEstimates, packageGroups, preferredBidTarget],
  )
  const effectiveBidTarget = draft.bidTarget && bidTargets.includes(draft.bidTarget)
    ? draft.bidTarget
    : (preferredBidTarget && bidTargets.includes(preferredBidTarget) ? preferredBidTarget : (bidTargets[0] ?? draft.bidTarget))

  const activePackageGroup = useMemo(
    () => findPackageGroup(packageGroups, effectiveBidTarget),
    [effectiveBidTarget, packageGroups],
  )
  const activeEstimate = useMemo(
    () => findEstimate(bidEstimates, effectiveBidTarget),
    [bidEstimates, effectiveBidTarget],
  )
  const selectedPackageOption = useMemo(
    () => findPackageOption(activePackageGroup, draft, effectiveBidTarget),
    [activePackageGroup, draft, effectiveBidTarget],
  )
  const selectedPackageSelection = useMemo(
    () => resolveCampaignPackageSelection(effectiveBidTarget, draft, activePackageGroup),
    [activePackageGroup, draft, effectiveBidTarget],
  )
  const resolvedBidType = selectedPackageOption?.bidType ?? activeEstimate?.bidType ?? 'smart_bid'
  const resolvedBudget = toFenAmount(draft.budgetInput)
  const accountInfoPayload = useMemo(() => buildAccountInfoPayload({
    promotionMode: draft.promotionMode,
    orderPrice: draft.promotionMode === 'order'
      ? (selectedPackageOption?.orderPrice ?? null)
      : resolvedBudget,
    bidType: resolvedBidType,
  }), [draft.promotionMode, resolvedBidType, resolvedBudget, selectedPackageOption?.orderPrice])
  const accountInfo = useSuperPolishRequest(
    (uid) => getSuperPolishAccountInfo(uid, accountInfoPayload ?? {}),
    [accountInfoPayload, 'account-info'],
    Boolean(accountInfoPayload),
  )
  const accountInfoData = useMemo(
    () => asRecord(unwrapMtopData(accountInfo.data ?? {})),
    [accountInfo.data],
  )

  return {
    packages,
    estimates,
    accountInfo,
    accountInfoData,
    resolvedBidType,
    bidTargets,
    effectiveBidTarget,
    activeEstimate,
    activePackageGroup,
    selectedPackageOption: selectedPackageOption as SuperPolishPackageOption | null,
    selectedPackageSelection: selectedPackageSelection as CampaignDetailPackageSelection | null,
  }
}
