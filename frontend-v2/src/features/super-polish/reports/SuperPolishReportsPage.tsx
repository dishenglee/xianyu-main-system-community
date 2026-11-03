import { useMemo, useState } from 'react'

import {
  getSuperPolishCampaigns,
  unwrapMtopData,
} from '../../../api/superPolish'
import {
  SuperPolishBadge,
  SuperPolishError,
  SuperPolishField,
  SuperPolishInput,
  SuperPolishMetricGrid,
  SuperPolishSection,
  SuperPolishSelect,
} from '../components/SuperPolishUi'
import { useSuperPolishRequest } from '../hooks/useSuperPolishRequest'
import { useSuperPolishScope } from '../hooks/useSuperPolishScope'
import { campaignListFromResponse, daysAgoInput, formatMoney, pickString, todayInput } from '../model'
import { campaignStatusMeta } from '../presentation'
import {
  loadCampaignReportRange,
  type CampaignReportRangeData,
  type CampaignReportRow,
  type CampaignReportSummary,
} from './campaignReportData'
import { buildCampaignCatalogPayload } from './reportCatalog'

function reportStatusLabel(statusCode: number | null) {
  if (statusCode === null) return ''
  if (statusCode === 99) return '擦亮已完成'
  if (statusCode < 0) return '已暂停'
  return '投放中'
}

function summaryMetrics(summary: CampaignReportSummary) {
  if (
    summary.clickPv === 0 &&
    summary.chatPv === 0 &&
    summary.favorites === 0 &&
    summary.cost === 0 &&
    summary.showPv === 0 &&
    summary.inquiries === 0 &&
    summary.payPv === 0
  ) {
    return []
  }

  return [
    { label: '曝光量', value: String(summary.showPv) },
    { label: '点击量', value: String(summary.clickPv) },
    { label: '咨询量', value: String(summary.chatPv) },
    { label: '收藏量', value: String(summary.favorites) },
    { label: '消耗', value: formatMoney(summary.cost), tone: 'info' as const },
  ]
}

interface CampaignRowViewModel extends CampaignReportRow {
  statusText: string
}

function CampaignRows({ rows }: { rows: CampaignRowViewModel[] }) {
  return rows.map((row) => {
    const status = campaignStatusMeta(row.statusText)
    const latestTime = row.detailItems[row.detailItems.length - 1]?.time?.slice(0, 10) ?? ''
    const detailBits = [
      `计划 ID ${row.campaignId}`,
      row.adGroupCount > 0 ? `分组数 ${row.adGroupCount}` : '',
      latestTime ? `最近数据 ${latestTime}` : '',
    ].filter(Boolean)

    return (
      <div
        key={row.campaignId}
        className="rounded-[16px] border px-4 py-3"
        style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
      >
        <div className="flex flex-wrap items-center gap-2">
          <div className="font-medium" style={{ color: 'var(--text-main)' }}>
            {row.title || '未命名计划'}
          </div>
          {row.statusText ? <SuperPolishBadge tone={status.tone}>{status.label}</SuperPolishBadge> : null}
        </div>
        <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
          {detailBits.join(' · ')}
        </div>
        <div className="mt-3 grid gap-2 text-[12px] md:grid-cols-5" style={{ color: 'var(--text-secondary)' }}>
          <div>曝光：{row.showPv}</div>
          <div>点击：{row.clickPv}</div>
          <div>咨询：{row.chatPv}</div>
          <div>收藏：{row.favorites}</div>
          <div>消耗：{formatMoney(row.cost)}</div>
        </div>
      </div>
    )
  })
}

