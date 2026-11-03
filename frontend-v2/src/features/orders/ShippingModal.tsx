// @ts-nocheck
import { Copy, RefreshCw } from 'lucide-react'
import { Modal } from '../../components/Modal'
import { SensitiveValue } from '../../components/SensitiveValue'
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

function AddressPanel({ title, address, copied, onCopy }) {
  const hasAddress = Boolean(formatAddressCopyText(address))

  return (
    <section className="liquid-muted-card rounded-[16px] p-4">
      <div className="flex items-center justify-between gap-3">
        <div className="text-[12.5px] font-semibold" style={{ color: 'var(--text-main)' }}>{title}</div>
        <button
          type="button"
          onClick={onCopy}
          disabled={!hasAddress}
          aria-label="复制地址"
          className="h-7 px-2.5 rounded-[8px] text-[12px] cursor-pointer disabled:opacity-40"
          style={{ background: 'var(--surface-control)', color: 'var(--text-main)', border: '1px solid var(--control-border)' }}
        >
          <Copy className="w-3.5 h-3.5 inline mr-1" />
          {copied ? '已复制' : '复制地址'}
        </button>
      </div>

      {hasAddress ? (
        <div className="mt-3 space-y-1.5">
          <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
            <SensitiveValue kind="buyer" value={address.fullName || '姓名未知'} />
            {address.mobile ? <SensitiveValue kind="phone" className="ml-2 font-mono" value={address.mobile} /> : null}
          </div>
          <div className="text-[12.5px] leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
            <SensitiveValue kind="address" value={address.formatFullAddress || '地址未知'} />
          </div>
        </div>
      ) : (
        <div className="mt-3 text-[12.5px]" style={{ color: 'var(--text-muted)' }}>地址未加载</div>
      )}
    </section>
  )
}

export function ShippingModal({
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
  return (
    <Modal
      open={Boolean(shipModal)}
      onClose={onClose}
      title="发货处理"
      footer={(
        <>
          <button onClick={onClose} className="btn-secondary btn-sm">取消</button>
          <button
            onClick={onSubmit}
            disabled={!shipModal || shipping || logisticsDisabled}
            className="btn-primary btn-sm disabled:opacity-40"
          >
            {shipping ? '发货中…' : shipMode === 'dummy' ? '确认无需物流发货' : '确认物流发货'}
          </button>
        </>
      )}
    >
      {shipModal ? (
        <div className="space-y-4" style={{ color: 'var(--text-main)', fontSize: 13.5 }}>
          <div className="liquid-muted-card rounded-[16px] p-4">
            <SensitiveValue kind="item-title" className="text-[13px] font-semibold" style={lineClamp2} value={getOrderTitle(shipModal.order)} />
            <div className="mt-2 flex flex-wrap items-center gap-2 text-[12.5px]" style={{ color: 'var(--text-secondary)' }}>
              <SensitiveValue kind="buyer" value={getOrderBuyer(shipModal.order)} />
              <span>·</span>
              <SensitiveValue kind="money" value={formatMoney(getOrderPrice(shipModal.order))} />
              <span>·</span>
              <span>{statusBadge(shipModal.order)}</span>
            </div>
          </div>

          <div className="glass-control flex items-center gap-1 rounded-[12px] p-[3px]">
            <button
              type="button"
              onClick={() => setShipMode('logistics')}
              className="flex-1 px-3 py-2 rounded-[8px] text-[13px] font-medium cursor-pointer"
              style={{ background: shipMode === 'logistics' ? 'var(--surface-control)' : 'transparent', color: shipMode === 'logistics' ? 'var(--text-main)' : 'var(--text-secondary)' }}
            >
              物流发货
            </button>
            <button
              type="button"
              onClick={() => setShipMode('dummy')}
              className="flex-1 px-3 py-2 rounded-[8px] text-[13px] font-medium cursor-pointer"
              style={{ background: shipMode === 'dummy' ? 'var(--surface-control)' : 'transparent', color: shipMode === 'dummy' ? 'var(--text-main)' : 'var(--text-secondary)' }}
            >
              无需物流
            </button>
          </div>

          {shipContextLoading ? (
            <div className="liquid-muted-card rounded-[16px] p-4 flex items-center gap-2 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
              <RefreshCw className="w-4 h-4 animate-spin" />
              正在加载发货信息…
            </div>
          ) : shipContextError ? (
            <div className="liquid-notice liquid-notice-danger rounded-[16px] p-4 text-[13px]">
              {shipContextError}
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
              <AddressPanel
                title="寄件地址"
                address={senderAddress}
                copied={copiedAddress === 'sender'}
                onCopy={() => onCopyAddress(formatAddressCopyText(senderAddress), 'sender')}
              />
              <AddressPanel
                title="收货地址"
                address={receiverAddress}
                copied={copiedAddress === 'receiver'}
                onCopy={() => onCopyAddress(formatAddressCopyText(receiverAddress), 'receiver')}
              />
            </div>
          )}

          {shipMode === 'logistics' ? (
            <div className="grid grid-cols-[1fr_1fr] gap-3">
              <label className="space-y-1.5">
                <div className="text-[12.5px] font-medium" style={{ color: 'var(--text-secondary)' }}>物流公司</div>
                <select
                  value={shipForm.companyCode}
                  onChange={(event) => {
                    const next = expressList.find((item) => item.code === event.target.value) || expressList[0]
                    setShipForm((prev) => ({ ...prev, companyCode: next.code, companyName: next.name }))
                  }}
                  className="w-full rounded-[10px] px-3 py-2 outline-none"
                  style={{ border: '1px solid var(--control-border)', background: 'var(--surface-control)' }}
                >
                  {expressList.map((item) => (
                    <option key={item.code} value={item.code}>{item.name}</option>
                  ))}
                </select>
              </label>

              <label className="space-y-1.5">
                <div className="text-[12.5px] font-medium" style={{ color: 'var(--text-secondary)' }}>物流单号</div>
                <input
                  value={shipForm.mailNo}
                  onChange={(event) => setShipForm((prev) => ({ ...prev, mailNo: event.target.value }))}
                  className="w-full rounded-[10px] px-3 py-2 outline-none"
                  style={{ border: '1px solid var(--control-border)', background: 'var(--surface-control)' }}
                  placeholder="请输入物流单号"
                />
              </label>
            </div>
          ) : (
            <div className="liquid-muted-card rounded-[16px] p-4">
              <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>无需物流</div>
              <div className="text-[12.5px] mt-1" style={{ color: 'var(--text-muted)' }}>适用于虚拟商品、线下交付或不需要填写快递单号的订单。</div>
            </div>
          )}
        </div>
      ) : null}
    </Modal>
  )
}
