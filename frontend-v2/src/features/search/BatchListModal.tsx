// @ts-nocheck
// 一键上架 modal — 从搜索结果批量导入并发布到自己的账号
import { useState } from 'react'
import {
  X, RefreshCw, CheckCircle2, XCircle, ShoppingBag, AlertCircle,
} from 'lucide-react'
import {
  itemImportLink, itemPublish, publishCheck,
  uploadImageUrl, applyPostPublishExtras, refreshAccount, syncItems,
} from '../../api/index'
import {
  isPublishAllowed, publishCheckMessage, pickPreferredPublishAccount, resolveSimpleItemFlag,
} from '../items/publishUtils'
import { emitItemsSynced } from '../runtimeEvents'
import { useSafeNotify } from '../../components/Notify'
import { useTaskProgressStore } from '../../store/taskProgressStore'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import { SensitiveItemImage, SensitiveValue } from '../../components/SensitiveValue'

/* ── Types ────────────────────────────────────────────────────────── */

interface SearchItem {
  itemId: string
  title: string
  price: string
  picUrl: string
  sellerNick: string
  area: string
  freeShip: boolean
}

interface ListResult {
  itemId: string
  itemTitle: string
  uid: string
  nick: string
  ok: boolean
  msg: string
}

type PriceMode = 'original' | 'markup' | 'markdown' | 'fixed'
const MAX_BATCH_PUBLISH_TITLE_LENGTH = 30

/* ── Price calc ───────────────────────────────────────────────────── */

function calcPrice(original: string, mode: PriceMode, adjustVal: string, fixedVal: string): string {
  const p = parseFloat(original) || 0
  if (p <= 0) return original
  const pct = parseFloat(adjustVal) || 0
  switch (mode) {
    case 'markup':
      return (p * (1 + pct / 100)).toFixed(2)
    case 'markdown':
      return Math.max(0.01, p * (1 - pct / 100)).toFixed(2)
    case 'fixed':
      return fixedVal || original
    default:
      return String(p)
  }
}

export function normalizeBatchPublishTitle(title: string): string {
  return String(title || '').trim().slice(0, MAX_BATCH_PUBLISH_TITLE_LENGTH)
}

function toTaskProgressResult(result: ListResult) {
  return {
    uid: result.uid,
    label: `${result.itemTitle || result.itemId || '商品'} → ${result.nick || result.uid}`,
    ok: result.ok,
    msg: result.msg,
  }
}

/* ── Component ────────────────────────────────────────────────────── */

