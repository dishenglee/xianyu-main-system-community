import { useEffect, useState } from 'react'
import { accountCoinDetail, accountSecurityScoreRecords, accountSellerData } from '../api/index'
import { getAuthToken } from '../api/client'

export interface Point { t: number; v: number; label?: string }

export interface AccountSeries {
  loading: boolean
  coin: Point[]          // 闲鱼币余额历史（基于当前 + 流水反推）
  security: Point[]      // 安全分历史（基于 score 变动记录）
  sellerByPeriod: Record<string, any>  // 1d/7d/30d 对应的 seller data payload
}

function parseTime(raw: any): number {
  if (raw == null || raw === '') return 0
  if (typeof raw === 'number') return raw > 1_000_000_000_000 ? raw : raw * 1000
  const s = String(raw)
  const n = Number(s)
  if (!Number.isNaN(n) && n > 0) return n > 1_000_000_000_000 ? n : n * 1000
  const d = new Date(s)
  const t = d.getTime()
  return Number.isNaN(t) ? 0 : t
}

/**
 * 拉账号的三个历史/切片数据，加工成曲线 points。
 *
 * - coin: 从最新余额倒推累加（每条流水的 score 是 delta）
 * - security: 从 aq2.score.record.list 拿到 scoreAfter 作为那一时刻的分值
 * - sellerByPeriod: 1d/7d/30d 周期的 seller.summary 原始 data（前端渲染时按 name 取 value）
 */
export function useAccountSeries(
  uid: string,
  enabled: boolean,
  currentCoin: number,
): AccountSeries {
  const [loading, setLoading] = useState(false)
  const [coin, setCoin] = useState<Point[]>([])
  const [security, setSecurity] = useState<Point[]>([])
  const [sellerByPeriod, setSellerByPeriod] = useState<Record<string, any>>({})

  useEffect(() => {
    if (!enabled || !uid) return
    let cancelled = false
    const requestToken = getAuthToken()
    setLoading(true)

    const coinP = accountCoinDetail(uid, 1, 100).catch(() => null)
    const secP = accountSecurityScoreRecords(uid, 1).catch(() => null)
    const sellerAll = Promise.all(
      (['recent1d', 'recent7d', 'recent30d'] as const).map((dt) =>
        accountSellerData(uid, dt).catch(() => null).then((p) => [dt, p] as const),
      ),
    )

    Promise.all([coinP, secP, sellerAll]).then(([coinResp, secResp, sellerResps]) => {
      if (cancelled || getAuthToken() !== requestToken) return
      // ---- 闲鱼币：把流水反推成余额点 ----
      const coinList = Array.isArray(coinResp?.userScoreDetailDOList)
        ? coinResp.userScoreDetailDOList
        : Array.isArray(coinResp?.data?.userScoreDetailDOList)
          ? coinResp.data.userScoreDetailDOList
          : []
      // 按时间升序
      const coinSorted = [...coinList].sort((a: any, b: any) => parseTime(a.time) - parseTime(b.time))
      const pts: Point[] = []
      // 从总余额（currentCoin）倒推历史：逆序扣除 delta，再正序组织点
      // 更直观做法：起点 = currentCoin - sum(all deltas)，按顺序累加。
      const totalDelta = coinSorted.reduce((s: number, r: any) => s + Number(r.score ?? 0), 0)
      let running = currentCoin - totalDelta
      for (const r of coinSorted) {
        running += Number(r.score ?? 0)
        const t = parseTime(r.time)
        if (t > 0) pts.push({ t, v: Math.round(running), label: String(r.describe ?? '') })
      }
      // 追加"现在"一点
      pts.push({ t: Date.now(), v: currentCoin })
      setCoin(pts)

      // ---- 安全分变动记录 ----
      const secList = Array.isArray(secResp?.data?.data) ? secResp.data.data
        : Array.isArray(secResp?.data?.list) ? secResp.data.list
        : Array.isArray(secResp?.list) ? secResp.list
        : []
      const secPts: Point[] = []
      for (const r of secList) {
        const t = parseTime(r.gmtCreate ?? r.gmtModified ?? r.time)
        const after = Number(r.scoreAfter ?? r.score)
        if (t > 0 && !Number.isNaN(after) && after > 0) {
          secPts.push({ t, v: after, label: String(r.changeTypeName ?? '') })
        }
      }
      secPts.sort((a, b) => a.t - b.t)
      setSecurity(secPts)

      // ---- seller 各周期 ----
      // 真实响应结构（抓包验证）：
      //   后端返回 MTOP 外层 { api, ret, v, data: <inner> }
      //   inner = { code, data: { graphBannerBenchData: {...} }, msg }  ← 再套一层 data
      // 所以要一路 data.data.graphBannerBenchData；兜底还支持 fallback 平铺。
      const byPeriod: Record<string, any> = {}
      for (const [dt, payload] of sellerResps) {
        const banner =
          payload?.data?.data?.graphBannerBenchData
          ?? payload?.data?.graphBannerBenchData
          ?? payload?.graphBannerBenchData
          ?? null
        byPeriod[dt] = banner
      }
      setSellerByPeriod(byPeriod)

      setLoading(false)
    }).catch(() => {
      if (!cancelled && getAuthToken() === requestToken) setLoading(false)
    })

    return () => { cancelled = true }
  }, [uid, enabled, currentCoin])

  return { loading, coin, security, sellerByPeriod }
}

/**
 * 从 bannerDataList 里按 name 取 value，支持多别名 + 中英兼容。
 */
export function pickSellerMetric(banner: any, aliases: string[]): { data: number; ratio: string; cycle: string } {
  const list = banner?.bannerDataList
  if (!Array.isArray(list)) return { data: 0, ratio: '-', cycle: '' }
  for (const alias of aliases) {
    const found = list.find((x: any) => x?.name === alias)
    if (found) {
      return {
        data: Number(found.data ?? parseFloat(String(found.dataStr ?? found.dataFormat ?? '0').replace(/[^\d.-]/g, '')) ?? 0),
        ratio: String(found.ratioFormat ?? '-'),
        cycle: String(found.cycle ?? ''),
      }
    }
  }
  // 模糊匹配（中文 key 或 substring）
  for (const x of list) {
    if (x?.name && aliases.some((a) => String(x.name).includes(a))) {
      return {
        data: Number(x.data ?? parseFloat(String(x.dataStr ?? x.dataFormat ?? '0').replace(/[^\d.-]/g, '')) ?? 0),
        ratio: String(x.ratioFormat ?? '-'),
        cycle: String(x.cycle ?? ''),
      }
    }
  }
  return { data: 0, ratio: '-', cycle: '' }
}
