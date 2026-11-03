// @ts-nocheck
// Pure helpers and the batch-publish orchestrator used by DesktopItems.
// Kept separate to stay within the 800-line file limit.
import {
  itemPublish, publishCheck,
  applyPostPublishExtras,
  refreshAccount,
} from '../../api/index'
import { derivePublishTitle, isPublishAllowed, publishCheckMessage } from './publishUtils'

// ---- Status helper ----
// 三态：'onsale' / 'review' / 'off'。
// 'review'（审核中）来自上游 management.query 的 cardData.itemStatus 非 0
// （后端写到 detail_json.itemStatus）—— 商品已发布、status=ON_SALE，但还在
// 审核流程里。这种商品归属"在售"tab（用户可以下架/删除），UI 加角标提示。
export function statusOf(it) {
  if (it?._fromSoldOut) return 'off'
  if (it._sold) return 'off'
  const normalizedStatus = String(it?.status || '').toUpperCase()
  if (normalizedStatus === 'OFF_SHELF' || normalizedStatus === 'SOLD_OUT') {
    return 'off'
  }
  const detail = (it?.detail && typeof it.detail === 'object') ? it.detail : {}
  const rawItemStatus = Number(detail.itemStatus ?? 0)
  // 审核中：DB 标记 ON_SALE 但 itemStatus != 0 或 online=false（且未售出）。
  if (normalizedStatus === 'ON_SALE' && (rawItemStatus !== 0 || it?.online === false)) {
    return 'review'
  }
  if (it?.online === false) return 'off'
  return 'onsale'
}

// 审核中的商品应当在"在售"tab 一并展示（业务方明确要求）。
// 把这个判定集中到一个函数，KPI / 筛选 / chip 计数共用。
export function isOnSaleListed(it) {
  const s = statusOf(it)
  return s === 'onsale' || s === 'review'
}

// ---- Mtop error helpers ----

// 把 mtop 错误信息净化成用户能看的人话：
//   "non-success upstream ret: FAIL_BIZ_USER_ERROR_USER_WAS_FORBIDDEN2::亲…||{...}"
//   → "亲…"
export function cleanMtopErr(raw: any): string {
  let s = String(raw ?? '').trim()
  if (!s) return '操作失败'
  s = s.replace(/^non-success\s+upstream\s+ret:\s*/i, '')
  s = s.replace(/^[A-Z][A-Z0-9_]+::/, '')
  const jsonStart = s.indexOf('||{')
  if (jsonStart > 0) s = s.slice(0, jsonStart).trim()
  return s || '操作失败'
}

export function isCoinAgreementError(raw: any): boolean {
  const s = String(raw ?? '').toUpperCase()
  return s.includes('NOT_AGREE_DEDUCTION')
    || String(raw ?? '').includes('未同意抵扣协议')
    || String(raw ?? '').includes('未同意闲鱼币抵扣协议')
    || String(raw ?? '').includes('抵扣协议')
}

// 闲鱼 mtop 网关 200 不代表业务成功，看 ret 数组首项是否 SUCCESS::。
export function mtopOutcome(r: any): { ok: boolean; msg: string; code?: string } {
  if (!r || typeof r !== 'object') return { ok: false, msg: '响应格式异常' }
  if (r.detail && r.status === 'error') {
    const raw = String(r.detail)
    return { ok: false, msg: cleanMtopErr(raw), code: raw.match(/\b(FAIL_[A-Z0-9_]+)\b/i)?.[1]?.toUpperCase() }
  }
  const data = r.data
  if (data && typeof data === 'object') {
    if ('result' in data && data.result === false) {
      const raw = String(data.message || data.msg || '操作失败')
      return { ok: false, msg: cleanMtopErr(raw), code: raw.match(/\b(FAIL_[A-Z0-9_]+)\b/i)?.[1]?.toUpperCase() }
    }
    if ('success' in data) {
      const raw = String(data.message || data.msg || (data.success ? '操作成功' : '操作失败'))
      return {
        ok: !!data.success,
        msg: cleanMtopErr(raw),
        code: data.success ? undefined : raw.match(/\b(FAIL_[A-Z0-9_]+)\b/i)?.[1]?.toUpperCase(),
      }
    }
  }
  const ret = Array.isArray(r.ret) ? String(r.ret[0] || '') : String(r.retCode || '')
  if (ret.startsWith('SUCCESS::')) return { ok: true, msg: ret.slice('SUCCESS::'.length) || '操作成功' }
  if (ret) return {
    ok: false,
    msg: cleanMtopErr(ret),
    code: ret.match(/\b(FAIL_[A-Z0-9_]+)\b/i)?.[1]?.toUpperCase(),
  }
  return { ok: true, msg: '操作完成' }
}

