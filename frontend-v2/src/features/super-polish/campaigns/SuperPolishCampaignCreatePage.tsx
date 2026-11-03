import { Sparkles } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAppDialog } from '../../../components/AppDialog'
import { useSafeNotify } from '../../../components/Notify'
import { SuperPolishError } from '../components/SuperPolishUi'
import { useSuperPolishScope } from '../hooks/useSuperPolishScope'
import { CreatePlanAdvancedSettingsDialog } from './create/CreatePlanAdvancedSettingsDialog'
import { CreatePlanConfigSection } from './create/CreatePlanConfigSection'
import { CreatePlanItemPickerModal } from './create/CreatePlanItemPickerModal'
import { CreatePlanSelectedItemsSection } from './create/CreatePlanSelectedItemsSection'
import { useSuperPolishCreatePlan } from './create/useSuperPolishCreatePlan'

type AdvancedEntry = 'division' | 'schedule'

export function SuperPolishCampaignCreatePage() {
  const { uid, buildPath } = useSuperPolishScope()

  if (!uid) {
    return <SuperPolishError message="当前没有可用账号，无法创建超级擦亮计划。" />
  }

  return <SuperPolishCampaignCreateWorkspace key={uid} uid={uid} buildPath={buildPath} />
}

function SuperPolishCampaignCreateWorkspace({
  uid,
  buildPath,
}: {
  uid: string
  buildPath: (path: string) => string
}) {
  const navigate = useNavigate()
  const notify = useSafeNotify()
  const dialog = useAppDialog()
  const plan = useSuperPolishCreatePlan({
    uid,
    notify,
    dialog,
    onCreateSuccess: ({ campaignIds, needRecharge, title: createdTitle }) => {
      if (!needRecharge) return
      const next = new URLSearchParams()
      next.set('uid', uid)
      next.set('balance', '0')
      next.set('planName', createdTitle)
      if (campaignIds[0]) {
        next.set('createdCampaignId', campaignIds[0])
      }
      navigate({
        pathname: '/super-polish/campaigns',
        search: `?${next.toString()}`,
      }, { replace: true })
    },
  })
  const [pickerOpen, setPickerOpen] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [advancedEntry, setAdvancedEntry] = useState<AdvancedEntry>('division')

  const pageError = [
    plan.precheck.error,
    plan.recommend.error,
    plan.packages.error,
    plan.estimates.error,
    plan.accountInfo.error,
  ].find(Boolean) ?? ''

  return (
    <div className="space-y-4">
      <SuperPolishError message={pageError} />

      <div
        className="flex items-start gap-3 rounded-[18px] border px-4 py-3"
        style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
      >
        <Sparkles className="mt-0.5 h-4 w-4 shrink-0" style={{ color: 'var(--primary)' }} />
        <div className="text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
          {plan.precheckNotice}
        </div>
      </div>

      <CreatePlanSelectedItemsSection
        items={plan.selectedItems}
        onOpenPicker={() => setPickerOpen(true)}
        onClear={() => { void plan.clearSelected() }}
        onRemoveItem={plan.removeSelectedItem}
      />

      <CreatePlanConfigSection
        bidTargets={plan.bidTargets}
        selectedBidTarget={plan.selectedBidTarget}
        onSelectBidTarget={plan.setSelectedBidTarget}
        promotionMode={plan.promotionMode}
        onChangePromotionMode={plan.changePromotionMode}
        title={plan.title}
        onTitleChange={plan.setTitle}
        activePackageGroup={plan.activePackageGroup}
        selectedPackageOption={plan.selectedPackageOption}
        onChoosePackageOption={plan.choosePackageOption}
        activeEstimate={plan.activeEstimate}
        customBidMode={plan.customBidMode}
        onChangeCustomBidMode={plan.changeCustomBidMode}
        customBidInput={plan.customBidInput}
        onCustomBidInputChange={plan.updateCustomBidInput}
        budgetInput={plan.budgetInput}
        onBudgetInputChange={plan.updateBudgetInput}
        creativeOptAvailable={plan.creativeOptAvailable}
        creativeOptEnabled={plan.creativeOptEnabled}
        onCreativeOptChange={plan.updateCreativeOptEnabled}
        areaTargetAvailable={plan.areaTargetAvailable}
        areaTargetSupport={plan.areaTargetSupport}
        areaTargetForcesCustom={plan.areaTargetForcesCustom}
        divisionSummary={plan.divisionSummary}
        timeSummary={plan.timeSummary}
        onOpenDivisionAdvanced={() => {
          setAdvancedEntry('division')
          setAdvancedOpen(true)
        }}
        onOpenScheduleAdvanced={() => {
          setAdvancedEntry('schedule')
          setAdvancedOpen(true)
        }}
        accountInfoData={plan.accountInfoData}
        summaryLines={plan.summaryLines}
        disabled={plan.submitDisabled}
        submitting={plan.submitBusy}
        onSubmit={() => { void plan.submitCreate() }}
        createMetrics={plan.createMetrics}
        createdCampaignId={plan.createdCampaignId}
        buildPath={buildPath}
      />

      {pickerOpen ? (
        <CreatePlanItemPickerModal
          uid={uid}
          open
          selectedIds={plan.selectedIds}
          selectedCount={plan.selectedItems.length}
          onClose={() => setPickerOpen(false)}
          onToggleItem={plan.toggleItem}
          onAddItems={plan.addItems}
          onClearSelected={() => { void plan.clearSelected() }}
          onRememberItems={plan.rememberItems}
        />
      ) : null}

      {advancedOpen ? (
        <CreatePlanAdvancedSettingsDialog
          uid={uid}
          open
          initialTab={advancedEntry}
          areaTargetAvailable={plan.areaTargetAvailable}
          areaTargetSupport={plan.areaTargetSupport}
          creativeOptAvailable={plan.creativeOptAvailable}
          initialDivisions={plan.selectedDivisions}
          initialHours={plan.selectedHours}
          initialCreativeOptEnabled={plan.creativeOptEnabled}
          onClose={() => setAdvancedOpen(false)}
          onSave={(value) => {
            plan.applyAdvancedSettings(value)
            setAdvancedOpen(false)
          }}
        />
      ) : null}
    </div>
  )
}

export default SuperPolishCampaignCreatePage
