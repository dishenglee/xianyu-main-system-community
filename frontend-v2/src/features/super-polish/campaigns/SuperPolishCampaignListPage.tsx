import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { useSearchParams } from 'react-router-dom'
import { getSuperPolishCampaigns, unwrapMtopData } from '../../../api/superPolish'
import { useSuperPolishRequest } from '../hooks/useSuperPolishRequest'
import { useSuperPolishScope } from '../hooks/useSuperPolishScope'
import { campaignListFromResponse, compactDate, formatMoney, pickString } from '../model'
import {
  SuperPolishBadge,
  SuperPolishError,
  SuperPolishField,
  SuperPolishInput,
  SuperPolishSection,
  SuperPolishSelect,
  SuperPolishMetricGrid,
} from '../components/SuperPolishUi'
import { campaignStatusMeta, sumNumber } from '../presentation'
import { campaignMetricNumber, campaignMetricText, campaignMetricValuePaths, campaignStatusText } from './campaignMetrics'
import { builtCampaignDefaultWindow, normalizeLegacyCampaignListWindow } from './campaignDateWindow'

const DEFAULT_STATUSES = [-6, -5, -4, -3, -2, 1, 2, 3, 4, 6, 99]
const COMPLETED_STATUS_GROUP = 'COMPLETED'
const COMPLETED_CAMPAIGN_STATUS = 99