export function SuperPolishReportsPage() {
  const { uid } = useSuperPolishScope()
  const [startDate, setStartDate] = useState(daysAgoInput(365))
  const [endDate, setEndDate] = useState(todayInput())
  const [pageSize, setPageSize] = useState('10')

  const report = useSuperPolishRequest<CampaignReportRangeData>(
    (accountUid) => loadCampaignReportRange(accountUid, { startDate, endDate }),
    [startDate, endDate],
  )
  const campaignCatalog = useSuperPolishRequest(
    (accountUid) => getSuperPolishCampaigns(accountUid, buildCampaignCatalogPayload(startDate, endDate, '50')),
    [startDate, endDate],
  )

  const rows = report.data?.rows ?? []
  const campaignStatusById = useMemo(() => {
    const catalogRows = campaignListFromResponse(unwrapMtopData(campaignCatalog.data ?? {}))
    const map = new Map<string, string>()

    for (const row of catalogRows) {
      const campaignId = pickString(row, [['campaignId']], '')
      if (!campaignId) continue
      const statusText = pickString(row, [['statusDescTitle'], ['statusDesc'], ['status']], '')
      if (!statusText) continue
      map.set(campaignId, statusText)
    }

    return map
  }, [campaignCatalog.data])
  const reportRows = useMemo<CampaignRowViewModel[]>(
    () => rows.map((row) => ({
      ...row,
      statusText: campaignStatusById.get(row.campaignId) ?? reportStatusLabel(row.statusCode),
    })),
    [campaignStatusById, rows],
  )
  const total = reportRows.length
  const pausedCampaignCount = useMemo(
    () => reportRows.filter((row) => /暂停|完成/.test(row.statusText)).length,
    [reportRows],
  )
  const visibleRows = useMemo(() => reportRows.slice(0, Number(pageSize)), [pageSize, reportRows])
  const metrics = useMemo(
    () => summaryMetrics(report.data?.summary ?? {
      clickPv: 0,
      chatPv: 0,
      favorites: 0,
      cost: 0,
      showPv: 0,
      inquiries: 0,
      payPv: 0,
      realCost: 0,
    }),
    [report.data],
  )

  return (
    <div className="space-y-4">
      <SuperPolishError message={report.error} />

      <SuperPolishSection
        title="计划报表"
        subtitle="按时间范围查看计划层级投放结果，长区间会自动拆成多段真实报表请求。"
        actions={(
          <button
            onClick={report.reload}
            disabled={!uid || report.loading}
            className="btn-primary btn-sm"
          >
            刷新报表
          </button>
        )}
      >
        <div className="grid grid-cols-3 gap-2 md:gap-3">
          <SuperPolishField label="开始日期">
            <SuperPolishInput type="date" value={startDate} onChange={(event) => setStartDate(event.target.value)} />
          </SuperPolishField>
          <SuperPolishField label="结束日期">
            <SuperPolishInput type="date" value={endDate} onChange={(event) => setEndDate(event.target.value)} />
          </SuperPolishField>
          <SuperPolishField label="每页条数">
            <SuperPolishSelect value={pageSize} onChange={(event) => setPageSize(event.target.value)}>
              <option value="10">10</option>
              <option value="20">20</option>
              <option value="50">50</option>
            </SuperPolishSelect>
          </SuperPolishField>
        </div>

        {metrics.length > 0 ? (
          <div className="mt-4">
            <div className="mb-2 text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>核心指标</div>
            <SuperPolishMetricGrid items={metrics} />
          </div>
        ) : null}

        <div className="mt-4 space-y-2">
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>计划报表</div>
              <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                共 {total} 条计划记录
              </div>
            </div>
            {pausedCampaignCount > 0 ? (
              <SuperPolishBadge tone="warning">暂停 / 完成 {pausedCampaignCount}</SuperPolishBadge>
            ) : null}
          </div>

          {visibleRows.length > 0 ? <CampaignRows rows={visibleRows} /> : null}

          {visibleRows.length === 0 ? (
            <div
              className="rounded-[16px] border px-4 py-6 text-[12px]"
              style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
            >
              {report.loading
                ? '正在查询计划报表…'
                : report.error
                  ? '当前无法读取报表结果，请先处理上方错误后重试。'
                  : '当前筛选条件下没有返回计划报表数据，可调整日期范围后重试。'}
            </div>
          ) : null}
        </div>
      </SuperPolishSection>
    </div>
  )
}

export default SuperPolishReportsPage
