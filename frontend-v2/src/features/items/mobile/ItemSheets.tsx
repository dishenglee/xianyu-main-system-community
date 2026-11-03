// @ts-nocheck
import { useEffect, useState } from 'react'
import {
  TrendingDown, Coins, Sparkles, Edit3, Share2, Trash2,
  ArrowUpFromLine, ArrowDownToLine, ShieldCheck, Loader2,
} from 'lucide-react'
import { BottomSheet } from '../../../components/Modal'
import { SensitiveValue, useSensitiveText } from '../../../components/SensitiveValue'
import {
  reducePrice,
  itemCoinDeductionOpen, itemCoinDeductionClose, coinDeductionRatio,
  itemGrouponStatus, itemGrouponGuide, itemGrouponOpen, itemGrouponClose,
  publishCheck, publishBadwords,
  itemDelete, itemDownshelf, itemUpshelf, itemShare,
  isSuccess, retMsg,
} from '../../../api/index'
import {
  coinDisplayRatio, coinRatioOptionsFrom, normalizeCoinRatio,
  buildGrouponOptions, grouponDisplayPrice, grouponPriceValidation,
  resolveGrouponItemPrice, normalizeGrouponSubmitPrice, formatMoneyDisplay,
  coinCanClose, coinCloseRemainingText, markdownAmountValidation,
} from '../itemUtils'
import { useAppDialog } from '../../../components/AppDialog'

// ============================================================
// 单项操作主菜单
// ============================================================

interface ItemActionSheetProps {
  open: boolean
  item: any
  onClose: () => void
  onReload: () => void
  onOpenPriceReduce: () => void
  onOpenCoinDeduction: () => void
  onOpenKnife: () => void
  onOpenPublishCheck: () => void
  onOpenEdit: () => void
  notify: (message: string, type?: 'success' | 'error' | 'info', duration?: number) => void
}

export function ItemActionSheet({
  open, item, onClose, onReload,
  onOpenPriceReduce, onOpenCoinDeduction, onOpenKnife, onOpenPublishCheck, onOpenEdit,
  notify,
}: ItemActionSheetProps) {
  const { showConfirm } = useAppDialog()
  const sheetTitle = useSensitiveText('item-title', item?.title || '商品操作')
  if (!item) return null
  const isSoldOut = item._listType === 'soldOut' || item.itemStatus === 0

  async function handleShelfToggle() {
    onClose()
    try {
      if (isSoldOut) {
        const r = await itemUpshelf(item._uid, item.itemId || item.id, item)
        notify(isSuccess(r) ? '已上架' : retMsg(r, '上架失败'), isSuccess(r) ? 'success' : 'error')
      } else {
        const r = await itemDownshelf(item._uid, item.itemId || item.id)
        notify(isSuccess(r) ? '已下架' : retMsg(r, '下架失败'), isSuccess(r) ? 'success' : 'error')
      }
      onReload()
    } catch (e) {
      notify(String(e?.message || e), 'error')
    }
  }

  async function handleDelete() {
    const confirmed = await showConfirm('确认删除商品？该操作不可恢复', {
      title: '删除商品',
      confirmLabel: '删除',
      danger: true,
    })
    if (!confirmed) return
    onClose()
    try {
      const r = await itemDelete(item._uid, item.itemId || item.id)
      notify(isSuccess(r) ? '已删除' : retMsg(r, '删除失败'), isSuccess(r) ? 'success' : 'error')
      onReload()
    } catch (e) {
      notify(String(e?.message || e), 'error')
    }
  }

  async function handleShare() {
    onClose()
    try {
      const r = await itemShare(item._uid, item.itemId || item.id)
      const shareUrl = r?.data?.shareUrl || r?.data?.url || ''
      if (shareUrl) {
        if (navigator.share) {
          await navigator.share({ title: item.title, url: shareUrl })
        } else {
          await navigator.clipboard.writeText(shareUrl)
          notify('链接已复制', 'success')
        }
      } else {
        notify(retMsg(r, '获取分享链接失败'), 'error')
      }
    } catch (e) {
      notify(String(e?.message || e), 'error')
    }
  }

  const actions = [
    { icon: isSoldOut ? ArrowUpFromLine : ArrowDownToLine, label: isSoldOut ? '上架' : '下架', color: isSoldOut ? 'var(--success)' : 'var(--warning)', onClick: handleShelfToggle },
    { icon: TrendingDown, label: '调价', color: 'var(--primary)', onClick: () => { onClose(); onOpenPriceReduce() } },
    { icon: Coins, label: '币抵扣', color: 'var(--warning)', onClick: () => { onClose(); onOpenCoinDeduction() } },
    { icon: Sparkles, label: '小刀', color: 'var(--primary)', onClick: () => { onClose(); onOpenKnife() } },
    { icon: ShieldCheck, label: '政策检查', color: 'var(--success)', onClick: () => { onClose(); onOpenPublishCheck() } },
    { icon: Edit3, label: '编辑', color: 'var(--text-muted)', onClick: () => { onClose(); onOpenEdit() } },
    { icon: Share2, label: '分享', color: 'var(--primary)', onClick: handleShare },
    { icon: Trash2, label: '删除', color: 'var(--danger)', onClick: handleDelete },
  ]

  return (
    <BottomSheet open={open} onClose={onClose} title={sheetTitle}>
      <div className="grid grid-cols-4 gap-2">
        {actions.map((a) => (
          <button
            key={a.label}
            onClick={a.onClick}
            className="flex flex-col items-center justify-center gap-1.5 py-3 rounded-xl active:bg-[var(--surface-soft)]/60 cursor-pointer transition-colors"
          >
            <div
              className="w-10 h-10 rounded-full flex items-center justify-center"
              style={{ backgroundColor: `color-mix(in srgb, ${a.color} 12%, transparent)` }}
            >
              <a.icon className="w-5 h-5" style={{ color: a.color }} />
            </div>
            <span className="text-[12px]" style={{ color: 'var(--text-main)' }}>{a.label}</span>
          </button>
        ))}
      </div>
    </BottomSheet>
  )
}