export function SuperPolishCampaignListPage() {
  const { buildPath } = useSuperPolishScope()
  const [searchParams] = useSearchParams()
  const defaultWindow = useMemo(() => builtCampaignDefaultWindow(), [])
  const createdCampaignId = searchParams.get('createdCampaignId')?.trim() ?? ''
  const createdPlanName = searchParams.get('planName')?.trim() ?? ''
  const lowBalanceCreated = searchParams.get('balance') === '0' && Boolean(createdCampaignId || createdPlanName)
  const [draftStartDate, setDraftStartDate] = useState(defaultWindow.startDate)
  const [draftEndDate, setDraftEndDate] = useState(defaultWindow.endDate)
  const [pageSize, setPageSize] = useState('10')
  const [statusGroup, setStatusGroup] = useState('')
  const [dateWindowTouched, setDateWindowTouched] = useState(false)
  const dateWindow = useMemo(
    () => (
      dateWindowTouched
        ? { startDate: draftStartDate, endDate: draftEndDate }
        : normalizeLegacyCampaignListWindow({ startDate: draftStartDate, endDate: draftEndDate })
    ),
    [dateWindowTouched, draftEndDate, draftStartDate],
  )
  const startDate = dateWindow.startDate
  const endDate = dateWindow.endDate

  const payload = useMemo(() => {
    const completedSelected = statusGroup === COMPLETED_STATUS_GROUP
    return {
      pageNum: 1,
      pageSize: Number(pageSize),
      startDate: compactDate(startDate),
      endDate: compactDate(endDate),
      includeEndDate: true,
      statusGroup: statusGroup && !completedSelected ? [statusGroup] : [],
      campaignStatus: completedSelected
        ? [COMPLETED_CAMPAIGN_STATUS]
        : statusGroup
          ? undefined
          : DEFAULT_STATUSES,
    }
  }, [endDate, pageSize, startDate, statusGroup])

  const payloadKey = JSON.stringify(payload)
  const campaigns = useSuperPolishRequest(
    (uid) => getSuperPolishCampaigns(uid, payload),
    [payloadKey],
  )

  const focusedCampaignPayload = useMemo(() => {
    const numericId = Number(createdCampaignId)
    if (!Number.isFinite(numericId) || numericId <= 0) return null
    return {
      pageNum: 1,
      pageSize: 10,
      startDate: compactDate(startDate),
      endDate: compactDate(endDate),
      includeEndDate: true,
      campaignIds: [numericId],
    }
  }, [createdCampaignId, endDate, startDate])
  const focusedCampaignPayloadKey = JSON.stringify(focusedCampaignPayload ?? {})
  const focusedCampaign = useSuperPolishRequest(
    (uid) => getSuperPolishCampaigns(uid, focusedCampaignPayload ?? {}),
    [focusedCampaignPayloadKey, 'focused-campaign'],
    Boolean(focusedCampaignPayload),
  )

  const campaignRows = useMemo(() => {
    const rows = campaignListFromResponse(unwrapMtopData(campaigns.data ?? {}))
    const focusedRows = campaignListFromResponse(unwrapMtopData(focusedCampaign.data ?? {}))
    if (focusedRows.length === 0) return rows
    const merged = new Map<string, Record<string, unknown>>()
    for (const row of focusedRows) {
      const id = pickString(row, [['campaignId'], ['id']], '')
      merged.set(id || JSON.stringify(row), row)
    }
    for (const row of rows) {
      const id = pickString(row, [['campaignId'], ['id']], '')
      const key = id || JSON.stringify(row)
      if (!merged.has(key)) {
        merged.set(key, row)
      }
    }
    return Array.from(merged.values())
  }, [campaigns.data, focusedCampaign.data])
  const requestError = campaigns.error || focusedCampaign.error
  const requestLoading = campaigns.loading || focusedCampaign.loading
  const activeCount = useMemo(
    () => campaignRows.filter((row) => /投放|进行|生效/.test(campaignStatusText(row, ''))).length,
    [campaignRows],
  )
  const pausedCount = useMemo(
    () => campaignRows.filter((row) => /暂停|不足|冻结/.test(campaignStatusText(row, ''))).length,
    [campaignRows],
  )
  const totalCost = useMemo(
    () => sumNumber(campaignRows, campaignMetricValuePaths('cost')),
    [campaignRows],
  )
  const summaryMetrics = useMemo(() => {
    return [
      { label: '投放中', value: String(activeCount), tone: 'success' as const },
      { label: '暂停中', value: String(pausedCount), tone: pausedCount > 0 ? 'warning' as const : 'neutral' as const },
      { label: '累计消耗', value: formatMoney(totalCost), tone: 'info' as const },
    ]
  }, [activeCount, pausedCount, totalCost])
  const showSummaryMetrics = campaignRows.length > 0 || !requestError

  return (
    <div className="space-y-4">
      <SuperPolishError message={requestError} />

      {lowBalanceCreated ? (
        <div
          className="rounded-[18px] border px-4 py-3 text-[13px] leading-6"
          style={{
            borderColor: 'color-mix(in srgb, var(--status-warning-text) 30%, var(--control-border))',
            background: 'color-mix(in srgb, var(--status-warning-bg) 75%, white)',
            color: 'var(--status-warning-text)',
          }}
        >
          <div className="font-semibold">余额不足，计划已创建但暂未生效。</div>
          <div className="mt-1">
            {createdPlanName ? `计划名称：${createdPlanName}` : '请先完成充值，再回来刷新余额和投放状态。'}
            {createdCampaignId ? ` · 计划 ID：${createdCampaignId}` : ''}
          </div>
        </div>
      ) : null}

      <SuperPolishSection
        title="已建计划"
        subtitle="按日期和状态组查看已创建的超级擦亮计划，并直接进入计划详情与维护页面。"
        actions={(
          <button
            onClick={() => {
              campaigns.reload()
              focusedCampaign.reload()
            }}
            className="btn-ghost btn-sm"
          >
            刷新列表
          </button>
        )}
      >
        <div className="grid grid-cols-3 gap-2 md:gap-3">
          <SuperPolishField label="开始日期">
            <SuperPolishInput
              type="date"
              value={startDate}
              onChange={(event) => {
                setDateWindowTouched(true)
                setDraftStartDate(event.target.value)
              }}
            />
          </SuperPolishField>
          <SuperPolishField label="结束日期">
            <SuperPolishInput
              type="date"
              value={endDate}
              onChange={(event) => {
                setDateWindowTouched(true)
                setDraftEndDate(event.target.value)
              }}
            />
          </SuperPolishField>
          <SuperPolishField label="每页条数">
            <SuperPolishSelect value={pageSize} onChange={(event) => setPageSize(event.target.value)}>
              <option value="10">10</option>
              <option value="20">20</option>
              <option value="50">50</option>
            </SuperPolishSelect>
          </SuperPolishField>
        </div>

        <div className="mt-3 grid gap-3 md:grid-cols-3">
          <SuperPolishField label="状态组">
            <SuperPolishSelect value={statusGroup} onChange={(event) => setStatusGroup(event.target.value)}>
              <option value="">全部</option>
              <option value="PAUSED">暂停中</option>
              <option value="PAUSED_ALL">全部暂停</option>
              <option value="BALANCE_PAUSED">余额不足暂停</option>
              <option value={COMPLETED_STATUS_GROUP}>已完成</option>
            </SuperPolishSelect>
          </SuperPolishField>
        </div>

        {showSummaryMetrics ? (
          <div className="mt-4">
            <SuperPolishMetricGrid items={summaryMetrics} mobileColumns={3} />
          </div>
        ) : null}

        <div className="mt-4 space-y-3">
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>计划列表</div>
              <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                共 {campaignRows.length} 个计划
              </div>
            </div>
          </div>
          {campaignRows.length === 0 ? (
            <div className="rounded-[18px] border px-4 py-8 text-center text-[13px]" style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}>
              {requestLoading
                ? '正在加载计划列表…'
                : requestError
                  ? '当前无法读取计划列表，请先处理上方错误后重试。'
                  : '当前筛选条件下没有返回计划。'}
            </div>
          ) : campaignRows.map((row, index) => {
            const campaignId = pickString(row, [['campaignId'], ['id']], '')
            const status = campaignStatusMeta(campaignStatusText(row))
            const isCreatedCampaign = Boolean(createdCampaignId) && campaignId === createdCampaignId
            return (
              <div
                key={campaignId || `campaign-${index}`}
                className="rounded-[18px] border px-4 py-4"
                style={{
                  borderColor: isCreatedCampaign ? 'var(--primary)' : 'var(--control-border)',
                  background: isCreatedCampaign ? 'var(--primary-light)' : 'var(--surface-soft)',
                }}
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                        {pickString(row, [['itemTitle'], ['title']], '未命名计划')}
                      </div>
                      <SuperPolishBadge tone={status.tone}>{status.label}</SuperPolishBadge>
                    </div>
                    <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      计划 ID {campaignId || '未返回'} · 商品 ID {pickString(row, [['itemId']], '未返回')}
                    </div>
                  </div>
                  {campaignId ? (
                    <Link to={buildPath(`/super-polish/campaigns/${campaignId}`)} state={{ campaign: row }} className="btn-secondary btn-sm">
                      查看分组 / 编辑
                    </Link>
                  ) : null}
                </div>

                <div className="mt-3 grid grid-cols-4 gap-1.5 text-[11px] md:gap-2 md:text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  <div className="min-w-0 truncate">消耗：{formatMoney(campaignMetricNumber(row, 'cost'))}</div>
                  <div className="min-w-0 truncate">点击：{campaignMetricText(row, 'clickPv')}</div>
                  <div className="min-w-0 truncate">咨询：{campaignMetricText(row, 'chatPv')}</div>
                  <div className="min-w-0 truncate">收藏：{campaignMetricText(row, 'favorites')}</div>
                </div>
              </div>
            )
          })}
        </div>
      </SuperPolishSection>
    </div>
  )
}

export default SuperPolishCampaignListPage
