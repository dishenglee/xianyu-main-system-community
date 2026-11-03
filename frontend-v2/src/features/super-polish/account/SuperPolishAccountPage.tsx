import { useMemo, useState } from 'react'
import {
  getSuperPolishAccountCard,
  getSuperPolishActivityReward,
  getSuperPolishCouponStatistics,
  getSuperPolishCoupons,
  getSuperPolishLoginInfo,
  getSuperPolishMemberData,
  getSuperPolishQualificationCategories,
  getSuperPolishQualificationRecords,
  getSuperPolishUnreadMessages,
  unwrapMtopData,
} from '../../../api/superPolish'
import { useSuperPolishRequest } from '../hooks/useSuperPolishRequest'
import { asArray, asRecord, compactDate, daysAgoInput, extractRecords, formatMoney, pickNumberString, pickString, todayInput } from '../model'
import {
  SuperPolishBadge,
  SuperPolishError,
  SuperPolishField,
  SuperPolishInput,
  SuperPolishKeyValueList,
  SuperPolishMetricGrid,
  SuperPolishSection,
  SuperPolishSelect,
} from '../components/SuperPolishUi'
import { buildBooleanMetric, campaignStatusMeta } from '../presentation'

export function SuperPolishAccountPage() {
  const [startDate, setStartDate] = useState(daysAgoInput(6))
  const [endDate, setEndDate] = useState(todayInput())
  const [recordStatus, setRecordStatus] = useState('')
  const [categoryId, setCategoryId] = useState('')

  const memberPayload = useMemo(() => ({
    startDate: compactDate(startDate),
    endDate: compactDate(endDate),
    includeEndDate: true,
    pageNum: 1,
    pageSize: 10,
    allPeriod: false,
  }), [endDate, startDate])
  const recordPayload = useMemo(() => ({
    status: recordStatus,
    categoryId,
    pageNum: 1,
    pageSize: 10,
  }), [categoryId, recordStatus])
  const memberKey = JSON.stringify(memberPayload)
  const recordKey = JSON.stringify(recordPayload)

  const loginInfo = useSuperPolishRequest((uid) => getSuperPolishLoginInfo(uid))
  const accountCard = useSuperPolishRequest((uid) => getSuperPolishAccountCard(uid, { scene: 'PC_HOME' }))
  const unread = useSuperPolishRequest((uid) => getSuperPolishUnreadMessages(uid))
  const memberData = useSuperPolishRequest(
    (uid) => getSuperPolishMemberData(uid, memberPayload),
    [memberKey],
  )
  const activityReward = useSuperPolishRequest((uid) => getSuperPolishActivityReward(uid))
  const coupons = useSuperPolishRequest((uid) => getSuperPolishCoupons(uid, { pageNum: 1, pageSize: 10, couponType: 0, source: 'BONUS' }))
  const couponStats = useSuperPolishRequest((uid) => getSuperPolishCouponStatistics(uid))
  const qualificationCategories = useSuperPolishRequest((uid) => getSuperPolishQualificationCategories(uid))
  const qualificationRecords = useSuperPolishRequest(
    (uid) => getSuperPolishQualificationRecords(uid, recordPayload),
    [recordKey],
  )

  const loginInfoData = asRecord(unwrapMtopData(loginInfo.data ?? {}))
  const accountCardData = asRecord(unwrapMtopData(accountCard.data ?? {}))
  const unreadData = asRecord(unwrapMtopData(unread.data ?? {}))
  const memberDataPayload = asRecord(unwrapMtopData(memberData.data ?? {}))
  const couponStatsData = asRecord(unwrapMtopData(couponStats.data ?? {}))
  const activityRewardData = asRecord(unwrapMtopData(activityReward.data ?? {}))
  const balanceRows = asArray<Record<string, unknown>>(accountCardData.bonusAccountDetailList)
  const couponRows = extractRecords(unwrapMtopData(coupons.data ?? {}), [['result']])
  const categoryRows = extractRecords(unwrapMtopData(qualificationCategories.data ?? {}), [['result']])
  const recordRows = extractRecords(unwrapMtopData(qualificationRecords.data ?? {}), [['result']])

  const errorMessage = [
    loginInfo.error,
    accountCard.error,
    unread.error,
    memberData.error,
    activityReward.error,
    coupons.error,
    couponStats.error,
    qualificationCategories.error,
    qualificationRecords.error,
  ].find(Boolean) ?? ''

  const metrics = [
    { label: '展示昵称', value: pickString(loginInfoData, [['displayNick'], ['userNick']], '未返回') },
    { label: '未读消息', value: pickNumberString(unreadData, [['unreadCount']], '0') },
    { label: '现金余额', value: pickString(accountCardData, [['cashBalanceOfYuan'], ['cashBalance']], '0') },
    { label: '礼金余额', value: pickString(accountCardData, [['bonusBalanceOfYuan']], '0') },
    { label: '已激活券额', value: pickString(couponStatsData, [['activatedCouponAmountOfYuan']], '0') },
    { label: '即将过期券额', value: pickString(couponStatsData, [['expiringCouponAmountOfYuan']], '0') },
    { label: '点击量', value: pickNumberString(memberDataPayload, [['data', 'clickPv'], ['clickPv']], '0') },
    { label: '消耗', value: formatMoney(asRecord(memberDataPayload.data).cost ?? memberDataPayload.cost ?? 0) },
  ]
  const memberMetrics = [
    { label: '点击量', value: pickNumberString(memberDataPayload, [['data', 'clickPv'], ['clickPv']], '0') },
    { label: '咨询量', value: pickNumberString(memberDataPayload, [['data', 'chatPv'], ['chatPv']], '0') },
    { label: '收藏量', value: pickNumberString(memberDataPayload, [['data', 'favorites'], ['favorites']], '0') },
    { label: '订单量', value: pickNumberString(memberDataPayload, [['data', 'orderInquiries'], ['orderInquiries']], '0') },
  ]
  const accountFlags = [
    buildBooleanMetric({
      label: '子账号登录',
      value: loginInfoData.subAccount,
      whenTrue: '是',
      whenFalse: '否',
      helperWhenTrue: '当前是超级擦亮子账号视角。',
      helperWhenFalse: '当前是主账号视角。',
    }),
    buildBooleanMetric({
      label: '伪登录态',
      value: loginInfoData.pseudoLogin,
      whenTrue: '已启用',
      whenFalse: '未启用',
      helperWhenTrue: '当前账号处于代运营或伪登录态。',
      helperWhenFalse: '当前登录态正常。',
    }),
    {
      label: '活动奖励',
      value: Object.keys(activityRewardData).length > 0 ? '有活动信息' : '暂无活动',
      helper: Object.keys(activityRewardData).length > 0 ? '奖励页已有返回，可继续跟进。' : '当前没有返回额外活动奖励。',
      tone: Object.keys(activityRewardData).length > 0 ? 'success' as const : 'neutral' as const,
    },
  ]

  return (
    <div className="space-y-4">
      <SuperPolishError message={errorMessage} />

      <SuperPolishSection title="账户与资质总览" subtitle="集中承接登录信息、账户卡、成员数据、优惠券、资质类目和资质记录。">
        <SuperPolishMetricGrid items={metrics} />
      </SuperPolishSection>

      <SuperPolishSection title="成员数据" subtitle="对应 `member.data.query`，保留日期范围筛选。">
        <div className="grid gap-3 md:grid-cols-3">
          <SuperPolishField label="开始日期">
            <SuperPolishInput type="date" value={startDate} onChange={(event) => setStartDate(event.target.value)} />
          </SuperPolishField>
          <SuperPolishField label="结束日期">
            <SuperPolishInput type="date" value={endDate} onChange={(event) => setEndDate(event.target.value)} />
          </SuperPolishField>
        </div>

        <div className="mt-4">
          <SuperPolishMetricGrid items={memberMetrics} />
        </div>
      </SuperPolishSection>

      <SuperPolishSection title="账户状态" subtitle="把登录态、奖励和余额明细转成日常运营可直接阅读的状态卡。">
        <SuperPolishMetricGrid items={accountFlags} />

        <div className="mt-4 grid gap-4 xl:grid-cols-2">
          <div
            className="rounded-[18px] border px-4 py-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
          >
            <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>登录与操作员</div>
            <div className="mt-3">
              <SuperPolishKeyValueList
                items={[
                  { label: '展示昵称', value: pickString(loginInfoData, [['displayNick'], ['userNick']], '未返回') },
                  { label: '操作员', value: pickString(loginInfoData, [['operatorDisplayNick'], ['operatorUserNick']], '未返回') },
                  { label: '主账号 ID', value: pickString(loginInfoData, [['userId']], '未返回') },
                  { label: '操作员 ID', value: pickString(loginInfoData, [['operatorUserId']], '未返回') },
                ]}
              />
            </div>
          </div>

          <div
            className="rounded-[18px] border px-4 py-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
          >
            <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>余额明细</div>
            <div className="mt-3 space-y-2">
              <div className="flex items-center justify-between text-[12px]">
                <span style={{ color: 'var(--text-secondary)' }}>现金余额</span>
                <span style={{ color: 'var(--text-main)' }} className="font-medium">
                  {pickString(accountCardData, [['cashBalanceOfYuan'], ['cashBalance']], '0')}
                </span>
              </div>
              {balanceRows.length === 0 ? (
                <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>当前没有返回礼金或红包明细。</div>
              ) : balanceRows.map((item, index) => (
                <div key={`${pickString(item, [['accountTypeDesc']], 'balance')}-${index}`} className="flex items-center justify-between text-[12px]">
                  <span style={{ color: 'var(--text-secondary)' }}>{pickString(item, [['accountTypeDesc']], '未命名余额')}</span>
                  <span style={{ color: 'var(--text-main)' }} className="font-medium">
                    {pickString(item, [['balanceOfYuan']], '0')}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </SuperPolishSection>

      <SuperPolishSection title="优惠券与资质" subtitle="优惠券默认查看 BONUS 维度，资质记录支持状态和类目筛选。">
        <div className="grid gap-3 md:grid-cols-3">
          <SuperPolishField label="资质状态">
            <SuperPolishInput value={recordStatus} onChange={(event) => setRecordStatus(event.target.value)} placeholder="留空代表全部" />
          </SuperPolishField>
          <SuperPolishField label="资质类目 ID">
            <SuperPolishInput value={categoryId} onChange={(event) => setCategoryId(event.target.value)} placeholder="留空代表全部" />
          </SuperPolishField>
          <SuperPolishField label="可审核类目数">
            <SuperPolishSelect value={String(categoryRows.length)} disabled>
              <option value={String(categoryRows.length)}>{categoryRows.length}</option>
            </SuperPolishSelect>
          </SuperPolishField>
          <SuperPolishField label="优惠券条数">
            <SuperPolishSelect value={String(couponRows.length)} disabled>
              <option value={String(couponRows.length)}>{couponRows.length}</option>
            </SuperPolishSelect>
          </SuperPolishField>
        </div>

        <div className="mt-4 grid gap-4 xl:grid-cols-3">
          <div className="space-y-2">
            <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>优惠券</div>
            {couponRows.length === 0 ? (
              <div className="rounded-[16px] border px-4 py-6 text-[12px]" style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}>
                当前没有返回可展示的优惠券记录。
              </div>
            ) : couponRows.slice(0, 8).map((item, index) => (
              <div key={`${pickString(item, [['couponId'], ['id']], 'coupon')}-${index}`} className="rounded-[16px] border px-4 py-3 text-[12px]" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}>
                <div className="font-medium" style={{ color: 'var(--text-main)' }}>
                  {pickString(item, [['couponName'], ['title'], ['name']], '未命名优惠券')}
                </div>
                <div className="mt-1" style={{ color: 'var(--text-secondary)' }}>
                  面额 {pickString(item, [['amountOfYuan'], ['amount']], '未返回')} · 来源 {pickString(item, [['sourceDesc'], ['source']], 'BONUS')}
                </div>
              </div>
            ))}
          </div>

          <div className="space-y-2">
            <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>资质类目</div>
            {categoryRows.length === 0 ? (
              <div className="rounded-[16px] border px-4 py-6 text-[12px]" style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}>
                当前没有返回可审核资质类目。
              </div>
            ) : categoryRows.slice(0, 8).map((item, index) => (
              <div key={`${pickString(item, [['cateId']], 'cat')}-${index}`} className="rounded-[16px] border px-4 py-3 text-[12px]" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}>
                {pickString(item, [['name']], '未命名类目')} · ID {pickString(item, [['cateId']], '未返回')}
              </div>
            ))}
          </div>
          <div className="space-y-2">
            <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>资质记录</div>
            {recordRows.length === 0 ? (
              <div className="rounded-[16px] border px-4 py-6 text-[12px]" style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}>
                当前筛选条件下没有返回资质记录。
              </div>
            ) : recordRows.slice(0, 8).map((item, index) => (
              <div key={`${pickString(item, [['recordId'], ['id']], 'record')}-${index}`} className="rounded-[16px] border px-4 py-3 text-[12px]" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}>
                <div className="flex flex-wrap items-center gap-2">
                  <div className="font-medium" style={{ color: 'var(--text-main)' }}>
                    {pickString(item, [['categoryName'], ['name']], '未命名记录')}
                  </div>
                  <SuperPolishBadge tone={campaignStatusMeta(pickString(item, [['statusDesc'], ['status']], '未返回')).tone}>
                    {pickString(item, [['statusDesc'], ['status']], '未返回')}
                  </SuperPolishBadge>
                </div>
                <div className="mt-1" style={{ color: 'var(--text-secondary)' }}>
                  记录 ID {pickString(item, [['recordId'], ['id']], '未返回')}
                </div>
              </div>
            ))}
          </div>
        </div>
      </SuperPolishSection>
    </div>
  )
}

export default SuperPolishAccountPage
