import { useMemo, useState } from 'react'
import { Modal } from '../../../../components/Modal'
import {
  SuperPolishBadge,
  SuperPolishField,
  SuperPolishInput,
} from '../../components/SuperPolishUi'
import { bidTargetLabel } from '../../presentation'
import { CreatePlanAdvancedSettingsDialog } from '../create/CreatePlanAdvancedSettingsDialog'
import { CreatePlanPackagePickerDialog } from '../create/CreatePlanPackagePickerDialog'
import { AreaTargetSupportNotice } from '../components/AreaTargetSupportNotice'
import type { AreaTargetSupportSummary } from '../areaTargetSupport'
import {
  ALL_HOURS,
  formatFenInput,
  formatCentMoney,
  formatDivisionSummary,
  type CustomBidMode,
  type PromotionMode,
} from '../create/helpers'
import {
  formatCampaignPackageSelectionSummary,
  snapshotCampaignPackageSelection,
} from './helpers'
import { useCampaignDetailStrategyOptions } from './useCampaignDetailStrategyOptions'
import type {
  SuperPolishCampaignDetailItem,
  SuperPolishCampaignSettingsDraft,
} from './types'

type AdvancedTab = 'division' | 'schedule'

function formatHourSummary(hours: number[]) {
  return hours.length === ALL_HOURS.length ? '全部时段' : `${hours.length}个时段`
}

function SegmentButton({
  active,
  children,
  onClick,
}: {
  active: boolean
  children: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded-[14px] border px-3 py-2 text-[12px] font-medium transition"
      style={{
        borderColor: active ? 'var(--primary)' : 'var(--control-border)',
        background: active ? 'var(--primary-light)' : 'var(--surface-soft)',
        color: active ? 'var(--text-main)' : 'var(--text-secondary)',
      }}
    >
      {children}
    </button>
  )
}

function ToggleRow({
  label,
  helper,
  checked,
  disabled,
  onChange,
}: {
  label: string
  helper: string
  checked: boolean
  disabled?: boolean
  onChange: (value: boolean) => void
}) {
  return (
    <label
      className="flex items-start justify-between gap-4 rounded-[16px] border px-4 py-3"
      style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
    >
      <div className="min-w-0">
        <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
          {label}
        </div>
        <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
          {helper}
        </div>
      </div>
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
    </label>
  )
}

function deriveCustomBidMode({
  bidInput,
  recommendedBidCent,
}: {
  bidInput: string
  recommendedBidCent: number | null | undefined
}): CustomBidMode {
  const currentBidText = bidInput.trim()
  const recommendedBidText = formatFenInput(recommendedBidCent ?? null)
  if (currentBidText && recommendedBidText && currentBidText === recommendedBidText) {
    return 'recommend'
  }
  return 'manual'
}

