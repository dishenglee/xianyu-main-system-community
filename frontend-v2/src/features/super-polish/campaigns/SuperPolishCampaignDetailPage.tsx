import { useMemo, useState } from 'react'
import { useLocation, useParams } from 'react-router-dom'
import { useAppDialog } from '../../../components/AppDialog'
import { useSafeNotify } from '../../../components/Notify'
import { editSuperPolishCampaign } from '../../../api/superPolish'
import { asRecord, pickNumberString, pickString, readPath, type JsonRecord } from '../model'
import {
  SuperPolishError,
  SuperPolishField,
  SuperPolishInput,
  SuperPolishKeyValueList,
  SuperPolishMetricGrid,
  SuperPolishSection,
} from '../components/SuperPolishUi'
import { bidTargetLabel, buildBooleanMetric, campaignStatusMeta } from '../presentation'
import { formatCentMoney } from './create/helpers'
import { CampaignDetailCurrentItemsSection } from './detail/CampaignDetailCurrentItemsSection'
import { CampaignDetailItemEditorModal } from './detail/CampaignDetailItemEditorModal'
import { CampaignDetailSettingsEditorModal } from './detail/CampaignDetailSettingsEditorModal'
import { summarizeAreaTargetSupport } from './areaTargetSupport'
import {
  buildCampaignDetailSettingsDraft,
  buildSuperPolishCampaignEditPayload,
  formatCampaignDivisionSelectionSummary,
  formatCampaignPackageSelectionSummary,
  formatCampaignScheduleHoursSummary,
  readCampaignCreativeOptAvailable,
  resolveCampaignDetailEditSettings,
} from './detail/helpers'
import { useSuperPolishCampaignDetail } from './detail/useSuperPolishCampaignDetail'
import type {
  SuperPolishCampaignDetailItem,
  SuperPolishCampaignSettingsDraft,
} from './detail/types'
import { useSuperPolishScope } from '../hooks/useSuperPolishScope'

function promotionModeLabel(value: unknown) {
  switch (String(value ?? '').trim()) {
    case 'custom':
      return '自定义擦亮'
    case 'order':
      return '套餐擦亮'
    default:
      return String(value ?? '').trim() || '未返回'
  }
}

