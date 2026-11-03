import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { ChevronDown, ChevronUp } from 'lucide-react'
import {
  SuperPolishBadge,
  SuperPolishField,
  SuperPolishInput,
  SuperPolishMetricGrid,
  SuperPolishSection,
} from '../../components/SuperPolishUi'
import { AreaTargetSupportNotice } from '../components/AreaTargetSupportNotice'
import type { AreaTargetSupportSummary } from '../areaTargetSupport'
import {
  bidTargetLabel,
  formatCentMoney,
  type CustomBidMode,
  type PromotionMode,
  type SuperPolishBidEstimate,
  type SuperPolishPackageGroup,
  type SuperPolishPackageOption,
} from './helpers'
import { CreatePlanPackagePickerDialog } from './CreatePlanPackagePickerDialog'

function SegmentButton({
  active,
  disabled,
  children,
  onClick,
}: {
  active: boolean
  disabled?: boolean
  children: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="rounded-[14px] border px-3 py-2 text-[12px] font-medium transition disabled:cursor-not-allowed disabled:opacity-60"
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

function SummaryChip({
  label,
  disabled,
  onClick,
}: {
  label: string
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="rounded-[12px] border px-3 py-2 text-[12px] font-medium transition disabled:cursor-not-allowed disabled:opacity-60"
      style={{
        borderColor: 'var(--control-border)',
        background: 'var(--surface-card)',
        color: 'var(--text-main)',
      }}
    >
      {label}
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
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        aria-label={label}
      />
    </label>
  )
}

export function CreatePlanConfigSection({
  bidTargets,
  selectedBidTarget,
  onSelectBidTarget,
  promotionMode,
  onChangePromotionMode,
  title,
  onTitleChange,
  activePackageGroup,
  selectedPackageOption,
  onChoosePackageOption,
  activeEstimate,
  customBidMode,
  onChangeCustomBidMode,
  customBidInput,
  onCustomBidInputChange,
  budgetInput,
  onBudgetInputChange,
  creativeOptAvailable,
  creativeOptEnabled,
  onCreativeOptChange,
  areaTargetAvailable,
  areaTargetSupport,
  areaTargetForcesCustom,
  divisionSummary,
  timeSummary,
  onOpenDivisionAdvanced,
  onOpenScheduleAdvanced,
  accountInfoData,
  summaryLines,
  disabled,
  submitting,
  onSubmit,
  createMetrics,
  createdCampaignId,
  buildPath,
}: {
  bidTargets: string[]
  selectedBidTarget: string
  onSelectBidTarget: (value: string) => void
  promotionMode: PromotionMode
  onChangePromotionMode: (mode: PromotionMode) => void
  title: string
  onTitleChange: (value: string) => void
  activePackageGroup: SuperPolishPackageGroup | null
  selectedPackageOption: SuperPolishPackageOption | null
  onChoosePackageOption: (index: number) => void
  activeEstimate: SuperPolishBidEstimate | null
  customBidMode: CustomBidMode
  onChangeCustomBidMode: (mode: CustomBidMode) => void
  customBidInput: string
  onCustomBidInputChange: (value: string) => void
  budgetInput: string
  onBudgetInputChange: (value: string) => void
  creativeOptAvailable: boolean
  creativeOptEnabled: boolean
  onCreativeOptChange: (value: boolean) => void
  areaTargetAvailable: boolean
  areaTargetSupport: AreaTargetSupportSummary
  areaTargetForcesCustom: boolean
  divisionSummary: string
  timeSummary: string
  onOpenDivisionAdvanced: () => void
  onOpenScheduleAdvanced: () => void
  accountInfoData: Record<string, unknown>
  summaryLines: string[]
  disabled: boolean
  submitting: boolean
  onSubmit: () => void
  createMetrics: Array<{ label: string; value: string; helper?: string; tone?: 'neutral' | 'info' | 'success' | 'warning' | 'danger' }>
  createdCampaignId: string
  buildPath: (path: string) => string
}) {
  const [advancedExpanded, setAdvancedExpanded] = useState(false)
  const [packageDialogOpen, setPackageDialogOpen] = useState(false)

  const recommendedBudgetChips = useMemo(
    () => activeEstimate?.recommendBudgetList ?? [],
    [activeEstimate],
  )
  const hasNeedRecharge = accountInfoData.needReCharge === true
  const usableBalance = Number(accountInfoData.usableBalance ?? accountInfoData.cashBalance ?? 0)
  const recommendedRecharge = Number((accountInfoData.rechargeAmountList as unknown[] | undefined)?.[0] ?? 0)

  return (
    <SuperPolishSection
      title="计划设置"
      subtitle="按真实建计划顺序完成目标、计划名、擦亮方式，再补地域、时段和创意优化。"
    >
      <div className="space-y-5">
        <div>
          <div className="mb-2 text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
            推广目标
          </div>
          <div className="flex flex-wrap gap-2">
            {bidTargets.map((bidTarget) => (
              <SegmentButton
                key={bidTarget}
                active={selectedBidTarget === bidTarget}
                onClick={() => onSelectBidTarget(bidTarget)}
              >
                {bidTargetLabel(bidTarget)}
              </SegmentButton>
            ))}
          </div>
        </div>

        <SuperPolishField label="计划名称">
          <SuperPolishInput value={title} onChange={(event) => onTitleChange(event.target.value)} placeholder="填写计划名称" />
        </SuperPolishField>

        <div>
          <div className="mb-2 text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
            擦亮方式
          </div>
          <div className="flex flex-wrap gap-2">
            <SegmentButton
              active={promotionMode === 'order'}
              disabled={areaTargetForcesCustom}
              onClick={() => onChangePromotionMode('order')}
            >
              套餐擦亮
            </SegmentButton>
            <SegmentButton active={promotionMode === 'custom'} onClick={() => onChangePromotionMode('custom')}>
              自定义擦亮
            </SegmentButton>
          </div>
          {areaTargetForcesCustom ? (
            <div className="mt-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              已设置投放地域，当前计划将按自定义擦亮提交。
            </div>
          ) : null}
        </div>

        {promotionMode === 'order' ? (
          <div
            className="rounded-[18px] border p-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
          >
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                    {selectedPackageOption ? formatCentMoney(selectedPackageOption.orderPrice) : '待返回'}
                  </div>
                  <SuperPolishBadge tone="info">套餐擦亮</SuperPolishBadge>
                  {creativeOptAvailable ? <SuperPolishBadge tone="success">支持创意优化</SuperPolishBadge> : null}
                </div>
                <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                  {selectedPackageOption
                    ? `${selectedPackageOption.orderStartDuration}-${selectedPackageOption.orderEndDuration} 天 · ${selectedPackageOption.promotionText || '按平台推荐套餐投放'}`
                    : '正在等待平台返回套餐推荐。'}
                </div>
              </div>
              <button type="button" onClick={() => setPackageDialogOpen(true)} className="btn-ghost btn-sm">
                编辑套餐
              </button>
            </div>
          </div>
        ) : (
          <div
            className="space-y-4 rounded-[18px] border p-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
          >
            <div className="space-y-3">
              <div className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
                出价设置
              </div>
              <div className="flex flex-wrap gap-2">
                <SegmentButton active={customBidMode === 'recommend'} onClick={() => onChangeCustomBidMode('recommend')}>
                  系统推荐
                </SegmentButton>
                <SegmentButton active={customBidMode === 'manual'} onClick={() => onChangeCustomBidMode('manual')}>
                  自定义
                </SegmentButton>
              </div>
              {customBidMode === 'recommend' ? (
                <div
                  className="rounded-[14px] border px-3 py-3 text-[12px]"
                  style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
                >
                  优先控制投放成本，稳定成本下尽可能拿量
                </div>
              ) : (
                <SuperPolishField label="自定义出价">
                  <SuperPolishInput
                    value={customBidInput}
                    onChange={(event) => onCustomBidInputChange(event.target.value)}
                    placeholder="自定义金额"
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
                    const selected = budgetInput === chipValue
                    return (
                      <button
                        key={budget}
                        type="button"
                        onClick={() => onBudgetInputChange(chipValue)}
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
              <SuperPolishField label="自定义预算">
                <SuperPolishInput
                  value={budgetInput}
                  onChange={(event) => onBudgetInputChange(event.target.value)}
                  placeholder="自定义金额"
                />
              </SuperPolishField>
              {activeEstimate ? (
                <div className="text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                  当前 {bidTargetLabel(activeEstimate.bidTarget)} 推荐：平台推荐 {formatCentMoney(activeEstimate.bidPrice)}，
                  自定义上限 {formatCentMoney(activeEstimate.customBidPrice)}。
                </div>
              ) : null}
            </div>
          </div>
        )}

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
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                高级设置
              </div>
              <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                高级设置会影响计划拿量效果，如非必须建议使用系统默认设置。
              </div>
            </div>
            <button type="button" onClick={() => setAdvancedExpanded((value) => !value)} className="btn-ghost btn-sm">
              {advancedExpanded ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
              {advancedExpanded ? '收起' : '展开'}
            </button>
          </div>

          {advancedExpanded ? (
            <div className="mt-4 space-y-3 border-t pt-4" style={{ borderColor: 'var(--control-border)' }}>
              <div className="flex flex-wrap gap-2">
                <SummaryChip label={divisionSummary} disabled={!areaTargetAvailable} onClick={onOpenDivisionAdvanced} />
                <SummaryChip label={timeSummary} onClick={onOpenScheduleAdvanced} />
              </div>

              {!areaTargetSupport.fullySupported ? (
                <AreaTargetSupportNotice
                  summary={areaTargetSupport}
                  subject="当前所选宝贝"
                />
              ) : null}

              <ToggleRow
                label="创意优化"
                helper={creativeOptAvailable ? '开启后按平台能力自动优化创意拿量。' : '当前账号暂未开放创意优化。'}
                checked={creativeOptEnabled}
                disabled={!creativeOptAvailable}
                onChange={onCreativeOptChange}
              />
            </div>
          ) : null}
        </div>

        <div
          className="rounded-[18px] border px-4 py-4"
          style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
        >
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                立即擦亮
              </div>
              <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                确认当前宝贝池、目标、套餐或出价以及高级设置后，再发起正式计划。
              </div>
            </div>
            <button type="button" onClick={onSubmit} disabled={disabled || submitting} className="btn-primary btn-sm">
              {submitting ? '提交中...' : '立即擦亮'}
            </button>
          </div>

          <div className="mt-4 space-y-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
            {summaryLines.map((line) => (
              <div key={line}>{line}</div>
            ))}
          </div>

          {createMetrics.length > 0 ? (
            <div className="mt-4 space-y-3">
              <SuperPolishMetricGrid items={createMetrics} />
              {createdCampaignId ? (
                <Link to={buildPath(`/super-polish/campaigns/${createdCampaignId}`)} className="btn-secondary btn-sm inline-flex">
                  查看新建计划
                </Link>
              ) : null}
            </div>
          ) : null}
        </div>
      </div>

      {packageDialogOpen ? (
        <CreatePlanPackagePickerDialog
          open
          packageGroup={activePackageGroup}
          selectedPackageOption={selectedPackageOption}
          onClose={() => setPackageDialogOpen(false)}
          onConfirm={(index) => {
            onChoosePackageOption(index)
            setPackageDialogOpen(false)
          }}
        />
      ) : null}
    </SuperPolishSection>
  )
}
