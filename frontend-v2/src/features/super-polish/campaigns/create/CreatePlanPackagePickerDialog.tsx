import { useState } from 'react'
import { Modal } from '../../../../components/Modal'
import { SuperPolishBadge } from '../../components/SuperPolishUi'
import {
  formatCentMoney,
  packageOptionDescription,
  type SuperPolishPackageGroup,
  type SuperPolishPackageOption,
} from './helpers'

export function CreatePlanPackagePickerDialog({
  open,
  packageGroup,
  selectedPackageOption,
  onClose,
  onConfirm,
}: {
  open: boolean
  packageGroup: SuperPolishPackageGroup | null
  selectedPackageOption: SuperPolishPackageOption | null
  onClose: () => void
  onConfirm: (index: number) => void
}) {
  const [draftOrderPrice, setDraftOrderPrice] = useState<number | null>(selectedPackageOption?.orderPrice ?? null)

  const options = packageGroup?.options ?? []
  const activeIndex = options.findIndex((option) => option.orderPrice === draftOrderPrice)

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="选择套餐"
      width={760}
      footer={(
        <>
          <button type="button" onClick={onClose} className="btn-secondary btn-sm">
            取消
          </button>
          <button
            type="button"
            onClick={() => {
              if (activeIndex < 0) return
              onConfirm(activeIndex)
            }}
            className="btn-primary btn-sm"
            disabled={activeIndex < 0}
          >
            确定
          </button>
        </>
      )}
    >
      <div className="space-y-3">
        {options.length === 0 ? (
          <div
            className="rounded-[18px] border px-4 py-5 text-[13px]"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
          >
            当前目标没有返回可选套餐。
          </div>
        ) : options.map((option, index) => {
          const selected = option.orderPrice === draftOrderPrice
          return (
            <button
              key={`${packageGroup?.bidTarget ?? 'package'}-${option.orderPrice ?? index}`}
              type="button"
              onClick={() => setDraftOrderPrice(option.orderPrice)}
              className="flex w-full items-center justify-between gap-4 rounded-[16px] border px-4 py-3 text-left transition"
              style={{
                borderColor: selected ? 'var(--primary)' : 'var(--control-border)',
                background: selected ? 'var(--primary-light)' : 'var(--surface-card)',
              }}
            >
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                    {formatCentMoney(option.orderPrice)}
                  </div>
                  {option.isRecommended ? <SuperPolishBadge tone="success">推荐档</SuperPolishBadge> : null}
                </div>
                <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                  {packageOptionDescription(option)}
                </div>
              </div>
              {selected ? <SuperPolishBadge tone="info">已选</SuperPolishBadge> : null}
            </button>
          )
        })}
      </div>
    </Modal>
  )
}