// ---- Batch publish orchestrator ----

export interface BatchPublishParams {
  selectedUids: string[]
  form: any
  imageUrls: string[]
  cardList?: any[]
  propSelections?: Record<string, string>
  coinEnabled: boolean
  coinDeductionPercent: string
  grouponEnabled: boolean
  grouponAmount: string
}

export interface BatchPublishResult {
  uid: string
  ok: boolean
  msg: string
}

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

export async function runBatchPublishJobs(
  params: BatchPublishParams,
  activeAccounts: any[],
  taskProgressStore: { start: (n: number, title?: string) => void; tick: (i: number, results: BatchPublishResult[]) => void; finish: (results: BatchPublishResult[]) => void },
  doSync: (uid: string) => Promise<void>,
): Promise<void> {
  const {
    selectedUids,
    form,
    imageUrls,
    cardList = [],
    propSelections = {},
    coinEnabled,
    coinDeductionPercent,
    grouponEnabled,
    grouponAmount,
  } = params
  const results: BatchPublishResult[] = []
  taskProgressStore.start(selectedUids.length, '发布')

  for (let i = 0; i < selectedUids.length; i += 1) {
    const uid = selectedUids[i]
    const account = activeAccounts.find((a: any) => a.uid === uid)
    const loc = account?.location || {}
    const effectiveTitle = derivePublishTitle(form.title, form.desc) || '闲置商品'
    try {
      const publishable = await ensurePublishable(uid)
      if (publishable.checkResp && !publishable.ok) {
        results.push({ uid, ok: false, msg: publishCheckMessage(publishable.checkResp, '暂不可发布') })
        taskProgressStore.tick(i + 1, [...results])
        continue
      }
      const r: any = await itemPublish(uid, {
        title: effectiveTitle, desc: form.desc,
        price: String(form.price), origPrice: String(form.origPrice || ''),
        imageUrls, simpleItem: form.simpleItem, noBargain: form.noBargain,
        shippingType: form.shippingType, postPrice: form.postPrice,
        freightTemplateId: form.freightTemplateId, quantity: form.quantity || 1,
        cardList,
        propSelections,
        province: String(form.province || loc.province),
        city: String(form.city || loc.city),
        area: String(form.area || loc.area),
        divisionId: String(form.divisionId || loc.divisionId),
        latitude: String(form.latitude || loc.latitude),
        longitude: String(form.longitude || loc.longitude),
      })
      const ok = Array.isArray(r?.ret) ? String(r.ret[0] || '').startsWith('SUCCESS') : !!r?.success
      if (ok) {
        const extraRun = await applyPostPublishExtras({
          uid,
          publishResponse: r,
          price: String(form.price || '100'),
          coinEnabled,
          preferredCoinPercent: parseInt(coinDeductionPercent, 10) || 5,
          grouponEnabled,
          grouponAmount,
          grouponDelayMs: 2000,
        })
        const extras = extraRun.results.map((entry: any) => entry.ok ? `${entry.label}✓` : entry.label)
        results.push({ uid, ok: true, msg: extras.length ? `成功 (${extras.join(' ')})` : '成功' })
      } else {
        results.push({ uid, ok: false, msg: String(r?.ret?.[0] || '发布失败') })
      }
    } catch (e: any) {
      results.push({ uid, ok: false, msg: e?.message || '网络错误' })
    }
    taskProgressStore.tick(i + 1, [...results])
  }

  const successUids = [...new Set(results.filter((r) => r.ok).map((r) => r.uid))]
  if (successUids.length > 0) {
    for (const uid of successUids) await doSync(uid).catch(() => {})
  }
  taskProgressStore.finish([...results])
}