// ============================================================
// 调价 BottomSheet
// ============================================================

interface PriceReduceSheetProps {
  open: boolean
  item: any
  onClose: () => void
  onReload: () => void
  notify: (message: string, type?: 'success' | 'error' | 'info', duration?: number) => void
}

export function PriceReduceSheet({ open, item, onClose, onReload, notify }: PriceReduceSheetProps) {
  const [amount, setAmount] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => { if (open) setAmount('') }, [open])

  async function handleSubmit() {
    const n = Number(amount)
    const validation = markdownAmountValidation(String(n), item?.price, item?.grouponOpen ? item?.grouponPrice : '')
    if (!validation.ok) {
      notify(validation.message, 'error')
      return
    }
    setBusy(true)
    try {
      const r = await reducePrice(item._uid, item.itemId || item.id, n)
      if (isSuccess(r) && !(r?.data && typeof r.data === 'object' && r.data.result === false)) {
        notify(`已降价 ¥${n}`, 'success')
        onClose()
        onReload()
      } else {
        notify(retMsg(r, '降价失败'), 'error')
      }
    } catch (e) {
      notify(String(e?.message || e), 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <BottomSheet
      open={open}
      onClose={onClose}
      title="降价"
      footer={
        <>
          <button className="btn-secondary flex-1" onClick={onClose} disabled={busy}>取消</button>
          <button className="btn-primary flex-1" onClick={handleSubmit} disabled={busy || !amount}>
            {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : '确认降价'}
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="text-[13px]" style={{ color: 'var(--text-secondary)' }}>
          当前售价 <span className="font-semibold" style={{ color: 'var(--primary)' }}>¥<SensitiveValue kind="money" value={item?.price || '—'} /></span>
        </div>
        <div>
          <label className="label">降价金额（元）</label>
          <input
            type="number"
            className="input"
            placeholder="例如 5"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            min="0"
            step="0.01"
            inputMode="decimal"
          />
          <p className="text-[12px] mt-2" style={{ color: 'var(--text-muted)' }}>
            建议每次降价 1-10 元；过大降价可能引发系统限制
          </p>
        </div>
      </div>
    </BottomSheet>
  )
}

// ============================================================
// 币抵扣 BottomSheet
// ============================================================

interface CoinDeductionSheetProps {
  open: boolean
  item: any
  onClose: () => void
  onReload: () => void
  notify: (message: string, type?: 'success' | 'error' | 'info', duration?: number) => void
}

export function CoinDeductionSheet({ open, item, onClose, onReload, notify }: CoinDeductionSheetProps) {
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [options, setOptions] = useState<any[]>([])
  const [selected, setSelected] = useState('')
  const currentRatio = coinDisplayRatio(item)
  const isOpen = !!currentRatio
  const canClose = coinCanClose(item)
  const closeHint = coinCloseRemainingText(item)

  useEffect(() => {
    if (!open || !item) return
    let cancelled = false
    setLoading(true)
    setOptions([])
    setSelected(currentRatio || '')
    coinDeductionRatio(item._uid, item.price || '100', item.itemId || item.id)
      .then((r) => {
        if (cancelled) return
        const opts = coinRatioOptionsFrom(r, item)
        setOptions(opts.length ? opts : [{ value: '5', label: '5%', description: '抵扣 5%', disabled: false }, { value: '10', label: '10%', description: '抵扣 10%', disabled: false }, { value: '20', label: '20%', description: '抵扣 20%', disabled: false }])
      })
      .catch(() => setOptions([{ value: '5', label: '5%', description: '抵扣 5%', disabled: false }, { value: '10', label: '10%', description: '抵扣 10%', disabled: false }, { value: '20', label: '20%', description: '抵扣 20%', disabled: false }]))
      .finally(() => !cancelled && setLoading(false))
    return () => { cancelled = true }
  }, [open, item])

  async function handleOpen() {
    const ratio = normalizeCoinRatio(selected)
    if (!ratio) {
      notify('请选择抵扣比例', 'error')
      return
    }
    setBusy(true)
    try {
      const r = await itemCoinDeductionOpen(item._uid, item.itemId || item.id, ratio)
      if (isSuccess(r)) {
        notify(`${isOpen ? '已修改为' : '已开启'} ${ratio}% 币抵扣`, 'success')
        onClose()
        onReload()
      } else {
        notify(retMsg(r, '开启失败'), 'error')
      }
    } catch (e) {
      notify(String(e?.message || e), 'error')
    } finally {
      setBusy(false)
    }
  }

  async function handleClose() {
    setBusy(true)
    try {
      const r = await itemCoinDeductionClose(item._uid, item.itemId || item.id)
      if (isSuccess(r)) {
        notify('已关闭币抵扣', 'success')
        onClose()
        onReload()
      } else {
        notify(retMsg(r, '关闭失败'), 'error')
      }
    } catch (e) {
      notify(String(e?.message || e), 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <BottomSheet
      open={open}
      onClose={onClose}
      title="币抵扣"
      footer={
        <>
          <button className="btn-secondary flex-1" onClick={onClose} disabled={busy}>取消</button>
          {isOpen ? (
            <button
              className="btn-danger flex-1"
              onClick={handleClose}
              disabled={busy || !canClose}
              title={canClose ? '关闭抵扣' : (closeHint || '开启 24 小时后才可关闭')}
              style={!canClose ? { opacity: 0.45, cursor: 'not-allowed' } : undefined}
            >
              {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : (canClose ? '关闭抵扣' : '暂时不能关闭')}
            </button>
          ) : (
            <button className="btn-primary flex-1" onClick={handleOpen} disabled={busy || !selected}>
              {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : '开启抵扣'}
            </button>
          )}
        </>
      }
    >
      {loading ? (
        <div className="flex items-center justify-center py-12 gap-2" style={{ color: 'var(--text-secondary)' }}>
          <Loader2 className="w-4 h-4 animate-spin" /> 加载中…
        </div>
      ) : (
        <div className="space-y-2">
          {isOpen && (
            <div className="liquid-notice liquid-notice-warning p-3 rounded-xl mb-2">
              <p className="text-[13px] font-medium">当前已开启 {currentRatio}% 抵扣</p>
              <p className="text-[12px] mt-1">
                {canClose ? '可修改比例，也可关闭抵扣' : `当前只能修改比例，${closeHint || '开启 24 小时后才可关闭'}`}
              </p>
            </div>
          )}
          {options.map((opt) => (
            <button
              key={opt.value}
              onClick={() => !opt.disabled && setSelected(opt.value)}
              disabled={opt.disabled}
              className="w-full flex items-center justify-between p-3 rounded-xl text-left transition-colors disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer"
              style={{
                backgroundColor: selected === opt.value ? 'var(--primary-light)' : 'var(--surface-soft)',
                border: `2px solid ${selected === opt.value ? 'var(--primary)' : 'transparent'}`,
              }}
            >
              <div className="flex-1 min-w-0">
                <div className="text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>{opt.label}</div>
                <div className="text-[12px] mt-0.5 truncate" style={{ color: 'var(--text-secondary)' }}>{opt.description}</div>
              </div>
              {selected === opt.value && (
                <div className="w-5 h-5 rounded-full flex items-center justify-center" style={{ backgroundColor: 'var(--primary)' }}>
                  <div className="w-2 h-2 rounded-full" style={{ backgroundColor: 'var(--liquid-highlight)' }} />
                </div>
              )}
            </button>
          ))}
        </div>
      )}
    </BottomSheet>
  )
}

// ============================================================
// 小刀（团购）BottomSheet
// ============================================================

interface KnifeSheetProps {
  open: boolean
  item: any
  onClose: () => void
  onReload: () => void
  notify: (message: string, type?: 'success' | 'error' | 'info', duration?: number) => void
}

export function KnifeSheet({ open, item, onClose, onReload, notify }: KnifeSheetProps) {
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [statusData, setStatusData] = useState<any>(null)
  const [options, setOptions] = useState<any[]>([])
  const [selected, setSelected] = useState('')
  const [customPrice, setCustomPrice] = useState('')
  const [mode, setMode] = useState<'amount' | 'discount'>('amount')

  useEffect(() => {
    if (!open || !item) return
    let cancelled = false
    setLoading(true)
    setSelected('')
    setCustomPrice('')
    const loadSettings = async () => {
      const itemId = item.itemId || item.id
      const status = await itemGrouponStatus(item._uid, itemId).catch(() => null)
      if (cancelled) return
      const statusOptions = status?.data?.drawer?.options
      const statusHasFormData = isSuccess(status) && Array.isArray(statusOptions) && statusOptions.length > 0
      const guide = statusHasFormData
        ? null
        : await itemGrouponGuide(item._uid, itemId).catch(() => null)
      if (cancelled) return
      const itemPrice = resolveGrouponItemPrice(item.price, status?.data?.price, guide?.data?.price)
      const opts = buildGrouponOptions(itemPrice, status, guide)
      setMode(opts[0]?.mode === 'discount' ? 'discount' : 'amount')
      setStatusData(status)
      setOptions(opts)
      const current = grouponDisplayPrice(status?.data) || grouponDisplayPrice(item)
      if (opts[0]?.mode === 'discount') {
        const currentOpt = opts.find((opt) => normalizeGrouponSubmitPrice(opt.finalPrice || '') === current)
        if (currentOpt) setSelected(currentOpt.value)
      } else if (current) setSelected(current)
    }
    void loadSettings().finally(() => !cancelled && setLoading(false))
    return () => { cancelled = true }
  }, [open, item])

  const isOpen = !!grouponDisplayPrice(statusData?.data) || !!grouponDisplayPrice(item)

  async function handleOpen() {
    const finalPrice = mode === 'discount'
      ? (options.find((opt) => opt.value === selected)?.finalPrice || '')
      : (customPrice || selected)
    const submitPrice = normalizeGrouponSubmitPrice(finalPrice)
    if (!submitPrice) {
      notify(mode === 'discount' ? '请选择折扣档位' : '请选择或输入小刀金额', 'error')
      return
    }
    const validation = grouponPriceValidation(submitPrice, item.price)
    if (!validation.ok) {
      notify(validation.message, 'error')
      return
    }
    setBusy(true)
    try {
      const r = await itemGrouponOpen(item._uid, item.itemId || item.id, submitPrice, {
        mode,
        discount_rate: mode === 'discount' ? String(selected || '') : '',
      })
      if (isSuccess(r)) {
        const selectedLabel = options.find((opt) => opt.value === selected)?.label
        notify(mode === 'discount' ? `小刀已开启：${selectedLabel || `¥${submitPrice}`}` : `小刀已开启：¥${submitPrice}`, 'success')
        onClose()
        void Promise.resolve().then(() => onReload()).catch(() => {})
      } else {
        notify(retMsg(r, '开启失败'), 'error')
      }
    } catch (e) {
      notify(String(e?.message || e), 'error')
    } finally {
      setBusy(false)
    }
  }

  async function handleClose() {
    setBusy(true)
    try {
      const r = await itemGrouponClose(item._uid, item.itemId || item.id)
      if (isSuccess(r)) {
        notify('已关闭小刀', 'success')
        onClose()
        void Promise.resolve().then(() => onReload()).catch(() => {})
      } else {
        notify(retMsg(r, '关闭失败'), 'error')
      }
    } catch (e) {
      notify(String(e?.message || e), 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <BottomSheet
      open={open}
      onClose={onClose}
      title="小刀优惠"
      footer={
        <>
          <button className="btn-secondary flex-1" onClick={onClose} disabled={busy}>取消</button>
          {isOpen ? (
            <button className="btn-danger flex-1" onClick={handleClose} disabled={busy}>
              {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : '关闭小刀'}
            </button>
          ) : (
            <button className="btn-primary flex-1" onClick={handleOpen} disabled={busy || (!selected && !customPrice)}>
              {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : '开启小刀'}
            </button>
          )}
        </>
      }
    >
      {loading ? (
        <div className="flex items-center justify-center py-12 gap-2" style={{ color: 'var(--text-secondary)' }}>
          <Loader2 className="w-4 h-4 animate-spin" /> 加载中…
        </div>
      ) : (
        <div className="space-y-3">
          <div className="text-[13px]" style={{ color: 'var(--text-secondary)' }}>
            售价 <span className="font-semibold" style={{ color: 'var(--primary)' }}>¥<SensitiveValue kind="money" value={formatMoneyDisplay(item?.price)} /></span>
            {isOpen && <> · 当前小刀 <span className="font-semibold" style={{ color: 'var(--warning)' }}>¥<SensitiveValue kind="money" value={grouponDisplayPrice(statusData?.data) || grouponDisplayPrice(item)} /></span></>}
          </div>

          <div className="grid grid-cols-3 gap-2">
            {options.map((opt) => (
              <button
                key={opt.value}
                onClick={() => !opt.disabled && (setSelected(opt.value), setCustomPrice(''))}
                disabled={opt.disabled}
                className="py-3 px-2 rounded-xl text-center transition-colors disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
                style={{
                  backgroundColor: selected === opt.value && !customPrice ? 'var(--primary-light)' : 'var(--surface-soft)',
                  border: `2px solid ${selected === opt.value && !customPrice ? 'var(--primary)' : 'transparent'}`,
                }}
              >
                <div className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>{opt.label}</div>
                <div className="text-[10px] mt-0.5" style={{ color: 'var(--text-muted)' }}>
                  {opt.disabled ? '不可用' : (mode === 'discount' ? (opt.description || '折扣档位') : '固定档位')}
                </div>
              </button>
            ))}
          </div>

          {mode !== 'discount' ? (
            <div>
              <label className="label">自定义小刀金额（元）</label>
              <input
                type="number"
                className="input"
                placeholder="例如 8.88"
                value={customPrice}
                onChange={(e) => { setCustomPrice(e.target.value); setSelected('') }}
                min="0"
                step="0.01"
                inputMode="decimal"
              />
            </div>
          ) : null}
        </div>
      )}
    </BottomSheet>
  )
}

// ============================================================
// 政策检查 BottomSheet
// ============================================================

interface PublishCheckSheetProps {
  open: boolean
  item: any
  onClose: () => void
  notify: (message: string, type?: 'success' | 'error' | 'info', duration?: number) => void
}

export function PublishCheckSheet({ open, item, onClose, notify }: PublishCheckSheetProps) {
  const [loading, setLoading] = useState(false)
  const [policyResult, setPolicyResult] = useState<any>(null)
  const [badwordsResult, setBadwordsResult] = useState<any>(null)

  useEffect(() => {
    if (!open || !item) return
    let cancelled = false
    setLoading(true)
    setPolicyResult(null)
    setBadwordsResult(null)
    Promise.all([
      publishCheck(item._uid).catch((e) => ({ error: String(e?.message || e) })),
      publishBadwords(item._uid, item.title || '').catch((e) => ({ error: String(e?.message || e) })),
    ]).then(([policy, badwords]) => {
      if (cancelled) return
      setPolicyResult(policy)
      setBadwordsResult(badwords)
    }).finally(() => !cancelled && setLoading(false))
    return () => { cancelled = true }
  }, [open, item])

  const policyMessage = policyResult?.error || policyResult?.data?.message || policyResult?.data?.errorMsg || ''
  const policyOk = policyResult && !policyResult.error && (policyResult?.data?.passed === true || policyResult?.data?.canPublish === true || isSuccess(policyResult))
  const badwordsList = Array.isArray(badwordsResult?.data?.matches) ? badwordsResult.data.matches : []
  const badwordsOk = badwordsResult && !badwordsResult.error && badwordsList.length === 0

  return (
    <BottomSheet
      open={open}
      onClose={onClose}
      title="政策检查"
      footer={<button className="btn-primary w-full" onClick={onClose}>关闭</button>}
    >
      {loading ? (
        <div className="flex items-center justify-center py-12 gap-2" style={{ color: 'var(--text-secondary)' }}>
          <Loader2 className="w-4 h-4 animate-spin" /> 检查中…
        </div>
      ) : (
        <div className="space-y-3">
          <div className={`liquid-notice ${policyOk ? 'liquid-notice-success' : 'liquid-notice-warning'} p-3 rounded-xl`}>
            <p className="text-[13px] font-semibold mb-1">
              账号发布资格 {policyOk ? '✓ 正常' : '⚠ 异常'}
            </p>
            <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              {policyOk ? '账号当前可正常发布商品' : (policyMessage || '账号发布权限受限，建议查看消息中心')}
            </p>
          </div>

          <div className={`liquid-notice ${badwordsOk ? 'liquid-notice-success' : 'liquid-notice-warning'} p-3 rounded-xl`}>
            <p className="text-[13px] font-semibold mb-1">
              标题违禁词检查 {badwordsOk ? '✓ 通过' : '⚠ 命中'}
            </p>
            {badwordsOk ? (
              <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>商品标题未命中违禁词</p>
            ) : (
              <div className="space-y-1">
                {badwordsList.length > 0 ? badwordsList.slice(0, 5).map((m, i) => (
                  <p key={i} className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                    · {String(m?.word || m?.text || m)}
                  </p>
                )) : (
                  <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>{badwordsResult?.error || '检查失败'}</p>
                )}
              </div>
            )}
          </div>

          <p className="text-[12px] text-center pt-2" style={{ color: 'var(--text-muted)' }}>
            完整发布检查请在桌面端使用
          </p>
        </div>
      )}
    </BottomSheet>
  )
}
