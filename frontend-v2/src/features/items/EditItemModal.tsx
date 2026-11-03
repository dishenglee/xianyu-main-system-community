// @ts-nocheck
// ---------- 编辑 modal ----------
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { RefreshCw, Edit3, UploadCloud, X } from 'lucide-react'
import { itemEditDetail, itemProperties, itemEdit, republishItemWithDetail } from '../../api/index'
import { useSafeNotify } from '../../components/Notify'
import { emitActionFailure, emitActionStarted, emitActionSuccess } from '../../bootstrap/actionLogging'

function Field({ label, children }) {
  return (
    <div>
      <div className="text-[11.5px] mb-1" style={{ color: 'var(--text-muted)' }}>{label}</div>
      {children}
    </div>
  )
}

function normalizeDisplayPrice(raw: any, fallbackCents: any = '') {
  const text = String(raw ?? '').trim()
  if (!text) return ''
  const n = Number(text)
  if (Number.isNaN(n) || n <= 0) return ''
  const centsText = String(fallbackCents ?? '').trim()
  if (/^\d+$/.test(text) && /^\d+$/.test(centsText) && text === centsText) {
    return (n / 100).toFixed(2).replace(/\.00$/, '').replace(/(\.\d)0$/, '$1')
  }
  return text
}

function cleanSaveError(error: any) {
  let msg = String(error?.message || error || '保存失败')
  msg = msg.replace(/^non-success\s+upstream\s+ret:\s*/i, '')
  msg = msg.replace(/^[A-Z][A-Z0-9_]+::/, '')
  const j = msg.indexOf('||{')
  if (j > 0) msg = msg.slice(0, j).trim()
  return msg || '保存失败'
}

