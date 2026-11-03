// @ts-nocheck
// Tauri WKWebView 不支持原生 window.confirm/prompt/alert（直接忽略并返回
// null/false），所以这里用 React state 做内嵌 modal 接管所有对话框。
import { useState } from 'react'
import { createPortal } from 'react-dom'
import { X, Edit3, TrendingDown } from 'lucide-react'
import { formatMoneyDisplay, normalizePriceText } from './itemUtils'
import { dialogMessageTextStyle } from '../../components/dialogTypography'

// ---------- DialogModal ----------
// 替代 window.confirm/prompt/alert（Tauri WKWebView 直接忽略原生对话框）。
export function DialogModal({ dialog }) {
  const [value, setValue] = useState(dialog.defaultValue || '')
  const displayTitle = dialog.title || (
    dialog.kind === 'confirm'
      ? '请确认'
      : dialog.kind === 'prompt'
        ? '请输入'
        : '操作提示'
  )
  const ok = () => {
    if (dialog.kind === 'prompt') dialog.resolve(value)
    else if (dialog.kind === 'confirm') dialog.resolve(true)
    else dialog.resolve(null)
  }
  const cancel = () => {
    if (dialog.kind === 'confirm') dialog.resolve(false)
    else dialog.resolve(null)
  }
  return createPortal((
    <div
      className="liquid-scrim fixed inset-0 z-[120] flex items-center justify-center p-4"
      onClick={cancel}
    >
      <div
        className="liquid-panel w-full max-w-[400px] rounded-[24px] overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="px-5 pt-4 pb-2">
          <div className="flex items-center justify-between mb-1.5">
            <div className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>{displayTitle}</div>
          </div>
          <div
            className="font-semibold whitespace-pre-wrap text-center"
            style={{ color: 'var(--text-main)', ...dialogMessageTextStyle(dialog.message) }}
          >
            {dialog.message}
          </div>
          {dialog.kind === 'prompt' ? (
            <input
              autoFocus
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder={dialog.placeholder || ''}
              onKeyDown={(e) => { if (e.key === 'Enter') ok(); if (e.key === 'Escape') cancel() }}
              className="input mt-3 text-[13px]"
            />
          ) : null}
          {dialog.kind === 'choice' ? (
            <div className="mt-3 grid grid-cols-1 gap-2">
              {(dialog.options || []).map((option) => (
                <button
                  key={option.value}
                  type="button"
                  disabled={option.disabled}
                  onClick={() => dialog.resolve(option.value)}
                  className="liquid-muted-card text-left rounded-[14px] px-3 py-2 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>{option.label}</div>
                  {option.description ? (
                    <div className="text-[11px] mt-0.5" style={{ color: 'var(--text-muted)' }}>{option.description}</div>
                  ) : null}
                </button>
              ))}
            </div>
          ) : null}
        </div>
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t" style={{ borderColor: 'var(--control-border)', background: 'rgba(255,255,255,0.42)' }}>
          {dialog.kind !== 'alert' ? (
            <button onClick={cancel} className="btn-ghost btn-sm cursor-pointer">取消</button>
          ) : null}
          {dialog.kind !== 'choice' ? (
            <button
              onClick={ok}
              className={dialog.danger ? 'btn-danger btn-sm cursor-pointer' : 'btn-primary btn-sm cursor-pointer'}
            >
              {dialog.confirmLabel || '确认'}
            </button>
          ) : null}
        </div>
      </div>
    </div>
  ), document.body)
}

