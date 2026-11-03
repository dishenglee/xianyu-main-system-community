import { useEffect, useRef, useState } from 'react'
import { BarChart3, RefreshCw, Store, TrendingUp } from 'lucide-react'
import { getDatacompassFlow, getDatacompassItems, getDatacompassSummary } from '../../api/shopGrowth'
import type { DatacompassDateType } from '../../api/types'
import {
  presentFishShopFlow,
  presentFishShopItemRanking,
  presentFishShopSummary,
} from './fishShopMetricPresentation'

const DATE_OPTIONS: Array<{ value: DatacompassDateType; label: string }> = [
  { value: 'recent1d', label: '最近 1 天' },
  { value: 'recent7d', label: '最近 7 天' },
  { value: 'recent30d', label: '最近 30 天' },
]

export function FishShopDataCompassPanel({ uid }: { uid: string }) {
  const [dateType, setDateType] = useState<DatacompassDateType>('recent7d')
  const [summary, setSummary] = useState<unknown>(null)
  const [flow, setFlow] = useState<unknown>(null)
  const [items, setItems] = useState<unknown>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const latestRequestRef = useRef(0)

  async function load(signal?: AbortSignal) {
    const requestId = ++latestRequestRef.current
    setSummary(null)
    setFlow(null)
    setItems(null)
    setError('')
    if (!uid) {
      setLoading(false)
      return
    }
    setLoading(true)
    try {
      const [nextSummary, nextFlow, nextItems] = await Promise.all([
        getDatacompassSummary(uid, dateType, { signal }),
        getDatacompassFlow(uid, dateType, { signal }),
        getDatacompassItems(uid, dateType, 1, 10, { signal }),
      ])
      if (requestId !== latestRequestRef.current) return
      setSummary(nextSummary)
      setFlow(nextFlow)
      setItems(nextItems)
    } catch (err) {
      if (err instanceof Error && err.name === 'AbortError') return
      if (requestId === latestRequestRef.current) {
        setError(err instanceof Error ? err.message : '加载鱼小铺经营数据失败')
      }
    } finally {
      if (requestId === latestRequestRef.current) setLoading(false)
    }
  }

  useEffect(() => {
    const controller = new AbortController()
    void load(controller.signal)
    return () => {
      controller.abort()
      latestRequestRef.current += 1
    }
  }, [uid, dateType])

  const metrics = presentFishShopSummary(summary)
  const flowMetrics = presentFishShopFlow(flow)
  const itemRows = presentFishShopItemRanking(items).slice(0, 8)

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-lg font-semibold flex items-center gap-2">
            <Store className="w-5 h-5 text-emerald-500" />
            鱼小铺经营
          </div>
          <p className="text-sm text-muted mt-1">经营概览、流量转化、商品排行和客服售后数据</p>
        </div>
        <div className="flex items-center gap-2">
          <div className="segmented-control p-1 rounded-xl bg-surface border border-border">
            {DATE_OPTIONS.map((option) => (
              <button
                key={option.value}
                className={`px-3 py-1.5 rounded-lg text-sm ${dateType === option.value ? 'bg-primary text-white' : 'text-muted'}`}
                onClick={() => setDateType(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
          <button className="btn-secondary px-3 py-2 rounded-xl text-sm" onClick={() => void load()} disabled={loading}>
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
          </button>
        </div>
      </div>

      {error ? <div className="rounded-2xl border border-red-200 bg-red-50 p-3 text-sm text-red-700">{error}</div> : null}

      <div className="grid md:grid-cols-4 gap-3">
        {metrics.length ? metrics.map((metric) => (
          <div key={metric.key} className="rounded-2xl border border-border bg-surface p-4">
            <div className="text-xs text-muted">{metric.label}</div>
            <div className="text-2xl font-semibold mt-2">{metric.valueText}</div>
            <div className="text-xs text-muted mt-2">{metric.comparison}</div>
          </div>
        )) : (
          <div className="md:col-span-4 rounded-2xl border border-border bg-surface p-6 text-sm text-muted text-center">
            {loading ? '正在加载经营概览...' : '暂无经营概览数据'}
          </div>
        )}
      </div>

      <div className="grid lg:grid-cols-2 gap-4">
        <section className="rounded-2xl border border-border bg-surface p-4">
          <div className="text-sm font-semibold flex items-center gap-2 mb-3">
            <TrendingUp className="w-4 h-4 text-sky-500" />
            流量转化
          </div>
          <div className="space-y-2">
            {flowMetrics.length ? flowMetrics.map((metric) => (
              <div key={metric.key} className="flex items-center justify-between rounded-xl bg-background/70 p-3">
                <span className="text-sm text-muted">{metric.label}</span>
                <span className="text-sm font-semibold">{metric.valueText}</span>
              </div>
            )) : <div className="text-sm text-muted text-center py-8">暂无流量转化数据</div>}
          </div>
        </section>

        <section className="rounded-2xl border border-border bg-surface p-4">
          <div className="text-sm font-semibold flex items-center gap-2 mb-3">
            <BarChart3 className="w-4 h-4 text-orange-500" />
            商品排行
          </div>
          <div className="space-y-2">
            {itemRows.length ? itemRows.map((row, index) => (
                <div key={row.key} className="flex items-center justify-between gap-3 rounded-xl bg-background/70 p-3">
                  <div className="min-w-0">
                    <div className="text-sm font-medium truncate">{row.title}</div>
                    {row.metrics.length ? (
                      <div className="text-xs text-muted mt-1">{row.metrics.map((metric) => `${metric.label} ${metric.valueText}`).join(' · ')}</div>
                    ) : null}
                  </div>
                  <div className="text-xs text-muted">#{index + 1}</div>
                </div>
            )) : <div className="text-sm text-muted text-center py-8">暂无商品排行数据</div>}
          </div>
        </section>
      </div>
    </div>
  )
}