function CampaignDetailWorkspace({
  campaign,
  campaignId,
  initialItems,
  loading,
  uid,
}: {
  campaign: JsonRecord
  campaignId: string
  initialItems: SuperPolishCampaignDetailItem[]
  loading: boolean
  uid: string | null
}) {
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const [currentItems, setCurrentItems] = useState<SuperPolishCampaignDetailItem[]>(initialItems)
  const [editorOpen, setEditorOpen] = useState(false)
  const [settingsEditorOpen, setSettingsEditorOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveResult, setSaveResult] = useState(false)
  const [settingsDraft, setSettingsDraft] = useState<SuperPolishCampaignSettingsDraft>(
    () => buildCampaignDetailSettingsDraft(campaign),
  )

  const status = campaignStatusMeta(
    pickString(campaign, [['statusDescTitle'], ['statusDesc'], ['status']], '未返回'),
  )
  const areaTargetSupport = useMemo(
    () => summarizeAreaTargetSupport(currentItems),
    [currentItems],
  )
  const areaTargetAvailable = areaTargetSupport.available
  const creativeOptAvailable = readCampaignCreativeOptAvailable(campaign)
  const effectiveSettings = useMemo(
    () => resolveCampaignDetailEditSettings(campaign, settingsDraft, areaTargetAvailable),
    [areaTargetAvailable, campaign, settingsDraft],
  )
  const summaryItems = useMemo(() => ([
    { label: '计划名称', value: effectiveSettings.title || '未返回' },
    { label: '计划 ID', value: campaignId || '未返回' },
    { label: '主宝贝 ID', value: pickString(campaign, [['itemId'], ['feedEntityId']], '未返回') },
    { label: '当前宝贝数', value: String(currentItems.length) },
    { label: '当前状态', value: status.label, tone: status.tone },
  ]), [campaign, campaignId, currentItems.length, effectiveSettings.title, status.label, status.tone])
  const settingItems = useMemo(() => {
    const modeItems = effectiveSettings.promotionMode === 'order'
      ? [
          {
            label: '套餐金额',
            value: effectiveSettings.selectedPackageOption
              ? formatCentMoney(effectiveSettings.selectedPackageOption.orderPrice)
              : '待返回',
          },
          { label: '套餐说明', value: formatCampaignPackageSelectionSummary(effectiveSettings.selectedPackageOption) },
        ]
      : [
          { label: '当前出价', value: formatCentMoney(effectiveSettings.bidCent) },
          { label: '日预算', value: formatCentMoney(effectiveSettings.budgetCent) },
        ]

    return [
      { label: '擦亮模式', value: promotionModeLabel(effectiveSettings.promotionMode) },
      { label: '优化目标', value: bidTargetLabel(effectiveSettings.bidTarget) },
      ...modeItems,
      { label: '投放地域', value: formatCampaignDivisionSelectionSummary(effectiveSettings.divisions) },
      { label: '投放时段', value: formatCampaignScheduleHoursSummary(effectiveSettings.hours) },
      {
        label: '创意优化',
        value: effectiveSettings.creativeOptEnabled ? '已开启' : '未开启',
        tone: effectiveSettings.creativeOptEnabled ? 'success' as const : 'neutral' as const,
      },
    ]
  }, [effectiveSettings])
  const saveFeedback = saveResult
    ? [buildBooleanMetric({
        label: '计划维护',
        value: true,
        whenTrue: '已提交',
        whenFalse: '未提交',
        helperWhenTrue: '本次宝贝调整已经提交，请返回列表或稍后刷新确认。',
      })]
    : []

  function mutateCurrentItems(updater: (items: SuperPolishCampaignDetailItem[]) => SuperPolishCampaignDetailItem[]) {
    setCurrentItems((current) => updater(current))
    setSaveResult(false)
  }

  async function handleDelete(item: SuperPolishCampaignDetailItem) {
    const confirmed = await showConfirm(`确认从当前计划里删除「${item.title}」？`, {
      confirmLabel: '确认删除',
      cancelLabel: '取消',
    })
    if (!confirmed) return
    mutateCurrentItems((current) => current.filter((entry) => entry.itemId !== item.itemId))
  }

  async function handleConfirmRemove(item: SuperPolishCampaignDetailItem) {
    return showConfirm(`确认从待提交宝贝池里移除「${item.title}」？`, {
      confirmLabel: '确认移除',
      cancelLabel: '取消',
    })
  }

  async function submitEdit() {
    if (!uid || !campaignId || currentItems.length === 0) return
    const confirmed = await showConfirm(`确认提交计划 ${campaignId} 的宝贝调整？`, {
      confirmLabel: '确认提交',
      cancelLabel: '取消',
    })
    if (!confirmed) return

    setSaving(true)
    try {
      const payload = buildSuperPolishCampaignEditPayload(campaign, currentItems, effectiveSettings)
      await editSuperPolishCampaign(uid, campaignId, payload)
      setSaveResult(true)
      notify('计划修改已提交', 'success')
    } catch (error) {
      notify(error instanceof Error ? error.message : '编辑计划失败', 'error')
    } finally {
      setSaving(false)
    }
  }

  return (
    <>
      <CampaignDetailCurrentItemsSection
        items={currentItems}
        loading={loading}
        onDelete={(item) => void handleDelete(item)}
        onOpenEditor={() => setEditorOpen(true)}
      />

      <SuperPolishSection
        title="计划设置"
        subtitle="这里保留用户真正能看懂、也能直接执行的计划信息和操作，不再暴露原始 JSON 维护入口。"
        actions={(
          <div className="flex flex-wrap items-center gap-2">
            <button type="button" onClick={() => setSettingsEditorOpen(true)} className="btn-secondary btn-sm">
              编辑设置
            </button>
            <button
              onClick={() => void submitEdit()}
              disabled={!uid || !campaignId || currentItems.length === 0 || saving}
              className="btn-primary btn-sm"
            >
              保存计划修改
            </button>
          </div>
        )}
      >
        <div className="grid gap-4 xl:grid-cols-2">
          <div
            className="rounded-[18px] border px-4 py-4 text-[12px]"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
          >
            <div className="font-semibold" style={{ color: 'var(--text-main)' }}>当前计划</div>
            <div className="mt-3">
              <SuperPolishKeyValueList items={summaryItems} />
            </div>
          </div>

          <div
            className="rounded-[18px] border px-4 py-4 text-[12px]"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
          >
            <div className="font-semibold" style={{ color: 'var(--text-main)' }}>投放设置</div>
            <div className="mt-3">
              <SuperPolishKeyValueList items={settingItems} />
            </div>
          </div>
        </div>

        {saveFeedback.length > 0 ? (
          <div className="mt-4">
            <SuperPolishMetricGrid items={saveFeedback} />
          </div>
        ) : null}
      </SuperPolishSection>

      {editorOpen ? (
        <CampaignDetailItemEditorModal
          uid={uid ?? ''}
          open
          selectedItems={currentItems}
          onClose={() => setEditorOpen(false)}
          onConfirm={(items) => {
            setCurrentItems(items)
            setEditorOpen(false)
            setSaveResult(false)
          }}
          onConfirmRemove={handleConfirmRemove}
        />
      ) : null}

      {settingsEditorOpen && uid ? (
        <CampaignDetailSettingsEditorModal
          key={JSON.stringify(settingsDraft)}
          uid={uid}
          open
          items={currentItems}
          initialSettings={settingsDraft}
          areaTargetAvailable={areaTargetAvailable}
          areaTargetSupport={areaTargetSupport}
          creativeOptAvailable={creativeOptAvailable}
          onClose={() => setSettingsEditorOpen(false)}
          onSave={(nextSettings) => {
            setSettingsDraft(nextSettings)
            setSettingsEditorOpen(false)
            setSaveResult(false)
          }}
        />
      ) : null}
    </>
  )
}