export function EditItemModal({ uid, item, mode = 'edit', onClose, onSaved }: any) {
  const isRepublish = mode === 'republish'
  const notify = useSafeNotify()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [form, setForm] = useState({
    title: String(item.title || ''),
    desc: String(item.desc || item.content || ''),
    price: String(item.price ?? ''),
    orig_price: String(item.origPrice ?? item.originalPrice ?? ''),
    quantity: String(item.quantity ?? '1'),
    no_bargain: null as boolean | null,
    can_free_shipping: null as boolean | null,
  })
  const [rawDetail, setRawDetail] = useState<any>(null)
  // 类目属性卡片（按商品标题/类目从 kgraph.property.recommend 真返）。
  // 闲鱼上游对"分类"卡返回的 valuesList 就是该商品的多个候选分类（按 score 排序），
  // 用户点不同 chip 只是更新选中（propSelections['-10000']），不需重调 kgraph。
  // 其他属性卡（计价方式/预计工期等）按 title 推断，跟用户选哪个分类无关。
  const [propCards, setPropCards] = useState<any[]>([])
  const [propSelections, setPropSelections] = useState<Record<string, string>>({})
  // 商品图片可变 state（支持删除）。元素结构 {url, ...}
  const [editImages, setEditImages] = useState<any[]>([])

  useEffect(() => {
    let cancelled = false
    // 切换到新 itemId 时先清掉上一件商品的属性 / 图片 / detail，避免一闪旧数据
    setRawDetail(null)
    setPropCards([])
    setPropSelections({})
    setEditImages([])
    setLoading(true)
    setError('')
    emitActionStarted({
      module: 'items',
      action: 'load_item_edit_detail',
      message: '开始加载商品编辑详情',
      context: {
        uid,
        itemId: item.itemId,
        mode,
      },
    })
    ;(async () => {
      try {
        const r: any = await itemEditDetail(uid, item.itemId)
        if (cancelled) return
        const data = r?.data ?? r
        setRawDetail(data)
        // 兼容两种实际返回结构（curl 实测确认）：
        //  A. /api/item/edit_detail（H5 mapping 格式）：
        //     data.{ itemTextDTO.{title,desc,shortTitle}, imageInfoDOList[], itemPriceDTO.{priceInCent,origPriceInCent,noBargain},
        //            itemCatDTO.catId, quantity, simpleItem }
        //  B. /api/item/detail（pc.detail 直出格式）：
        //     data.itemDO.{title, desc, soldPrice, originalPrice, quantity, simpleItem, imageInfos[], itemCatDTO.{catId,tbCatId,channelCatId,leafId}, cpvLabels[]}
        const itemDO = data?.itemDO || data?.item || {}
        const txt = data?.itemTextDTO || {}
        const priceDTO = data?.itemPriceDTO || {}
        const centToYuan = (v: any) => {
          const n = Number(String(v ?? '').trim()); if (!n || Number.isNaN(n)) return ''
          return (n / 100).toFixed(2).replace(/\.00$/, '').replace(/(\.\d)0$/, '$1')
        }
        const title = itemDO.title ?? txt.title ?? form.title
        const desc = itemDO.desc ?? itemDO.description ?? txt.desc ?? form.desc
        const price = normalizeDisplayPrice(itemDO.soldPrice ?? itemDO.price, priceDTO.priceInCent) || centToYuan(priceDTO.priceInCent) || form.price
        const origPrice = normalizeDisplayPrice(itemDO.originalPrice ?? itemDO.origPrice, priceDTO.origPriceInCent) || centToYuan(priceDTO.origPriceInCent) || form.orig_price
        const qty = itemDO.quantity ?? data?.quantity ?? form.quantity
        const nb = itemDO.noBargain ?? data?.noBargain ?? priceDTO.noBargain
        const cfs = itemDO.canFreeShipping ?? data?.canFreeShipping ?? itemDO.freeShipping
        setForm((p) => ({
          ...p,
          title: String(title ?? ''),
          desc: String(desc ?? ''),
          price: String(price ?? ''),
          orig_price: String(origPrice ?? ''),
          quantity: String(qty ?? '1'),
          no_bargain: typeof nb === 'boolean' ? nb : p.no_bargain,
          can_free_shipping: typeof cfs === 'boolean' ? cfs : p.can_free_shipping,
        }))

        // 初始化可编辑图片列表（兼容两种结构）
        const imgsRaw = Array.isArray(itemDO.imageInfos) ? itemDO.imageInfos
          : Array.isArray(data?.imageInfoDOList) ? data.imageInfoDOList
          : Array.isArray(itemDO.imageUrls) ? itemDO.imageUrls.map((u: string) => ({ url: u }))
          : []
        setEditImages(imgsRaw.map((img: any) => ({
          url: String(img?.url || img?.picUrl || (typeof img === 'string' ? img : '')),
          ...img,
        })).filter((it: any) => it.url))

        // 并行拉类目属性 schema —— 后端会用商品标题搜索，从
        // smartUiFilterPanelDO.filterPanelDOList 提取该类目的属性 cards。
        // 不同 itemId 标题/类目不同 → 返不同 propertyName/valuesList。
        const titleForKw = String(itemDO.title || txt.title || data?.itemTextDTO?.title || (item as any)?.title || '')
        const cci = String(itemDO?.itemCatDTO?.channelCatId || data?.itemCatDTO?.channelCatId || (item as any)?.detail?.channelCatId || (item as any)?.channelCatId || '')
        let loadedPropertyCardCount = 0
        try {
          const p: any = await itemProperties(uid, item.itemId, titleForKw, cci)
          if (cancelled) return
          const cards = (p?.data?.cards || []) as any[]
          // 已选值：常规属性从 cpvLabels 取（propertyId -> valueId），但 *"分类"
          // (propertyId=-10000)* 不在 cpvLabels 里，而是从 itemCatDTO.channelCatId
          // 拿当前类目作为已选 valueId。这是闲鱼协议的特殊：分类是商品的类目本身。
          const labels = [
            ...((itemDO.cpvLabels || []) as any[]),
            ...((data?.cpvLabels || []) as any[]),
            ...((data?.itemLabelExtList || []) as any[]),
          ]
          const labelMap: Record<string, string> = {}
          for (const l of labels) {
            if (l?.propertyId != null && l?.valueId != null) {
              labelMap[String(l.propertyId)] = String(l.valueId)
            }
          }
          String(itemDO.properties || data?.properties || '')
            .split(/[;,]/)
            .map((part) => part.trim())
            .filter(Boolean)
            .forEach((part) => {
              const [propertyId, valueId] = part.split(':').map((it) => String(it || '').trim())
              if (propertyId && valueId && !labelMap[propertyId]) labelMap[propertyId] = valueId
            })
          // 分类已选 = 当前商品的 channelCatId（兼容两种 detail 结构）
          const currentChannelCat = String(
            itemDO?.itemCatDTO?.channelCatId
            || data?.itemCatDTO?.channelCatId
            || (item as any)?.detail?.channelCatId
            || (item as any)?.channelCatId
            || '',
          ).trim()
          if (currentChannelCat) labelMap['-10000'] = currentChannelCat
          const norm = cards.map((c: any) => ({
            propertyId: String(c.propertyId ?? ''),
            propertyName: String(c.propertyName ?? ''),
            valuesList: (c.valuesList || []).map((v: any) => ({
              valueId: String(v.valueId ?? ''),
              valueName: String(v.valueName ?? ''),
            })),
            selectedValueId: labelMap[String(c.propertyId ?? '')] || '',
          }))
          if (cancelled) return
          loadedPropertyCardCount = norm.length
          setPropCards(norm)
          const init: Record<string, string> = {}
          for (const card of norm) {
            if (card.selectedValueId) init[card.propertyId] = card.selectedValueId
          }
          setPropSelections(init)
        } catch { /* schema 拉不到不阻塞编辑 */ }
        if (!cancelled) {
          emitActionSuccess({
            module: 'items',
            action: 'load_item_edit_detail',
            message: '商品编辑详情加载成功',
            context: {
              uid,
              itemId: item.itemId,
              mode,
              imageCount: imgsRaw.length,
              propertyCardCount: loadedPropertyCardCount,
              hasDetail: !!data,
            },
          })
        }
      } catch (e: any) {
        if (!cancelled) {
          emitActionFailure({
            module: 'items',
            action: 'load_item_edit_detail',
            message: '商品编辑详情加载失败',
            error: e,
            context: {
              uid,
              itemId: item.itemId,
              mode,
            },
          })
          setError(e?.message || '加载商品详情失败')
        }
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => { cancelled = true }
  }, [uid, item.itemId])

  async function save() {
    setSaving(true)
    setError('')
    emitActionStarted({
      module: 'items',
      action: isRepublish ? 'republish_item_save' : 'edit_item_save',
      message: isRepublish ? '开始保存并上架商品' : '开始保存商品编辑',
      context: {
        uid,
        itemId: item.itemId,
        mode,
        imageCount: editImages.length,
        propertyCardCount: propCards.length,
        hasDetail: !!rawDetail,
      },
    })
    try {
      const payload: any = {
        title: form.title,
        desc: form.desc,
        price: form.price,
        orig_price: form.orig_price,
        image_urls: editImages.map((img: any) => img.url).filter(Boolean),
        card_list: propCards,
        prop_selections: propSelections,
      }
      const quantity = Number.parseInt(String(form.quantity || '').trim(), 10)
      if (Number.isFinite(quantity) && quantity > 0) payload.quantity = quantity
      if (form.no_bargain !== null) payload.no_bargain = form.no_bargain
      if (form.can_free_shipping !== null) payload.can_free_shipping = form.can_free_shipping
      if (rawDetail) payload.detail = rawDetail
      if (isRepublish) {
        const openResp: any = await republishItemWithDetail(uid, item.itemId, item, rawDetail, payload)
        const openRet = Array.isArray(openResp?.ret) ? String(openResp.ret[0] || '') : ''
        if (openRet && !openRet.startsWith('SUCCESS::')) {
          throw new Error(openRet)
        }
      } else {
        const editResp: any = await itemEdit(uid, item.itemId, payload)
        const editRet = Array.isArray(editResp?.ret) ? String(editResp.ret[0] || '') : ''
        if (editRet && !editRet.startsWith('SUCCESS::')) {
          throw new Error(editRet)
        }
      }
      notify(isRepublish ? '商品已保存并上架' : '商品保存成功', 'success')
      emitActionSuccess({
        module: 'items',
        action: isRepublish ? 'republish_item_save' : 'edit_item_save',
        message: isRepublish ? '商品保存并上架成功' : '商品编辑保存成功',
        context: {
          uid,
          itemId: item.itemId,
          mode,
          imageCount: editImages.length,
          propertyCardCount: propCards.length,
        },
      })
      try {
        await onSaved?.()
      } catch (refreshError: any) {
        notify(`商品已保存，但列表刷新失败：${cleanSaveError(refreshError)}`, 'error')
      }
    } catch (e: any) {
      // 把错误信息净化成人话
      const msg = cleanSaveError(e)
      emitActionFailure({
        module: 'items',
        action: isRepublish ? 'republish_item_save' : 'edit_item_save',
        message: isRepublish ? '商品保存并上架失败' : '商品编辑保存失败',
        error: msg,
        context: {
          uid,
          itemId: item.itemId,
          mode,
          imageCount: editImages.length,
          propertyCardCount: propCards.length,
        },
      })
      setError(msg)
    } finally {
      setSaving(false)
    }
  }

  // 同时支持两种结构（实测确认）：
  //   A. data.itemDO.{imageInfos[], itemCatDTO}（pc.detail 直出）
  //   B. data.{imageInfoDOList[], itemCatDTO}（edit_detail mapping）
  const itemDO = (rawDetail?.itemDO || rawDetail?.item || {}) as any

  return createPortal((
    <div
      className="liquid-scrim fixed inset-0 z-[120] flex items-end justify-center p-0 md:items-center md:p-4"
      onClick={onClose}
    >
      <div
        className="liquid-panel flex w-full max-w-[540px] flex-col overflow-hidden rounded-t-[28px] md:rounded-[26px]"
        style={{ maxHeight: 'calc(100dvh - env(safe-area-inset-top) - 8px)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b" style={{ borderColor: 'var(--control-border)' }}>
          <div className="flex items-center gap-2">
            {isRepublish ? <UploadCloud className="w-4 h-4" style={{ color: 'var(--primary)' }} /> : <Edit3 className="w-4 h-4" style={{ color: 'var(--primary)' }} />}
            <span className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>{isRepublish ? '重新上架商品' : '编辑商品'}</span>
            {isRepublish ? (
              <span className="ml-1 text-[11px] px-1.5 py-[1px] rounded-full" style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}>修改后将重新上架</span>
            ) : null}
          </div>
          <button onClick={onClose} className="mobile-icon-button p-1 rounded-full cursor-pointer">
            <X className="w-4 h-4" style={{ color: 'var(--text-muted)' }} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-auto px-4 py-3 space-y-2.5">
          {loading ? (
            <div className="flex items-center justify-center py-10 gap-2" style={{ color: 'var(--text-muted)' }}>
              <RefreshCw className="w-4 h-4 animate-spin" /> 加载商品详情…
            </div>
          ) : (
            <>
              {/* 商品图（可删除）—— 跟闲鱼 APP 一样，每张图右上角 X 按钮删除 */}
              {editImages.length > 0 ? (
                <Field label={`商品图（${editImages.length} 张）· 点 X 删除`}>
                  <div className="flex gap-1.5 overflow-x-auto pb-1">
                    {editImages.map((img: any, i: number) => (
                      <div key={i} className="relative shrink-0 group">
                        <img
                          src={img.url}
                          alt=""
                          className="h-16 w-16 rounded-[8px] border object-cover"
                          style={{ borderColor: 'var(--control-border)' }}
                          onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none' }}
                        />
                        <button
                          type="button"
                          onClick={() => setEditImages((prev) => prev.filter((_, j) => j !== i))}
                          className="absolute -top-1 -right-1 w-5 h-5 rounded-full text-white flex items-center justify-center cursor-pointer text-[11px] font-bold"
                          style={{ background: 'rgba(17,24,39,0.78)' }}
                          title="删除这张图"
                        >
                          ×
                        </button>
                      </div>
                    ))}
                  </div>
                </Field>
              ) : null}

              {/* 类目信息不再展示内部 ID（catId/tbCatId/channelCatId/leafId 是给后端调用
                  闲鱼接口用的数字编号，普通用户看不懂也没必要看）。类目人话名称已经
                  通过 propertyName="分类" 那张 chip 卡片展示给用户，足够直观。 */}

              {/* 类目属性（按商品类目动态加载）—— 闲鱼 APP 风格：
                  白底圆角卡片 + 每行左侧 propertyName 标签 + 右侧 chip 行。 */}
              {propCards.length > 0 ? (
                <div
                  className="liquid-muted-card rounded-[16px] px-3 py-2.5"
                >
                  <div className="flex flex-col gap-2.5">
                    {propCards.map((card) => (
                      <div key={card.propertyId} className="flex items-center gap-2">
                        <span className="text-[12.5px] shrink-0 w-[68px]" style={{ color: 'var(--text-muted)' }}>
                          {card.propertyName}
                        </span>
                        <div className="flex flex-wrap gap-1.5 flex-1 min-w-0">
                          {card.valuesList.slice(0, 12).map((v: any) => {
                            // 双向都非空才判 selected，避免空串相等的 bug
                            const sel = String(propSelections[card.propertyId] || '').trim()
                            const vid = String(v.valueId || '').trim()
                            const selected = sel.length > 0 && vid.length > 0 && sel === vid
                            return (
                              <button
                                key={v.valueId}
                                type="button"
                                onClick={() => {
                                  // 已选 → 取消选择（删除 key）；未选 → 设为当前
                                  setPropSelections((prev) => {
                                    const next = { ...prev }
                                    if (next[card.propertyId] === v.valueId) {
                                      delete next[card.propertyId]
                                    } else {
                                      next[card.propertyId] = v.valueId
                                    }
                                    return next
                                  })
                                }}
                                className="px-3 py-1.5 rounded-[8px] text-[12.5px] font-medium cursor-pointer transition-colors"
                                style={{
                                  background: selected ? 'var(--control-active)' : 'var(--surface-soft)',
                                  color: selected ? 'var(--primary)' : 'var(--text-secondary)',
                                  border: `1px solid ${selected ? 'color-mix(in srgb, var(--primary) 20%, transparent)' : 'transparent'}`,
                                }}
                              >
                                {v.valueName}
                              </button>
                            )
                          })}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              <Field label="标题">
                <input
                  className="input text-[13px]"
                  value={form.title}
                  onChange={(e) => setForm({ ...form, title: e.target.value })}
                />
              </Field>

              <Field label="描述">
                <textarea
                  className="input text-[13px] resize-none"
                  style={{ minHeight: 120 }}
                  value={form.desc}
                  onChange={(e) => setForm({ ...form, desc: e.target.value })}
                />
              </Field>

              <div className="grid grid-cols-3 gap-2.5">
                <Field label="售价 ¥">
                  <input
                    className="input text-[13px] font-mono"
                    value={form.price}
                    onChange={(e) => setForm({ ...form, price: e.target.value })}
                    inputMode="decimal"
                  />
                </Field>
                <Field label="原价 ¥">
                  <input
                    className="input text-[13px] font-mono"
                    value={form.orig_price}
                    onChange={(e) => setForm({ ...form, orig_price: e.target.value })}
                    inputMode="decimal"
                  />
                </Field>
                <Field label="库存">
                  <input
                    className="input text-[13px] font-mono"
                    value={form.quantity}
                    onChange={(e) => setForm({ ...form, quantity: e.target.value })}
                    inputMode="numeric"
                  />
                </Field>
              </div>

              <div className="flex items-center gap-4 pt-1">
                <label className="flex items-center gap-1.5 text-[12.5px] cursor-pointer" style={{ color: 'var(--text-main)' }}>
                  <input
                    type="checkbox"
                    checked={form.no_bargain === true}
                    onChange={(e) => setForm({ ...form, no_bargain: e.target.checked })}
                  />
                  不还价
                </label>
                <label className="flex items-center gap-1.5 text-[12.5px] cursor-pointer" style={{ color: 'var(--text-main)' }}>
                  <input
                    type="checkbox"
                    checked={form.can_free_shipping === true}
                    onChange={(e) => setForm({ ...form, can_free_shipping: e.target.checked })}
                  />
                  包邮
                </label>
              </div>

              {error ? (
                <div className="text-[12px] px-2.5 py-1.5 rounded-[12px]" style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}>
                  {error}
                </div>
              ) : null}
            </>
          )}
        </div>

        <div
          className="shrink-0 flex items-center justify-end gap-2 px-4 pt-3 border-t"
          style={{
            borderColor: 'var(--control-border)',
            background: 'rgba(255,255,255,0.72)',
            paddingBottom: 'calc(14px + env(safe-area-inset-bottom))',
          }}
        >
          <button onClick={onClose} className="btn-ghost btn-sm cursor-pointer">取消</button>
          {(() => {
            // 跳闲鱼 APP 编辑链接（rePublishUrl 是闲鱼自带 deeplink）
            const dl = String((item as any)?.detail?.rePublishUrl || '').trim()
            if (!dl) return null
            return (
              <a
                href={dl}
                target="_blank"
                rel="noreferrer"
                className="btn-ghost btn-sm cursor-pointer"
                title="跳到闲鱼 APP 完成编辑/上架（闲鱼对 PC 端发布做了网关风控，APP 端才能成）"
              >
                在闲鱼 APP 编辑
              </a>
            )
          })()}
          <button
            onClick={save}
            disabled={loading || saving}
            className="btn-primary btn-sm cursor-pointer disabled:opacity-40"
            title={isRepublish
              ? '尝试通过 PC 端 mtop 直接保存并上架。闲鱼可能拒绝（FAIL_BIZ_IDLE_UNKNOWN_THROWABLE / FAIL_SYS_UNAUTHORIZED_ENTRANCE），失败时请用「在闲鱼 APP 编辑」'
              : ''}
          >
            {saving ? (
              <><RefreshCw className="w-3.5 h-3.5 animate-spin" /> {isRepublish ? '上架中' : '保存中'}</>
            ) : (isRepublish ? '保存并上架' : '保存')}
          </button>
        </div>
      </div>
    </div>
  ), document.body)
}