// ---------- GrouponSheetModal ----------
export function GrouponSheetModal({ sheet, onClose, onSelect, onCustomPriceChange, onSubmit }) {
  const selected = sheet.selectedPrice || sheet.recommendedPrice || sheet.options[0]?.value || ''
  const customPrice = normalizePriceText(sheet.customPrice)
  const mode = sheet.mode === 'discount' ? 'discount' : 'amount'
  const editingCustom = selected === '__custom__'
  const closeSelected = selected === '__close__'
  const selectedOption = sheet.options.find((option) => option.value === selected)
  const fallbackDisplayPrice = sheet.recommendedPrice || sheet.selectedPrice || sheet.options.find((option) => option.value !== '__close__')?.label?.replace(/^¥/, '') || '--'
  const displayPriceRaw = closeSelected
    ? fallbackDisplayPrice
    : (
      (editingCustom ? customPrice : '')
      || sheet.selectedPrice
      || (selectedOption?.label || '').replace(/^¥/, '')
      || fallbackDisplayPrice
    )
  const displayPrice = formatMoneyDisplay(displayPriceRaw)
  const displayHelperText = closeSelected
    ? '当前将关闭 2人小刀'
    : (sheet.inlineError || sheet.helperText || '暂未设置优惠，系统自动推荐优惠档位')
  const options = [...sheet.options]
  if (!options.some((option) => option.value === '__close__')) {
    options.push({ value: '__close__', label: sheet.closeOptionLabel || '不开', description: '保留当前状态' })
  }
  const submitDisabled = sheet.loading || (mode !== 'discount' && editingCustom && !customPrice)
  const itemPriceText = formatMoneyDisplay(sheet.itemPrice)
  const summaryLabel = closeSelected ? '当前选择' : '最多可刀'
  const summaryValue = closeSelected ? (sheet.closeOptionLabel || '不开') : `¥${displayPrice}`
  return createPortal((
    <div
      className="liquid-scrim fixed inset-0 z-[120] flex items-end justify-center p-0 md:items-center md:p-4"
      onClick={onClose}
    >
      <div
        className="liquid-panel w-full max-w-[420px] overflow-hidden flex flex-col rounded-t-[28px] md:rounded-[24px]"
        style={{ maxHeight: 'calc(100dvh - env(safe-area-inset-top) - 8px)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="px-5 pt-4 pb-4 border-b"
          style={{ borderColor: 'var(--control-border)' }}
        >
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>{sheet.title}</div>
              <div className="mt-1 text-[12px]" style={{ color: 'var(--text-muted)' }}>
                {summaryLabel} · {summaryValue}
              </div>
            </div>
            <button
              type="button"
              className="w-9 h-9 rounded-full flex items-center justify-center cursor-pointer"
              style={{ background: 'var(--surface-control)', color: 'var(--text-main)', border: '1px solid var(--control-border)' }}
              onClick={onClose}
              aria-label="关闭"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
          <div
            className="mt-4 rounded-[18px] px-4 py-4"
            style={{
              background: 'var(--surface-soft)',
              border: '1px solid var(--glass-border)',
            }}
          >
            <div className="text-[12px] font-medium" style={{ color: 'var(--text-muted)' }}>
              {summaryLabel}
            </div>
            <div className="flex items-end gap-2">
              <div className="text-[28px] font-semibold leading-none" style={{ color: 'var(--text-main)' }}>
                {summaryValue}
              </div>
              {!closeSelected ? (
                <button
                  type="button"
                  onClick={() => {
                    onSelect('__custom__')
                    if (!sheet.customPrice) onCustomPriceChange(displayPrice)
                  }}
                  className="inline-flex items-center justify-center w-7 h-7 rounded-full cursor-pointer"
                  style={{ background: 'var(--primary-light)', color: 'var(--primary)', border: '1px solid color-mix(in srgb, var(--primary) 20%, transparent)' }}
                  aria-label="编辑小刀金额"
                >
                  <Edit3 className="w-3.5 h-3.5" />
                </button>
              ) : null}
            </div>
            <div className="mt-2 text-[12px] leading-5" style={{ color: sheet.inlineError ? 'var(--danger)' : 'var(--text-secondary)' }}>
              {displayHelperText}
            </div>
            <div className="mt-3 flex items-center gap-2 flex-wrap text-[12px]" style={{ color: 'var(--text-muted)' }}>
              <span>售价 ¥{itemPriceText}</span>
              {sheet.currentlyOpen ? (
                <span
                  className="inline-flex items-center rounded-full px-2 py-0.5"
                  style={{ background: 'var(--status-success-bg)', color: 'var(--status-success-text)' }}
                >
                  已开启
                </span>
              ) : null}
            </div>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4 space-y-3">
          <div className="space-y-2">
            {options.map((option) => {
              const active = option.value === selected
              const isClose = option.value === '__close__'
              const label = isClose
                ? (sheet.closeOptionLabel || '不开')
                : (mode === 'discount'
                  ? (option.label || option.value)
                  : `¥${formatMoneyDisplay((option.label || '').replace(/^¥/, '') || option.value)}`)
              const description = option.description || (
                isClose
                  ? '保持关闭或关闭当前小刀'
                  : (mode === 'discount' ? '按折后价参与 2人小刀' : '固定小刀金额')
              )
              return (
                <button
                  key={option.value}
                  type="button"
                  disabled={sheet.loading || option.disabled}
                  onClick={() => onSelect(option.value)}
                  className="w-full text-left rounded-[16px] px-4 py-3 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                  style={{
                    background: active ? 'var(--primary-light)' : 'var(--surface-soft)',
                    border: active ? '2px solid var(--primary)' : '1px solid var(--control-border)',
                  }}
                >
                  <div className="flex items-start gap-3">
                    <div className="min-w-0 flex-1">
                      <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
                        {label}
                      </div>
                      <div className="text-[12px] mt-1 leading-5" style={{ color: 'var(--text-secondary)' }}>
                        {option.disabled ? '当前不可用' : description}
                      </div>
                    </div>
                    {active ? (
                      <div className="w-5 h-5 rounded-full flex items-center justify-center shrink-0" style={{ background: 'var(--primary)' }}>
                        <div className="w-2 h-2 rounded-full" style={{ background: 'var(--liquid-highlight)' }} />
                      </div>
                    ) : null}
                  </div>
                </button>
              )
            })}
          </div>

          {editingCustom && mode !== 'discount' ? (
            <div className="mt-4">
              <div className="text-[13px] mb-2" style={{ color: 'var(--text-secondary)' }}>输入开启金额</div>
              <input
                type="text"
                inputMode="decimal"
                value={sheet.customPrice}
                onChange={(e) => onCustomPriceChange(e.target.value)}
                placeholder="例如 9.90"
                className="input w-full rounded-[16px] px-4 py-3 text-[16px]"
              />
            </div>
          ) : null}
        </div>

        <div
          className="shrink-0 flex items-center justify-end gap-2 px-4 pt-3 border-t"
          style={{
            borderColor: 'var(--control-border)',
            background: 'rgba(255,255,255,0.72)',
            paddingBottom: 'calc(14px + env(safe-area-inset-bottom))',
          }}
        >
          <button onClick={onClose} className="btn-ghost btn-sm cursor-pointer" disabled={sheet.loading}>取消</button>
          <button
            type="button"
            onClick={onSubmit}
            disabled={submitDisabled}
            className="btn-primary btn-sm cursor-pointer disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {sheet.loading ? '提交中...' : (sheet.buttonText || '确定')}
          </button>
        </div>
      </div>
    </div>
  ), document.body)
}