export function CampaignDetailSettingsEditorModal({
  uid,
  open,
  items,
  initialSettings,
  areaTargetAvailable,
  areaTargetSupport,
  creativeOptAvailable,
  onClose,
  onSave,
}: {
  uid: string
  open: boolean
  items: SuperPolishCampaignDetailItem[]
  initialSettings: SuperPolishCampaignSettingsDraft
  areaTargetAvailable: boolean
  areaTargetSupport: AreaTargetSupportSummary
  creativeOptAvailable: boolean
  onClose: () => void
  onSave: (value: SuperPolishCampaignSettingsDraft) => void
}) {
  const [draft, setDraft] = useState<SuperPolishCampaignSettingsDraft>(initialSettings)
  const [advancedTab, setAdvancedTab] = useState<AdvancedTab | null>(null)
  const [packageDialogOpen, setPackageDialogOpen] = useState(false)
  const [customBidModeByTarget, setCustomBidModeByTarget] = useState<Record<string, CustomBidMode>>({})
  const [manualBidInputByTarget, setManualBidInputByTarget] = useState<Record<string, string>>({
    [initialSettings.bidTarget]: initialSettings.bidInput,
  })

  const effectiveDivisions = useMemo(
    () => (areaTargetAvailable ? draft.divisions : []),
    [areaTargetAvailable, draft.divisions],
  )
  const strategy = useCampaignDetailStrategyOptions({
    items,
    draft,
    areaTargetEnabled: areaTargetAvailable && effectiveDivisions.length > 0,
  })
  const effectiveBidTarget = strategy.effectiveBidTarget || draft.bidTarget
  const effectivePromotionMode: PromotionMode = draft.promotionMode || 'custom'
  const activePackageSelection = strategy.selectedPackageSelection ?? draft.selectedPackageByBidTarget[effectiveBidTarget] ?? null
  const effectiveCustomBidMode = customBidModeByTarget[effectiveBidTarget]
    ?? deriveCustomBidMode({
      bidInput: manualBidInputByTarget[effectiveBidTarget] ?? draft.bidInput,
      recommendedBidCent: strategy.activeEstimate?.bidPrice,
    })
  const defaultManualBidInput = formatFenInput(strategy.activeEstimate?.customBidPrice ?? strategy.activeEstimate?.bidPrice)
  const effectiveManualBidInput = (
    manualBidInputByTarget[effectiveBidTarget]?.trim()
      ? manualBidInputByTarget[effectiveBidTarget]
      : draft.bidInput.trim()
        ? draft.bidInput
        : defaultManualBidInput
  )
  const divisionSummary = useMemo(
    () => formatDivisionSummary(effectiveDivisions),
    [effectiveDivisions],
  )
  const timeSummary = useMemo(
    () => formatHourSummary(draft.hours),
    [draft.hours],
  )
  const recommendedBudgetChips = useMemo(
    () => strategy.activeEstimate?.recommendBudgetList ?? [],
    [strategy.activeEstimate],
  )
  const effectiveCreativeOptAvailable = Boolean(
    creativeOptAvailable
      || strategy.activeEstimate?.creativeOptimizationAvailable
      || activePackageSelection?.creativeOptimizationAvailable,
  )
  const hasNeedRecharge = strategy.accountInfoData.needReCharge === true
  const usableBalance = Number(strategy.accountInfoData.usableBalance ?? strategy.accountInfoData.cashBalance ?? 0)
  const recommendedRecharge = Number((strategy.accountInfoData.rechargeAmountList as unknown[] | undefined)?.[0] ?? 0)
  const canSave = Boolean(
    draft.title.trim()
    && (
      effectivePromotionMode === 'order'
        ? activePackageSelection
        : (
          (effectiveCustomBidMode === 'recommend' || Number(effectiveManualBidInput) > 0)
          && Number(draft.budgetInput) > 0
        )
    ),
  )

  function handlePromotionModeChange(mode: PromotionMode) {
    setDraft((current) => ({ ...current, promotionMode: mode }))
  }

  function handleSave() {
    const nextPackageByBidTarget = { ...draft.selectedPackageByBidTarget }
    const nextPackageOrderPriceByBidTarget = { ...draft.selectedPackageOrderPriceByBidTarget }
    if (activePackageSelection) {
      nextPackageByBidTarget[effectiveBidTarget] = activePackageSelection
      nextPackageOrderPriceByBidTarget[effectiveBidTarget] = activePackageSelection.orderPrice
    }

    const nextBidInput = effectivePromotionMode === 'custom' && effectiveCustomBidMode === 'recommend'
      ? formatFenInput(strategy.activeEstimate?.bidPrice ?? null) || draft.bidInput
      : effectiveManualBidInput

    onSave({
      ...draft,
      bidTarget: effectiveBidTarget,
      bidInput: nextBidInput,
      divisions: effectiveDivisions,
      selectedPackageByBidTarget: nextPackageByBidTarget,
      selectedPackageOrderPriceByBidTarget: nextPackageOrderPriceByBidTarget,
    })
  }

  return (
    <>
      <Modal
        open={open}
        onClose={onClose}
        title="编辑计划设置"
        width={860}
        footer={(
          <>
            <button type="button" onClick={onClose} className="btn-ghost btn-sm">
              取消
            </button>
            <button
              type="button"
              onClick={handleSave}
              disabled={!canSave}
              className="btn-primary btn-sm"
            >
              保存设置
            </button>
          </>
        )}
      >
        <div className="space-y-4">
          <div
            className="rounded-[18px] border px-4 py-3 text-[12px] leading-5"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
          >
            这里按真实计划页顺序整理目标、计划名称、擦亮方式和高级设置。保存后还需要回到详情页点击“保存计划修改”，才会正式提交到超级擦亮。
          </div>

          <div
            className="rounded-[18px] border px-4 py-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
          >
            <div className="mb-2 text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
              优化目标
            </div>
            <div className="flex flex-wrap gap-2">
              {strategy.bidTargets.length > 0 ? strategy.bidTargets.map((bidTarget) => (
                <SegmentButton
                  key={bidTarget}
                  active={effectiveBidTarget === bidTarget}
                  onClick={() => setDraft((current) => ({ ...current, bidTarget }))}
                >
                  {bidTargetLabel(bidTarget)}
                </SegmentButton>
              )) : (
                <SuperPolishBadge tone="neutral">{bidTargetLabel(effectiveBidTarget)}</SuperPolishBadge>
              )}
            </div>
          </div>

          <SuperPolishField label="计划名称">
            <SuperPolishInput
              value={draft.title}
              onChange={(event) => setDraft((current) => ({ ...current, title: event.target.value }))}
              placeholder="填写计划名称"
            />
          </SuperPolishField>

          <div
            className="rounded-[18px] border px-4 py-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
          >
            <div className="mb-2 text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
              擦亮方式
            </div>
            <div className="flex flex-wrap gap-2">
              <SegmentButton active={effectivePromotionMode === 'order'} onClick={() => handlePromotionModeChange('order')}>
                套餐擦亮
              </SegmentButton>
              <SegmentButton active={effectivePromotionMode === 'custom'} onClick={() => handlePromotionModeChange('custom')}>
                自定义擦亮
              </SegmentButton>
            </div>

            {effectivePromotionMode === 'order' ? (
              <div
                className="mt-4 rounded-[18px] border px-4 py-4"
                style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                        {activePackageSelection ? formatCentMoney(activePackageSelection.orderPrice) : '待返回'}
                      </div>
                      <SuperPolishBadge tone="info">套餐擦亮</SuperPolishBadge>
                      {effectiveCreativeOptAvailable ? <SuperPolishBadge tone="success">支持创意优化</SuperPolishBadge> : null}
                    </div>
                    <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                      {formatCampaignPackageSelectionSummary(activePackageSelection)}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => setPackageDialogOpen(true)}
                    disabled={(strategy.activePackageGroup?.options.length ?? 0) === 0}
                    className="btn-ghost btn-sm disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    编辑套餐
                  </button>
                </div>
              </div>
            ) : (
              <div
                className="mt-4 space-y-4 rounded-[18px] border p-4"
                style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
              >
                <div className="space-y-3">
                  <div className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
                    出价设置
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <SegmentButton
                      active={effectiveCustomBidMode === 'recommend'}
                      onClick={() => setCustomBidModeByTarget((current) => ({ ...current, [effectiveBidTarget]: 'recommend' }))}
                    >
                      系统推荐
                    </SegmentButton>
                    <SegmentButton
                      active={effectiveCustomBidMode === 'manual'}
                      onClick={() => {
                        setCustomBidModeByTarget((current) => ({ ...current, [effectiveBidTarget]: 'manual' }))
                        setManualBidInputByTarget((current) => ({
                          ...current,
                          [effectiveBidTarget]: current[effectiveBidTarget] || defaultManualBidInput || draft.bidInput,
                        }))
                      }}
                    >
                      自定义
                    </SegmentButton>
                  </div>
                  {effectiveCustomBidMode === 'recommend' ? (
                    <div
                      className="rounded-[14px] border px-3 py-3 text-[12px]"
                      style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
                    >
                      优先控制投放成本，稳定成本下尽可能拿量
                    </div>
                  ) : (
                    <SuperPolishField label="自定义出价">
                      <SuperPolishInput
                        value={effectiveManualBidInput}
                        onChange={(event) => {
                          setManualBidInputByTarget((current) => ({
                            ...current,
                            [effectiveBidTarget]: event.target.value,
                          }))
                        }}
                        placeholder="输入元金额"
                      />
                    </SuperPolishField>
                  )}
                </div>

                <div className="space-y-3">
                  <div className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
                    日预算
                  </div>
                  {recommendedBudgetChips.length > 0 ? (
                    <div className="flex flex-wrap gap-2">
                      {recommendedBudgetChips.map((budget) => {
                        const chipValue = String(budget / 100)
                        const selected = draft.budgetInput === chipValue
                        return (
                          <button
                            key={budget}
                            type="button"
                            onClick={() => setDraft((current) => ({ ...current, budgetInput: chipValue }))}
                            className="rounded-[12px] border px-3 py-1.5 text-[12px]"
                            style={{
                              borderColor: selected ? 'var(--primary)' : 'var(--control-border)',
                              background: selected ? 'var(--primary-light)' : 'var(--surface-card)',
                              color: 'var(--text-main)',
                            }}
                          >
                            {formatCentMoney(budget)}
                          </button>
                        )
                      })}
                    </div>
                  ) : null}
                  <SuperPolishField label="日预算">
                    <SuperPolishInput
                      value={draft.budgetInput}
                      onChange={(event) => setDraft((current) => ({ ...current, budgetInput: event.target.value }))}
                      placeholder="输入元金额"
                    />
                  </SuperPolishField>
                </div>
              </div>
            )}

            {strategy.activeEstimate ? (
              <div className="mt-3 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                当前 {bidTargetLabel(strategy.activeEstimate.bidTarget)} 推荐：平台推荐 {formatCentMoney(strategy.activeEstimate.bidPrice)}，
                自定义上限 {formatCentMoney(strategy.activeEstimate.customBidPrice)}。
              </div>
            ) : null}
          </div>

          <div
            className="rounded-[18px] border px-4 py-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
          >
            <div className="flex flex-wrap items-center gap-2">
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                账户状态
              </div>
              {hasNeedRecharge ? <SuperPolishBadge tone="warning">建议充值</SuperPolishBadge> : null}
            </div>
            <div className="mt-2 grid gap-3 md:grid-cols-2">
              <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                可用余额：<span style={{ color: 'var(--text-main)' }}>{formatCentMoney(usableBalance)}</span>
              </div>
              <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                推荐充值：<span style={{ color: 'var(--text-main)' }}>{formatCentMoney(recommendedRecharge)}</span>
              </div>
            </div>
          </div>

          <div
            className="rounded-[18px] border px-4 py-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
          >
            <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
              高级设置
            </div>
            <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
              地域和时段继续沿用超级擦亮的弹层编辑方式，不把这些字段摊成一堆原始开关。
            </div>

            <div className="mt-4 flex flex-wrap gap-2">
              <button
                type="button"
                onClick={() => setAdvancedTab('division')}
                disabled={!areaTargetAvailable}
                className="rounded-[12px] border px-3 py-2 text-[12px] font-medium disabled:cursor-not-allowed disabled:opacity-60"
                style={{
                  borderColor: 'var(--control-border)',
                  background: 'var(--surface-soft)',
                  color: 'var(--text-main)',
                }}
              >
                {divisionSummary}
              </button>
              <button
                type="button"
                onClick={() => setAdvancedTab('schedule')}
                className="rounded-[12px] border px-3 py-2 text-[12px] font-medium"
                style={{
                  borderColor: 'var(--control-border)',
                  background: 'var(--surface-soft)',
                  color: 'var(--text-main)',
                }}
              >
                {timeSummary}
              </button>
            </div>

            {!areaTargetAvailable ? (
              <div className="mt-3">
                <AreaTargetSupportNotice
                  summary={areaTargetSupport}
                  subject="当前计划"
                />
              </div>
            ) : !areaTargetSupport.fullySupported ? (
              <div className="mt-3">
                <AreaTargetSupportNotice
                  summary={areaTargetSupport}
                  subject="当前计划"
                />
              </div>
            ) : null}

            <div className="mt-4">
              <ToggleRow
                label="创意优化"
                helper={effectiveCreativeOptAvailable ? '开启后按平台能力自动优化创意拿量。' : '当前计划未开放创意优化开关。'}
                checked={draft.creativeOptEnabled}
                disabled={!effectiveCreativeOptAvailable}
                onChange={(value) => setDraft((current) => ({ ...current, creativeOptEnabled: value }))}
              />
            </div>
          </div>

          {!canSave ? (
            <div className="text-[12px] leading-5" style={{ color: 'var(--status-warning-text)' }}>
              请填写有效的计划名称，并补齐当前擦亮方式需要的设置后再保存。
            </div>
          ) : null}
        </div>
      </Modal>

      {advancedTab ? (
        <CreatePlanAdvancedSettingsDialog
          uid={uid}
          open
          initialTab={advancedTab}
          areaTargetAvailable={areaTargetAvailable}
          areaTargetSupport={areaTargetSupport}
          creativeOptAvailable={effectiveCreativeOptAvailable}
          initialDivisions={effectiveDivisions}
          initialHours={draft.hours}
          initialCreativeOptEnabled={draft.creativeOptEnabled}
          onClose={() => setAdvancedTab(null)}
          onSave={(value) => {
            setDraft((current) => ({
              ...current,
              divisions: value.divisions,
              hours: value.hours,
            }))
            setAdvancedTab(null)
          }}
        />
      ) : null}

      {packageDialogOpen ? (
        <CreatePlanPackagePickerDialog
          open
          packageGroup={strategy.activePackageGroup}
          selectedPackageOption={strategy.selectedPackageOption}
          onClose={() => setPackageDialogOpen(false)}
          onConfirm={(index) => {
            const selectedOption = strategy.activePackageGroup?.options[index]
            if (!selectedOption) return
            setDraft((current) => ({
              ...current,
              selectedPackageOrderPriceByBidTarget: {
                ...current.selectedPackageOrderPriceByBidTarget,
                [effectiveBidTarget]: selectedOption.orderPrice,
              },
              selectedPackageByBidTarget: {
                ...current.selectedPackageByBidTarget,
                [effectiveBidTarget]: snapshotCampaignPackageSelection(selectedOption) ?? current.selectedPackageByBidTarget[effectiveBidTarget],
              },
            }))
            setPackageDialogOpen(false)
          }}
        />
      ) : null}
    </>
  )
}
