import { useCallback, useMemo, useState } from 'react'
import { getSuperPolishCampaignGroups, getSuperPolishCampaigns, unwrapMtopData } from '../../../../api/superPolish'
import {
  asRecord,
  campaignListFromResponse,
  compactDate,
  daysAgoInput,
  groupListFromResponse,
  pickString,
  todayInput,
  type JsonRecord,
} from '../../model'
import { useSuperPolishRequest } from '../../hooks/useSuperPolishRequest'
import {
  mergeCurrentCampaignItems,
  readCampaignBidCent,
  readCampaignBidTarget,
  readCampaignBidType,
  readCampaignBudgetCent,
  readCampaignCreativeOptSwitch,
  readCampaignDivisionSelections,
  readCampaignOrderPrice,
  readCampaignPromotionMode,
  readCampaignScheduleHours,
} from './helpers'

export function useSuperPolishCampaignDetail({
  campaignId,
  seedCampaign,
}: {
  campaignId: string
  seedCampaign: JsonRecord | null
}) {
  const [startDate, setStartDate] = useState(daysAgoInput(6))
  const [endDate, setEndDate] = useState(todayInput())

  const detailPayload = useMemo(() => ({
    pageNum: 1,
    pageSize: 10,
    startDate: compactDate(startDate),
    endDate: compactDate(endDate),
    campaignIds: campaignId ? [Number(campaignId)] : [],
  }), [campaignId, endDate, startDate])
  const detailPayloadKey = JSON.stringify(detailPayload)

  const groupsPayload = useMemo(() => ({
    pageNum: 1,
    pageSize: 10,
    startDate: compactDate(startDate),
    endDate: compactDate(endDate),
    includeEndDate: true,
    campaignId: Number(campaignId),
  }), [campaignId, endDate, startDate])
  const groupsPayloadKey = JSON.stringify(groupsPayload)

  const campaignsRequest = useSuperPolishRequest(
    (uid) => getSuperPolishCampaigns(uid, detailPayload),
    [detailPayloadKey],
    Boolean(campaignId),
  )
  const groupsRequest = useSuperPolishRequest(
    (uid) => getSuperPolishCampaignGroups(uid, campaignId, groupsPayload),
    [campaignId, groupsPayloadKey],
    Boolean(campaignId),
  )

  const campaignRows = campaignListFromResponse(unwrapMtopData(campaignsRequest.data ?? {}))
  const groupRows = groupListFromResponse(unwrapMtopData(groupsRequest.data ?? {}))
  const campaign = useMemo(() => {
    const matched = campaignRows.find((row) => pickString(row, [['campaignId']], '') === campaignId)
    return matched ?? asRecord(seedCampaign)
  }, [campaignId, campaignRows, seedCampaign])

  const initialItems = useMemo(
    () => mergeCurrentCampaignItems(groupRows, campaign),
    [campaign, groupRows],
  )
  const initialItemsKey = useMemo(
    () => JSON.stringify({
      campaignId,
      itemIds: initialItems.map((item) => item.itemId),
      settings: {
        title: pickString(campaign, [['title'], ['itemTitle']], ''),
        bidTarget: readCampaignBidTarget(campaign),
        bidType: readCampaignBidType(campaign),
        promotionMode: readCampaignPromotionMode(campaign),
        bidCent: readCampaignBidCent(campaign),
        budgetCent: readCampaignBudgetCent(campaign),
        orderPrice: readCampaignOrderPrice(campaign),
        creativeOptSwitch: readCampaignCreativeOptSwitch(campaign),
        divisions: readCampaignDivisionSelections(campaign),
        hours: readCampaignScheduleHours(campaign),
      },
      seedItemIds: mergeCurrentCampaignItems([], campaign).map((item) => item.itemId),
    }),
    [campaign, campaignId, initialItems],
  )

  const reloadAll = useCallback(() => {
    campaignsRequest.reload()
    groupsRequest.reload()
  }, [campaignsRequest, groupsRequest])

  return {
    campaign,
    campaignRows,
    endDate,
    error: campaignsRequest.error || groupsRequest.error,
    groupRows,
    initialItems,
    initialItemsKey,
    loading: campaignsRequest.loading || groupsRequest.loading,
    reloadAll,
    setEndDate,
    setStartDate,
    startDate,
  }
}
