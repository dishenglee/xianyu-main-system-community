// @ts-nocheck
import { Camera, Copy, RefreshCw, ScanLine, X } from 'lucide-react'
import { useNotify } from '../../components/Notify'
import {
  expressList,
  lineClamp2,
  formatAddressCopyText,
  getOrderTitle,
  getOrderBuyer,
  getOrderPrice,
  formatMoney,
  statusBadge,
} from './orderUtils'
import { SensitiveValue } from '../../components/SensitiveValue'

// 移动端发货全屏 sheet —— 与桌面 ShippingModal 功能等价
// 提供物流 / 无需物流双 tab，地址面板，扫码识别快递单号按钮（占位）
export function MobileShippingSheet({
  shipModal,
  onClose,
  shipMode,
  setShipMode,
  shipForm,
  setShipForm,
  shipContextLoading,
  shipContextError,
  senderAddress,
  receiverAddress,
  copiedAddress,
  onCopyAddress,
  shipping,
  logisticsDisabled,
  onSubmit,
}) {
  const notify = useNotify()

  if (!shipModal) return null

  return (
    <div className="fixed inset-0 z-[90] flex flex-col" style={{ background: 'var(--bg-page)' }}>
      {/* 顶部 sticky 头 */}
      <div
        className="flex items-center justify-between px-4"
        style={{
          height: 56,
          paddingTop: 'env(safe-area-inset-top)',
          borderBottom: '1px solid var(--glass-border)',
          background: 'var(--mobile-glass-surface)',
          backdropFilter: 'saturate(180%) blur(22px)',
          WebkitBackdropFilter: 'saturate(180%) blur(22px)',
        }}
      >
        <button
          type="button"
          onClick={onClose}
          className="h-10 w-10 -ml-2 rounded-full flex items-center justify-center cursor-pointer"
          aria-label="关闭"
        >
          <X className="w-5 h-5" style={{ color: 'var(--text-main)' }} />
        </button>
        <h2 className="text-[16px] font-semibold" style={{ color: 'var(--text-main)' }}>发货处理</h2>
        <div className="w-10" />
      </div>

      {/* 滚动主体 */}
      <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4">
        {/* 订单信息卡 */}
        <div className="rounded-[14px] p-4" style={{ background: 'var(--surface-card)', border: '1px solid var(--glass-border)' }}>
          <SensitiveValue kind="item-title" className="text-[14px] font-semibold" style={lineClamp2} value={getOrderTitle(shipModal.order)} />
          <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px]" style={{ color: 'var(--text-muted)' }}>
            <SensitiveValue kind="buyer" value={getOrderBuyer(shipModal.order)} />
            <span>·</span>
            <SensitiveValue kind="money" className="font-mono" value={formatMoney(getOrderPrice(shipModal.order))} />
            <span>·</span>
            <span>{statusBadge(shipModal.order)}</span>
          </div>
        </div>

        {/* 发货模式切换 */}
        <div className="flex items-center gap-1 rounded-[12px] p-[3px]" style={{ background: 'var(--mobile-glass-surface)' }}>
          <button
            type="button"
            onClick={() => setShipMode('logistics')}
            className="flex-1 h-10 rounded-[10px] text-[13.5px] font-medium cursor-pointer"
            style={{
              background: shipMode === 'logistics' ? 'var(--surface-card)' : 'transparent',
              color: shipMode === 'logistics' ? 'var(--text-main)' : 'var(--text-muted)',
              boxShadow: shipMode === 'logistics' ? 'var(--glass-shadow, 0 1px 2px rgba(17,24,39,0.08))' : 'none',
            }}
          >
            物流发货
          </button>
          <button
            type="button"
            onClick={() => setShipMode('dummy')}
            className="flex-1 h-10 rounded-[10px] text-[13.5px] font-medium cursor-pointer"
            style={{
              background: shipMode === 'dummy' ? 'var(--surface-card)' : 'transparent',
              color: shipMode === 'dummy' ? 'var(--text-main)' : 'var(--text-muted)',
              boxShadow: shipMode === 'dummy' ? 'var(--glass-shadow, 0 1px 2px rgba(17,24,39,0.08))' : 'none',
            }}
          >
            无需物流
          </button>
        </div>

        {/* 地址 */}
        {shipContextLoading ? (
          <div
            className="rounded-[14px] p-4 flex items-center gap-2 text-[13px]"
            style={{ border: '1px solid var(--glass-border)', background: 'var(--surface-card)', color: 'var(--text-muted)' }}
          >
            <RefreshCw className="w-4 h-4 animate-spin" />
            正在加载发货信息…
          </div>
        ) : shipContextError ? (
          <div
            className="rounded-[14px] p-4 text-[13px]"
            style={{ border: '1px solid color-mix(in srgb, var(--danger) 18%, transparent)', background: 'color-mix(in srgb, var(--danger) 10%, var(--surface-card))', color: 'var(--danger)' }}
          >
            {shipContextError}
          </div>
        ) : (
          <div className="space-y-3">
            <AddressBlock
              title="寄件地址"
              address={senderAddress}
              copied={copiedAddress === 'sender'}
              onCopy={() => onCopyAddress(formatAddressCopyText(senderAddress), 'sender')}
            />
            <AddressBlock
              title="收货地址"
              address={receiverAddress}
              copied={copiedAddress === 'receiver'}
              onCopy={() => onCopyAddress(formatAddressCopyText(receiverAddress), 'receiver')}
            />
          </div>
        )}

        {/* 物流表单 / 无需物流提示 */}
        {shipMode === 'logistics' ? (
          <div className="space-y-3">
            <label className="block space-y-1.5">
              <span className="text-[12.5px] font-medium" style={{ color: 'var(--text-muted)' }}>物流公司</span>
              <select
                value={shipForm.companyCode}
                onChange={(event) => {
                  const next = expressList.find((item) => item.code === event.target.value) || expressList[0]
                  setShipForm((prev) => ({ ...prev, companyCode: next.code, companyName: next.name }))
                }}
                className="w-full h-11 rounded-[10px] px-3 outline-none text-[14px]"
                style={{ border: '1px solid var(--glass-border)', background: 'var(--surface-card)' }}
              >
                {expressList.map((item) => (
                  <option key={item.code} value={item.code}>{item.name}</option>
                ))}
              </select>
            </label>

            <label className="block space-y-1.5">
              <span className="text-[12.5px] font-medium" style={{ color: 'var(--text-muted)' }}>物流单号</span>
              <div
                className="flex items-center gap-1 rounded-[10px] px-3 h-11"
                style={{ border: '1px solid var(--glass-border)', background: 'var(--surface-card)' }}
              >
                <input
                  inputMode="numeric"
                  value={shipForm.mailNo}
                  onChange={(event) => setShipForm((prev) => ({ ...prev, mailNo: event.target.value }))}
                  className="flex-1 bg-transparent outline-none text-[14px] font-mono"
                  placeholder="请输入或扫描快递单号"
                />
                <button
                  type="button"
                  className="h-8 px-2.5 rounded-[8px] flex items-center gap-1 text-[12px]"
                  style={{ background: 'color-mix(in srgb, var(--primary) 10%, transparent)', color: 'var(--primary)' }}
                  onClick={() => notify.info('扫码识别功能需要相机权限，即将上线')}
                  aria-label="扫码识别快递单号"
                  title="扫码识别快递单号"
                >
                  <ScanLine className="w-3.5 h-3.5" />
                  扫码
                </button>
              </div>
              <p className="text-[11.5px]" style={{ color: 'var(--text-muted)' }}>
                <Camera className="w-3 h-3 inline -mt-0.5 mr-0.5" />
                可通过相机扫描快递单条码自动填入（开发中）
              </p>
            </label>
          </div>
        ) : (
          <div className="rounded-[14px] p-4" style={{ border: '1px solid var(--glass-border)', background: 'var(--surface-card)' }}>
            <div className="text-[13.5px] font-medium" style={{ color: 'var(--text-main)' }}>无需物流</div>
            <div className="text-[12.5px] mt-1" style={{ color: 'var(--text-muted)' }}>
              适用于虚拟商品、线下交付或不需要填写快递单号的订单。
            </div>
          </div>
        )}
      </div>

      {/* 底部固定操作条 */}
      <div
        className="px-4 pt-3 flex items-center gap-2"
        style={{
          borderTop: '1px solid var(--glass-border)',
          background: 'var(--mobile-glass-surface)',
          backdropFilter: 'saturate(180%) blur(22px)',
          WebkitBackdropFilter: 'saturate(180%) blur(22px)',
          paddingBottom: 'calc(env(safe-area-inset-bottom) + 12px)',
        }}
      >
        <button
          type="button"
          onClick={onClose}
          className="btn-secondary"
          style={{ height: 44, flex: 1 }}
        >
          取消
        </button>
        <button
          type="button"
          onClick={onSubmit}
          disabled={shipping || logisticsDisabled}
          className="btn-primary disabled:opacity-40"
          style={{ height: 44, flex: 2 }}
        >
          {shipping ? '发货中…' : shipMode === 'dummy' ? '确认无需物流发货' : '确认发货'}
        </button>
      </div>
    </div>
  )
}