export function BatchListModal({
  items,
  accounts: rawAccounts,
  onClose,
}: {
  items: SearchItem[]
  accounts: any[]
  onClose: () => void
}) {
  const notify = useSafeNotify()
  const taskProgress = useTaskProgressStore()
  // Sort accounts by security_score descending
  const accounts = [...rawAccounts].sort(
    (a, b) => (Number(b.security_score) || 0) - (Number(a.security_score) || 0),
  )
  const availableAccounts = filterBusinessAccounts(accounts)

  // Target accounts
  const [selectedUids, setSelectedUids] = useState<string[]>(() => {
    const a = pickPreferredPublishAccount(availableAccounts)
    return a ? [a.uid] : availableAccounts.length ? [availableAccounts[0].uid] : []
  })

  // Price strategy
  const [priceMode, setPriceMode] = useState<PriceMode>('original')
  const [adjustVal, setAdjustVal] = useState('10')
  const [fixedVal, setFixedVal] = useState('')

  // Options
  const [shippingType, setShippingType] = useState<'free' | 'fixed' | 'self'>('free')
  const [enableCoin, setEnableCoin] = useState(true)
  const [coinPct, setCoinPct] = useState('5')
  const [grouponEnabled, setGrouponEnabled] = useState(true)
  const [grouponAmt, setGrouponAmt] = useState('')

  // Progress
  const [running, setRunning] = useState(false)
  const [finished, setFinished] = useState(false)
  const [progress, setProgress] = useState({ done: 0, total: 0, current: '' })
  const [results, setResults] = useState<ListResult[]>([])

  async function ensurePublishable(uid: string) {
    let checkResp: any = await publishCheck(uid).catch(() => null)
    if (checkResp && isPublishAllowed(checkResp)) return { ok: true, checkResp }

    const sessionReason = String(
      checkResp?.data?.message
      || checkResp?.data?.detail
      || checkResp?.detail
      || checkResp?.ret?.[0]
      || '',
    ).toUpperCase()

    if (
      sessionReason.includes('SESSION_EXPIRED')
      || sessionReason.includes('NOT_LOGIN')
      || sessionReason.includes('TOKEN')
    ) {
      await refreshAccount(uid).catch(() => null)
      checkResp = await publishCheck(uid).catch(() => null)
    }

    return { ok: !!(checkResp && isPublishAllowed(checkResp)), checkResp }
  }

  function toggleUid(uid: string) {
    setSelectedUids(prev =>
      prev.includes(uid)
        ? prev.length > 1 ? prev.filter(u => u !== uid) : prev
        : [...prev, uid],
    )
  }

  function toggleAllUids() {
    setSelectedUids(
      selectedUids.length === availableAccounts.length
        ? (() => { const a = pickPreferredPublishAccount(availableAccounts); return a ? [a.uid] : [] })()
        : availableAccounts.map(a => a.uid),
    )
  }

  async function doSubmit() {
    if (!selectedUids.length || !items.length || running) return
    if (!availableAccounts.length) {
      notify('当前没有可用于上架的在线账号', 'error')
      return
    }
    setRunning(true)
    setResults([])
    setFinished(false)
    // Close immediately — the floating progress bar takes over display
    onClose()
    const totalOps = items.length * selectedUids.length
    setProgress({ done: 0, total: totalOps, current: '准备中…' })
    taskProgress.start(totalOps, '上架')

    const helperUid = selectedUids.find((uid) => availableAccounts.some((a) => a.uid === uid)) || availableAccounts[0]?.uid
    if (!helperUid) {
      setRunning(false)
      notify('没有可用账号，无法导入并上架', 'error')
      return
    }
    const allResults: ListResult[] = []
    let doneCount = 0
    const successUids = new Set<string>()

    for (const item of items) {
      setProgress(p => ({ ...p, current: `导入: ${item.title.slice(0, 20)}…` }))

      // Import item to get full details (desc, images, etc.)
      let importData: any = null
      try {
        const r: any = await itemImportLink(
          helperUid,
          `https://www.goofish.com/item?id=${item.itemId}`,
        )
        importData = r?.data ?? r
      } catch {
        // Fallback to search result data
        importData = {
          title: item.title,
          price: item.price,
          imageUrls: item.picUrl ? [item.picUrl] : [],
        }
      }

      const rawTitle = String(importData?.title || item.title)
      const title = normalizeBatchPublishTitle(rawTitle)
      const desc = String(importData?.desc || '')
      const origPrice = String(importData?.origPrice || '')
      // imageUrls: prefer imported URLs, fallback to re-upload picUrl via CDN
      let imageUrls = Array.isArray(importData?.imageUrls)
        ? importData.imageUrls.filter(Boolean)
        : []
      if (!imageUrls.length && item.picUrl) {
        // Re-upload the search result image to Xianyu CDN so publish won't reject it
        try {
          const upResp: any = await uploadImageUrl(helperUid, item.picUrl)
          if (upResp?.url || upResp?.data?.url) {
            imageUrls = [String(upResp.url || upResp.data.url)]
          } else {
            imageUrls = [item.picUrl]
          }
        } catch {
          imageUrls = [item.picUrl]
        }
      }
      const finalPrice = calcPrice(
        String(importData?.price || item.price),
        priceMode, adjustVal, fixedVal,
      )

      // Publish to each selected account
      for (const uid of selectedUids) {
        const account = availableAccounts.find(a => a.uid === uid)
        const nick = account?.nick || uid.slice(0, 8)
        setProgress(p => ({
          ...p, current: `上架: ${rawTitle.slice(0, 12)}… → ${nick}`,
        }))

        try {
          const publishable = await ensurePublishable(uid)
          if (publishable.checkResp && !publishable.ok) {
            allResults.push({
              itemId: item.itemId, itemTitle: title, uid, nick,
              ok: false, msg: publishCheckMessage(publishable.checkResp, '暂不可发布'),
            })
            doneCount++
            setResults([...allResults])
            setProgress(p => ({ ...p, done: doneCount }))
            taskProgress.tick(doneCount, allResults.map(toTaskProgressResult))
            continue
          }

          const loc = account?.location || {}
          const r: any = await itemPublish(uid, {
            title, desc, price: finalPrice, origPrice, imageUrls,
            simpleItem: resolveSimpleItemFlag(importData?.simpleItem, null, {
              title,
              desc,
            }),
            noBargain: importData?.noBargain ?? false,
            canFreeShipping: shippingType === 'free',
            shippingType, quantity: 1,
            province: String(loc.province || ''),
            city: String(loc.city || ''),
            area: String(loc.area || ''),
            divisionId: String(loc.divisionId || ''),
            latitude: String(loc.latitude || ''),
            longitude: String(loc.longitude || ''),
          })

          const ok = Array.isArray(r?.ret)
            ? String(r.ret[0] || '').startsWith('SUCCESS')
            : !!r?.success
          if (ok) {
            const extraRun = await applyPostPublishExtras({
              uid,
              publishResponse: r,
              price: finalPrice,
              coinEnabled: enableCoin,
              preferredCoinPercent: parseInt(coinPct, 10) || 5,
              grouponEnabled,
              grouponAmount: grouponAmt,
              grouponDelayMs: 2000,
            })
            const extras = extraRun.results.map((entry: any) => entry.label)
            allResults.push({
              itemId: item.itemId, itemTitle: title, uid, nick,
              ok: true, msg: extras.length ? `成功 (${extras.join(' ')})` : '成功',
            })
            successUids.add(uid)
          } else {
            const errMsg = String(r?.ret?.[0] || r?.detail || '发布失败').replace(/^[A-Z_]+::/, '')
            allResults.push({ itemId: item.itemId, itemTitle: title, uid, nick, ok: false, msg: errMsg })
          }
        } catch (e: any) {
          allResults.push({
            itemId: item.itemId, itemTitle: title, uid, nick,
            ok: false, msg: e?.message || '网络错误',
          })
        }

        doneCount++
        setResults([...allResults])
        setProgress(p => ({ ...p, done: doneCount }))
        taskProgress.tick(doneCount, allResults.map(toTaskProgressResult))
      }
    }

    if (successUids.size > 0) {
      await Promise.allSettled(
        [...successUids].map((uid) => syncItems(uid)),
      )
      emitItemsSynced({ uids: [...successUids], source: 'search-batch-publish' })
    }

    setFinished(true)
    setRunning(false)
    setProgress({ done: totalOps, total: totalOps, current: '完成' })
    taskProgress.finish(allResults.map(toTaskProgressResult))
  }

  const okCount = results.filter(r => r.ok).length
  const failCount = results.filter(r => !r.ok).length

  const priceModes: { v: PriceMode; l: string }[] = [
    { v: 'original', l: '原价' },
    { v: 'markup', l: '加价%' },
    { v: 'markdown', l: '减价%' },
    { v: 'fixed', l: '固定价' },
  ]

  return (
    <div
      className="liquid-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      onClick={onClose}
    >
      <div
        className="liquid-panel w-full max-w-[640px] rounded-[26px] overflow-hidden flex flex-col"
        style={{ maxHeight: '90vh' }}
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b shrink-0"
          style={{ borderColor: 'var(--control-border)' }}>
          <div className="flex items-center gap-2">
            <ShoppingBag className="w-4 h-4" style={{ color: 'var(--primary)' }} />
            <span className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>
              一键上架
            </span>
            <span className="text-[11px]" style={{ color: 'var(--text-muted)' }}>
              · {items.length} 件商品
            </span>
          </div>
          <button onClick={onClose} className="mobile-icon-button p-1 rounded-full cursor-pointer">
            <X className="w-4 h-4" style={{ color: 'var(--text-muted)' }} />
          </button>
        </div>

        <div className="px-4 py-3 space-y-3 overflow-auto">
          {/* Selected items preview */}
          <div>
            <div className="text-[11.5px] mb-1.5" style={{ color: 'var(--text-muted)' }}>
              已选商品
            </div>
            <div className="flex gap-2 overflow-x-auto pb-1 no-scrollbar">
              {items.map(item => (
                <div key={item.itemId}
                  className="liquid-muted-card shrink-0 w-[100px] rounded-[14px] overflow-hidden">
                  <div className="aspect-square bg-[var(--surface-soft)]">
                    {item.picUrl
                      ? <SensitiveItemImage src={item.picUrl} alt="" className="w-full h-full object-cover" />
                      : <div className="w-full h-full flex items-center justify-center" style={{ color: 'var(--text-tertiary)' }}>—</div>}
                  </div>
                  <div className="px-1.5 py-1">
                    <p className="text-[10px] leading-tight line-clamp-1" style={{ color: 'var(--text-main)' }}>
                      <SensitiveValue kind="item-title" value={item.title} />
                    </p>
                    <p className="text-[11px] font-bold mt-0.5" style={{ color: 'var(--danger)' }}>
                      ¥<SensitiveValue kind="money" value={item.price} />
                      {priceMode !== 'original' ? (
                        <span className="text-[9px] font-normal ml-1" style={{ color: 'var(--primary)' }}>
                          → ¥<SensitiveValue kind="money" value={calcPrice(item.price, priceMode, adjustVal, fixedVal)} />
                        </span>
                      ) : null}
                    </p>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* Target accounts — 2~3 column grid */}
          <div>
            <div className="flex items-center justify-between mb-1">
              <span className="text-[11.5px]" style={{ color: 'var(--text-muted)' }}>
                上架到 · 已选 {selectedUids.length}/{availableAccounts.length}
              </span>
              <button type="button" onClick={toggleAllUids}
                className="text-[11px] font-medium cursor-pointer"
                style={{ color: 'var(--primary)' }}>
                {selectedUids.length === availableAccounts.length && availableAccounts.length > 0 ? '取消全选' : '全选'}
              </button>
            </div>
            <div className="liquid-list-panel rounded-[14px] overflow-y-auto p-2"
              style={{ maxHeight: 140 }}>
              <div className="grid grid-cols-2 md:grid-cols-3 gap-1.5">
                {availableAccounts.map(a => {
                  const checked = selectedUids.includes(a.uid)
                  const fp = Number(a.security_score) || 0
                  return (
                    <label key={a.uid}
                      className="flex items-center gap-1.5 px-2 py-[5px] rounded-[6px] cursor-pointer transition-colors"
                      style={{
                        background: checked ? 'var(--control-active)' : 'var(--surface-control)',
                        border: `1px solid ${checked ? 'color-mix(in srgb, var(--primary) 22%, transparent)' : 'var(--control-border)'}`,
                      }}>
                      <input type="checkbox" checked={checked}
                        onChange={() => toggleUid(a.uid)}
                        className="accent-[var(--primary)] shrink-0" />
                      <span className="text-[12px] truncate flex-1 min-w-0" style={{ color: checked ? 'var(--primary)' : 'var(--text-main)' }}>
                        <SensitiveValue kind="account" value={a.nick || a.uid} />
                      </span>
                      {fp > 0 ? (
                        <span className="shrink-0 text-[10px] font-mono tabular-nums"
                          style={{ color: fp >= 700 ? 'var(--success)' : fp >= 500 ? 'var(--warning)' : 'var(--text-muted)' }}>
                          {fp}
                        </span>
                      ) : null}
                    </label>
                  )
                })}
              </div>
            </div>
          </div>

          {/* Price strategy */}
          <div>
            <div className="text-[11.5px] mb-1.5" style={{ color: 'var(--text-muted)' }}>定价策略</div>
            <div className="flex gap-2 items-center flex-wrap">
              {priceModes.map(m => (
                <button key={m.v} onClick={() => setPriceMode(m.v)}
                  className="px-3 py-[5px] rounded-[8px] text-[12px] font-medium cursor-pointer transition-colors"
                  style={{
                    background: priceMode === m.v ? 'var(--primary)' : 'var(--surface-soft)',
                    color: priceMode === m.v ? 'var(--liquid-highlight)' : 'var(--text-main)',
                    border: `1px solid ${priceMode === m.v ? 'var(--primary)' : 'var(--control-border)'}`,
                  }}>
                  {m.l}
                </button>
              ))}
              {(priceMode === 'markup' || priceMode === 'markdown') ? (
                <div className="flex items-center gap-1">
                  <input value={adjustVal}
                    onChange={e => setAdjustVal(e.target.value.replace(/[^\d.]/g, ''))}
                    className="w-[52px] rounded-[6px] px-2 py-[4px] text-[12px] text-center font-mono"
                    style={{ border: '1px solid var(--control-border)', background: 'var(--surface-control)' }} />
                  <span className="text-[12px]" style={{ color: 'var(--text-muted)' }}>%</span>
                </div>
              ) : null}
              {priceMode === 'fixed' ? (
                <div className="flex items-center gap-1">
                  <span className="text-[12px]" style={{ color: 'var(--text-muted)' }}>¥</span>
                  <input value={fixedVal}
                    onChange={e => setFixedVal(e.target.value.replace(/[^\d.]/g, ''))}
                    placeholder="0.00"
                    className="w-[72px] rounded-[6px] px-2 py-[4px] text-[12px] text-center font-mono"
                    style={{ border: '1px solid var(--control-border)', background: 'var(--surface-control)' }} />
                </div>
              ) : null}
            </div>
          </div>

          {/* Options row */}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
            <div className="flex items-center gap-2">
              <span className="text-[11.5px]" style={{ color: 'var(--text-muted)' }}>发货</span>
              {(['free', 'fixed', 'self'] as const).map(v => (
                <button key={v} onClick={() => setShippingType(v)}
                  className="px-2 py-[3px] rounded-full text-[11px] font-medium cursor-pointer"
                  style={{
                    background: shippingType === v ? 'var(--primary)' : 'var(--surface-soft)',
                    color: shippingType === v ? 'var(--liquid-highlight)' : 'var(--text-main)',
                    border: `1px solid ${shippingType === v ? 'var(--primary)' : 'var(--control-border)'}`,
                  }}>
                  {{ free: '包邮', fixed: '固定运费', self: '无需邮寄' }[v]}
                </button>
              ))}
            </div>
            <label className="flex items-center gap-1.5 text-[12px] cursor-pointer" style={{ color: 'var(--primary)' }}>
              <input type="checkbox" checked={enableCoin}
                onChange={e => setEnableCoin(e.target.checked)} className="accent-[var(--primary)]" />
              闲鱼币
            </label>
            {enableCoin ? (
              <select value={coinPct} onChange={e => setCoinPct(e.target.value)}
                className="rounded-[6px] px-1.5 py-0.5 text-[11px]"
                style={{ border: '1px solid var(--primary)', color: 'var(--primary)', background: 'var(--primary-light)' }}>
                {['3', '5', '8', '10', '15', '20'].map(v => (
                  <option key={v} value={v}>{v}%</option>
                ))}
              </select>
            ) : null}
            <label className="flex items-center gap-1.5 text-[12px] cursor-pointer" style={{ color: 'var(--primary)' }}>
              <input type="checkbox" checked={grouponEnabled}
                onChange={e => setGrouponEnabled(e.target.checked)} className="accent-[var(--primary)]" />
              2人小刀
            </label>
            {grouponEnabled ? (
              <div className="flex items-center gap-1">
                <span className="text-[11px]" style={{ color: 'var(--primary)' }}>¥</span>
                <input value={grouponAmt}
                  onChange={e => setGrouponAmt(e.target.value.replace(/[^\d.]/g, ''))}
                  placeholder="自动"
                  className="w-[48px] rounded-[6px] px-1.5 py-0.5 text-[11px] text-center"
                  style={{ border: '1px solid var(--primary)', color: 'var(--primary)', background: 'var(--primary-light)' }} />
              </div>
            ) : null}
          </div>

          {/* Progress / Results */}
          {running || finished ? (
            <div className="space-y-2">
              {/* Progress bar */}
              <div className="rounded-full overflow-hidden h-2" style={{ background: 'var(--surface-soft)' }}>
                <div className="h-full transition-all duration-300 rounded-full"
                  style={{
                    width: progress.total ? `${(progress.done / progress.total) * 100}%` : '0%',
                    background: finished
                      ? (failCount > 0 ? 'var(--warning)' : 'var(--success)')
                      : 'var(--primary)',
                  }} />
              </div>
              <div className="text-[11px] flex items-center justify-between" style={{ color: 'var(--text-muted)' }}>
                <span>{progress.current || `${progress.done}/${progress.total}`}</span>
                {finished ? (
                  <span>
                    <span style={{ color: 'var(--success)' }}>{okCount} 成功</span>
                    {failCount > 0 ? <span style={{ color: 'var(--danger)' }}> · {failCount} 失败</span> : null}
                  </span>
                ) : null}
              </div>

              {/* Result list */}
              {results.length > 0 ? (
                <div className="liquid-list-panel rounded-[14px] overflow-hidden max-h-[180px] overflow-y-auto">
                  {results.map((r, i) => (
                    <div key={i} className="flex items-center gap-2 px-2.5 py-[6px] text-[11px]"
                      style={{ borderBottom: '1px solid var(--control-border)' }}>
                      {r.ok
                        ? <CheckCircle2 className="w-3.5 h-3.5 shrink-0" style={{ color: 'var(--success)' }} />
                        : <XCircle className="w-3.5 h-3.5 shrink-0" style={{ color: 'var(--danger)' }} />}
                      <span className="truncate flex-1" style={{ color: 'var(--text-main)' }}>
                        <SensitiveValue kind="item-title" value={`${r.itemTitle.slice(0, 15)}…`} /> → <SensitiveValue kind="account" value={r.nick} />
                      </span>
                      <span className="shrink-0" style={{ color: r.ok ? 'var(--success)' : 'var(--danger)' }}>
                        {r.msg}
                      </span>
                    </div>
                  ))}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t shrink-0"
          style={{ borderColor: 'var(--control-border)', background: 'rgba(255,255,255,0.42)' }}>
          <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>
            {items.length} 件 × {selectedUids.length} 个账号 = {items.length * selectedUids.length} 次发布
          </div>
          <div className="flex items-center gap-2">
            <button onClick={onClose} className="btn-ghost btn-sm cursor-pointer">
              {finished ? '关闭' : '取消'}
            </button>
            {!finished ? (
              <button onClick={doSubmit}
                disabled={running || !selectedUids.length || !items.length}
                className="btn-primary btn-sm cursor-pointer disabled:opacity-40">
                {running
                  ? <><RefreshCw className="w-3.5 h-3.5 animate-spin" /> 上架中…</>
                  : `一键上架 ${items.length} 件`}
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  )
}
