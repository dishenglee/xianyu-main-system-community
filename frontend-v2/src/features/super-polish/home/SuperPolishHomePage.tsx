import {
  getSuperPolishAccountInfo,
  getSuperPolishCouponStatistics,
  getSuperPolishHome,
  getSuperPolishUnreadMessages,
  unwrapMtopData,
} from '../../../api/superPolish'
import { SuperPolishError, SuperPolishMetricGrid, SuperPolishSection } from '../components/SuperPolishUi'
import { useSuperPolishRequest } from '../hooks/useSuperPolishRequest'
import { asRecord, formatMoney, pickNumberString } from '../model'
import { summarizeHomeCampaignInfo } from './campaignSummary'

const ACCOUNT_INFO_SAMPLE = {
  orderPrice: 5000,
  promotionMode: 'custom',
  bidType: 'smart_bid',
  scene: 'creationRealTime',
}

export function SuperPolishHomePage() {
  const home = useSuperPolishRequest((uid) => getSuperPolishHome(uid))
  const accountInfo = useSuperPolishRequest((uid) => getSuperPolishAccountInfo(uid, ACCOUNT_INFO_SAMPLE))
  const unread = useSuperPolishRequest((uid) => getSuperPolishUnreadMessages(uid))
  const couponStats = useSuperPolishRequest((uid) => getSuperPolishCouponStatistics(uid))

  const homeData = asRecord(unwrapMtopData(home.data ?? {}))
  const homeCampaignInfo = asRecord(homeData.campaignInfo)
  const homeMemberData = asRecord(asRecord(homeData.memberData).data)
  const accountInfoData = asRecord(unwrapMtopData(accountInfo.data ?? {}))
  const unreadData = asRecord(unwrapMtopData(unread.data ?? {}))
  const couponStatsData = asRecord(unwrapMtopData(couponStats.data ?? {}))
  const campaignSummary = summarizeHomeCampaignInfo(homeCampaignInfo)

  const errorMessage = [
    home.error,
    accountInfo.error,
    unread.error,
    couponStats.error,
  ].find(Boolean) ?? ''

  const metrics = [
    { label: '全部计划', value: String(campaignSummary.total) },
    { label: '投放中计划', value: String(campaignSummary.active) },
    { label: '账户可用余额', value: formatMoney(accountInfoData.usableBalance ?? accountInfoData.cashBalance ?? 0) },
    { label: '未读消息', value: pickNumberString(unreadData, [['unreadCount']], '0') },
    { label: '点击量', value: pickNumberString(homeMemberData, [['clickPv']], '0') },
    { label: '咨询量', value: pickNumberString(homeMemberData, [['chatPv']], '0') },
    { label: '消耗', value: formatMoney(homeMemberData.cost ?? 0) },
    { label: '优惠券余额', value: pickNumberString(couponStatsData, [['activatedCouponAmountOfYuan']], '0') },
  ]

  return (
    <div className="space-y-4">
      <SuperPolishError message={errorMessage} />

      <SuperPolishSection
        title="首页概览"
        subtitle="聚合计划数、余额、消息、消耗和优惠券等首页核心指标。"
      >
        <SuperPolishMetricGrid items={metrics} />
      </SuperPolishSection>
    </div>
  )
}

export default SuperPolishHomePage