function AddressBlock({ title, address, copied, onCopy }) {
  const hasAddress = Boolean(formatAddressCopyText(address))

  return (
    <section className="rounded-[14px] p-4" style={{ border: '1px solid var(--glass-border)', background: 'var(--surface-card)' }}>
      <div className="flex items-center justify-between gap-3">
        <div className="text-[12.5px] font-semibold" style={{ color: 'var(--text-main)' }}>{title}</div>
        <button
          type="button"
          onClick={onCopy}
          disabled={!hasAddress}
          aria-label="复制地址"
          className="h-7 px-2.5 rounded-[8px] text-[12px] cursor-pointer disabled:opacity-40"
          style={{ background: 'var(--mobile-glass-surface)', color: 'var(--text-main)' }}
        >
          <Copy className="w-3.5 h-3.5 inline mr-1" />
          {copied ? '已复制' : '复制'}
        </button>
      </div>

      {hasAddress ? (
        <div className="mt-3 space-y-1.5">
          <div className="text-[13.5px] font-medium" style={{ color: 'var(--text-main)' }}>
            <SensitiveValue kind="buyer" value={address.fullName || '姓名未知'} />
            {address.mobile ? <SensitiveValue kind="phone" className="ml-2 font-mono" value={address.mobile} /> : null}
          </div>
          <div className="text-[12.5px] leading-relaxed" style={{ color: 'var(--text-muted)' }}>
            <SensitiveValue kind="address" value={address.formatFullAddress || '地址未知'} />
          </div>
        </div>
      ) : (
        <div className="mt-3 text-[12.5px]" style={{ color: 'var(--text-muted)' }}>地址未加载</div>
      )}
    </section>
  )
}