export function SuperPolishCampaignDetailPage() {
  const { uid } = useSuperPolishScope()
  const { campaignId = '' } = useParams()
  const location = useLocation()
  const seedCampaign = asRecord((location.state as { campaign?: JsonRecord } | null)?.campaign)
  const detail = useSuperPolishCampaignDetail({
    campaignId,
    seedCampaign,
  })

  const campaign = detail.campaign
  const status = campaignStatusMeta(
    pickString(campaign, [['statusDescTitle'], ['statusDesc'], ['status']], '未返回'),
  )
  const campaignMetrics = [
    { label: '计划状态', value: status.label, tone: status.tone },
    {
      label: '消耗',
      value: formatCentMoney(
        Number(
          readPath(campaign, ['adDataDetailVO', 'cost'])
          ?? readPath(campaign, ['adData', 'cost'])
          ?? 0,
        ),
      ),
    },
    {
      label: '点击',
      value: pickNumberString(campaign, [['adDataDetailVO', 'clickPv'], ['adData', 'clickPv']], '0'),
    },
    {
      label: '咨询',
      value: pickNumberString(campaign, [['adDataDetailVO', 'chatPv'], ['adData', 'chatPv']], '0'),
    },
  ]

  return (
    <div className="space-y-4">
      <SuperPolishError message={detail.error} />

      <SuperPolishSection
        title={`计划 ${campaignId || '未返回 ID'}`}
        subtitle="先核对计划概况和当前宝贝，再进入编辑宝贝完成添加、移除和删除。"
        actions={(
          <button onClick={detail.reloadAll} className="btn-ghost btn-sm">
            刷新详情
          </button>
        )}
      >
        <div className="grid gap-3 md:grid-cols-3">
          <SuperPolishField label="开始日期">
            <SuperPolishInput type="date" value={detail.startDate} onChange={(event) => detail.setStartDate(event.target.value)} />
          </SuperPolishField>
          <SuperPolishField label="结束日期">
            <SuperPolishInput type="date" value={detail.endDate} onChange={(event) => detail.setEndDate(event.target.value)} />
          </SuperPolishField>
        </div>

        <div className="mt-4">
          <SuperPolishMetricGrid items={campaignMetrics} />
        </div>
      </SuperPolishSection>

      <CampaignDetailWorkspace
        key={detail.initialItemsKey}
        campaign={campaign}
        campaignId={campaignId}
        initialItems={detail.initialItems}
        loading={detail.loading}
        uid={uid}
      />
    </div>
  )
}

export default SuperPolishCampaignDetailPage
