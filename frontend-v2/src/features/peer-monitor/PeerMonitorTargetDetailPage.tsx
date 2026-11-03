import { useCallback, useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { Activity, ArrowLeft, Clock3, Package, Pause, Play, RefreshCw, Star, UserRound, type LucideIcon } from 'lucide-react'
import { getPeerMonitorTarget, refreshPeerMonitorTarget, updatePeerMonitorTarget } from '../../api/peerMonitor'
import { EmptyState } from '../../components/EmptyState'
import { SafeRemoteImage } from '../messages/SafeRemoteImage'
import { PeerMonitorTabs } from './PeerMonitorTabs'
import { Avatar } from './PeerMonitorPage'
import { eventMeta, formatTime, listFromResponse, statusMeta, valueOf, type PeerRecord, type PeerValue } from './peerMonitorUtils'

const asRecord = (value: unknown): PeerRecord => value && typeof value === 'object' ? value as PeerRecord : {}
const textValue = (value: PeerValue): string => value == null ? '' : String(value)

export default function PeerMonitorTargetDetailPage() {
  const { targetId = '' } = useParams()
  const [target, setTarget] = useState<PeerRecord | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const load = useCallback(async () => {
    setLoading(true)
    try { setTarget(asRecord(await getPeerMonitorTarget(targetId))); setError('') }
    catch { setError('监控详情暂时无法加载，请稍后重试') }
    finally { setLoading(false) }
  }, [targetId])
  useEffect(() => { const timer = window.setTimeout(() => { void load() }, 0); return () => window.clearTimeout(timer) }, [load])
  const collect = async () => {
    setBusy(true)
    try { await refreshPeerMonitorTarget(targetId); await load() }
    catch { setError('同行公开数据采集失败，请稍后再试') }
    finally { setBusy(false) }
  }
  const toggle = async () => {
    setBusy(true)
    try { await updatePeerMonitorTarget(targetId, { status: target?.status === 'paused' ? 'active' : 'paused' }); await load() }
    catch { setError('监控状态更新失败，请稍后再试') }
    finally { setBusy(false) }
  }
  const items = listFromResponse(target, ['items', 'snapshots', 'itemList'])
  const timeline = listFromResponse(target, ['timeline', 'timelineItems'])
  const reviews = listFromResponse(target, ['reviews', 'rates'])
  const events = listFromResponse(target, ['events', 'changes', 'eventList'])
  const runs = listFromResponse(target, ['runs', 'refreshHistory', 'history'])
  const status = target?.snapshotState === 'pending' ? statusMeta('pending') : statusMeta(target?.status)
  const sourceItem = asRecord(target?.sourceItem)
  const sourceItemId = valueOf(target, 'sourceItemId', 'source_item_id')
  const lastSuccessAt = valueOf(target, 'lastSuccessAt')
  const sellerName = valueOf(target, 'sellerNick', 'sellerName', 'nickname') || '同行账号'
  const area = valueOf(target, 'area', 'location')
  const summary = valueOf(target, 'sellerSummary')
  return <div className="h-full flex flex-col"><PeerMonitorTabs />
    <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6"><div className="mx-auto max-w-6xl space-y-4">
      <Link to="/peer-monitor" className="inline-flex items-center gap-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}><ArrowLeft className="w-3.5 h-3.5" />返回监控数据</Link>
      {error ? <div className="rounded-xl px-3 py-2 text-[12px]" style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}>{error}</div> : null}
      {loading ? <div className="py-24 text-center text-[13px]" style={{ color: 'var(--text-secondary)' }}><RefreshCw className="w-4 h-4 animate-spin inline mr-2" />加载详情…</div> : target ? <>
        <section className="surface-card rounded-2xl p-5 flex flex-col gap-4 md:flex-row md:items-center md:justify-between"><div className="flex min-w-0 items-center gap-3"><Avatar src={valueOf(target, 'sellerAvatar', 'avatar')} name={sellerName} size="w-12 h-12" /><div className="min-w-0"><h1 className="truncate text-[18px] font-semibold" style={{ color: 'var(--text-main)' }}>{sellerName}</h1><p className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>{[area, valueOf(target, 'sellerLevel'), valueOf(target, 'shopLevel')].filter(Boolean).join(' · ') || '账号公开主页'}</p>{summary ? <p className="mt-1 truncate text-[11px]" style={{ color: 'var(--text-tertiary)' }}>{summary}</p> : null}<p className="mt-1 text-[10px]" style={{ color: 'var(--text-tertiary)' }}>最近成功采集 {formatTime(lastSuccessAt)}</p></div></div><div className="flex items-center gap-2"><StatusPill meta={status} /><button type="button" disabled={busy} onClick={() => void toggle()} className="rounded-lg p-2" style={{ color: 'var(--text-secondary)', background: 'var(--surface-control)' }}>{target.status === 'paused' ? <Play className="w-4 h-4" /> : <Pause className="w-4 h-4" />}</button><button type="button" disabled={busy} onClick={() => void collect()} className="btn-primary px-3 py-2 text-[12px] inline-flex items-center gap-1.5"><RefreshCw className={`w-3.5 h-3.5 ${busy ? 'animate-spin' : ''}`} />立即采集</button></div></section>
        <section className="grid grid-cols-3 gap-3"><Metric icon={Package} label="公开宝贝" value={items.length} /><Metric icon={Activity} label="公开动态" value={timeline.length} /><Metric icon={Star} label="公开评价" value={reviews.length} /></section>
        <section className="surface-card rounded-2xl overflow-hidden"><SectionTitle icon={Package} title="全部公开宝贝" helper="展示最近一次成功采集到的在售公开宝贝。" />{items.length ? <div className="grid grid-cols-2 md:grid-cols-4 gap-px" style={{ background: 'var(--border-subtle)' }}>{items.map((item, index) => <ItemCard key={String(valueOf(item, 'itemId', 'id') || index)} item={item} />)}</div> : <EmptyState icon={Package} title="尚无公开宝贝数据" description="立即采集成功后，将在这里显示同行当前公开在售的宝贝。" />}</section>
        <div className="grid gap-4 lg:grid-cols-2"><section className="surface-card rounded-2xl overflow-hidden"><SectionTitle icon={Activity} title="公开动态" helper="只展示已成功采集到的上架、售出和下架动态。" />{timeline.length ? <div>{timeline.map((entry, index) => <TimelineRow key={String(valueOf(entry, 'eventKey', 'id') || index)} entry={entry} />)}</div> : <EmptyState icon={Activity} title="尚无公开动态" description="该同行没有可展示的已采集动态。" />}</section><section className="surface-card rounded-2xl overflow-hidden"><SectionTitle icon={Star} title="公开评价" helper="仅展示持久化保存的公开评价及其真实商品归属。" />{reviews.length ? <div>{reviews.map((review, index) => <ReviewRow key={String(valueOf(review, 'id', 'rateId') || index)} review={review} />)}</div> : <EmptyState icon={Star} title="尚无公开评价" description="采集到公开评价后会在这里显示。" />}</section></div>
        <div className="grid gap-4 lg:grid-cols-[1.2fr_.8fr]"><section className="surface-card rounded-2xl overflow-hidden"><SectionTitle icon={Activity} title="最近变化" helper="首次采集建立基线；后续比较成功快照后才记录真实变化。" />{events.length ? <div>{events.map((event, index) => <EventRow key={String(valueOf(event, 'id') || index)} event={event} />)}</div> : <EmptyState icon={Activity} title="还没有变化记录" description="下一次成功采集后，如公开资料、宝贝、动态或评价发生变化，会在这里显示。" />}</section><section className="surface-card rounded-2xl overflow-hidden"><SectionTitle icon={Clock3} title="采集记录" helper="每次采集都保留真实结果与已获取的数据数量。" />{runs.length ? <div>{runs.slice(0, 12).map((run, index) => <RunRow key={String(valueOf(run, 'id') || index)} run={run} />)}</div> : <p className="px-4 pb-5 text-[12px]" style={{ color: 'var(--text-secondary)' }}>尚无采集记录。</p>}</section></div>
        {sourceItemId ? <section className="surface-card rounded-2xl p-4"><div className="flex items-center gap-3"><SourceThumbnail source={sourceItem} /><div className="min-w-0"><p className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>发现来源</p><p className="mt-1 truncate text-[12px] font-medium" style={{ color: 'var(--text-main)' }}>{valueOf(sourceItem, 'title') || `来源商品 #${sourceItemId}`}</p></div></div></section> : null}
      </> : null}</div></div></div>
}

type StatusMeta = ReturnType<typeof statusMeta>
function Metric({ icon: Icon, label, value }: { icon: LucideIcon; label: string; value: number }) { return <div className="surface-card rounded-xl p-3"><Icon className="w-4 h-4" style={{ color: 'var(--primary)' }} /><p className="mt-3 text-[18px] font-semibold truncate" style={{ color: 'var(--text-main)' }}>{value}</p><p className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>{label}</p></div> }
function SectionTitle({ icon: Icon, title, helper }: { icon: LucideIcon; title: string; helper: string }) { return <div className="px-4 py-4 border-b" style={{ borderColor: 'var(--border-subtle)' }}><div className="flex gap-2 items-center"><Icon className="w-4 h-4" style={{ color: 'var(--primary)' }} /><h2 className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>{title}</h2></div><p className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>{helper}</p></div> }
function StatusPill({ meta }: { meta: StatusMeta }) { const colors = meta.tone === 'success' ? ['var(--status-success-bg)', 'var(--status-success-text)'] : meta.tone === 'warning' ? ['var(--status-warning-bg)', 'var(--status-warning-text)'] : meta.tone === 'danger' ? ['var(--status-danger-bg)', 'var(--status-danger-text)'] : ['var(--surface-control)', 'var(--text-secondary)']; return <span className="rounded-full px-2.5 py-1 text-[11px]" style={{ background: colors[0], color: colors[1] }}>{meta.label}</span> }
function ItemCard({ item }: { item: PeerRecord }) { const image = textValue(valueOf(item, 'imageUrl', 'picUrl', 'itemPic')); return <article className="p-3" style={{ background: 'var(--surface-card)' }}>{image ? <SafeRemoteImage src={image} alt="" className="mb-3 aspect-square w-full rounded-lg object-cover" fallback={<div className="mb-3 aspect-square w-full rounded-lg" style={{ background: 'var(--surface-sunken)' }} />} /> : null}<p className="line-clamp-2 text-[12px] font-medium" style={{ color: 'var(--text-main)' }}>{valueOf(item, 'title', 'itemTitle') || '公开宝贝'}</p><p className="mt-2 text-[15px] font-semibold" style={{ color: 'var(--danger)' }}>¥{valueOf(item, 'price', 'currentPrice') ?? '—'}</p><p className="mt-1 text-[10px]" style={{ color: 'var(--text-secondary)' }}>最近采集 {formatTime(valueOf(item, 'observedAt', 'lastObservedAt'))}</p></article> }
function TimelineRow({ entry }: { entry: PeerRecord }) { const meta = timelineMeta(valueOf(entry, 'status', 'eventType', 'statusLabel')); return <div className="flex gap-3 px-4 py-3 border-b last:border-0" style={{ borderColor: 'var(--border-subtle)' }}><SourceThumbnail source={entry} /><div className="min-w-0 flex-1"><div className="flex gap-2"><p className="min-w-0 flex-1 truncate text-[12px]" style={{ color: 'var(--text-main)' }}>{valueOf(entry, 'title', 'itemTitle') || '公开动态'}</p><span className="shrink-0 rounded-full px-2 py-0.5 text-[10px]" style={{ background: meta.background, color: meta.color }}>{valueOf(entry, 'statusLabel') || meta.label}</span></div><p className="mt-1 text-[12px] font-semibold" style={{ color: 'var(--danger)' }}>¥{valueOf(entry, 'price', 'itemPrice') ?? '—'}</p><p className="mt-1 text-[10px]" style={{ color: 'var(--text-secondary)' }}>{valueOf(entry, 'date') || formatTime(valueOf(entry, 'observedAt'))}</p></div></div> }
function ReviewRow({ review }: { review: PeerRecord }) { const itemTitle = valueOf(review, 'itemTitle'); return <article className="px-4 py-3 border-b last:border-0" style={{ borderColor: 'var(--border-subtle)' }}>{itemTitle ? <p className="truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>评价商品：{itemTitle}</p> : null}<div className="mt-1 flex justify-between gap-3 text-[11px]" style={{ color: 'var(--text-secondary)' }}><span>{valueOf(review, 'raterNick') || '匿名评价人'}</span><span>{valueOf(review, 'createdAt') || formatTime(valueOf(review, 'observedAt'))}</span></div><p className="mt-2 text-[12px] leading-5" style={{ color: 'var(--text-main)' }}>{valueOf(review, 'feedback') || '评价未提供文字内容。'}</p></article> }
function EventRow({ event }: { event: PeerRecord }) { const meta = eventMeta(valueOf(event, 'type', 'eventType')); return <div className="px-4 py-3 border-b last:border-0 flex gap-3" style={{ borderColor: 'var(--border-subtle)' }}><span className="mt-0.5 rounded-full px-2 py-1 h-fit text-[10px]" style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}>{meta.label}</span><div className="min-w-0 flex-1"><p className="text-[12px]" style={{ color: 'var(--text-main)' }}>{valueOf(event, 'title', 'summary', 'message') || '同行账号发生变化'}</p></div><span className="text-[10px] shrink-0" style={{ color: 'var(--text-secondary)' }}>{formatTime(valueOf(event, 'occurredAt', 'createdAt', 'at'))}</span></div> }
function RunRow({ run }: { run: PeerRecord }) { const meta = statusMeta(valueOf(run, 'status')); return <div className="px-4 py-3 border-b last:border-0" style={{ borderColor: 'var(--border-subtle)' }}><div className="flex items-center justify-between gap-3"><StatusPill meta={meta} /><span className="text-[10px]" style={{ color: 'var(--text-secondary)' }}>{formatTime(valueOf(run, 'finishedAt', 'createdAt'))}</span></div><p className="mt-2 text-[12px]" style={{ color: 'var(--text-main)' }}>{valueOf(run, 'detail') || '已完成采集。'}</p><p className="mt-1 text-[10px]" style={{ color: 'var(--text-secondary)' }}>主页 {valueOf(run, 'profileCount') ?? 0} · 宝贝 {valueOf(run, 'itemCount') ?? 0} · 动态 {valueOf(run, 'timelineCount') ?? 0} · 评价 {valueOf(run, 'rateCount') ?? 0}</p></div> }
function SourceThumbnail({ source }: { source: PeerRecord }) { const image = textValue(valueOf(source, 'imageUrl', 'picUrl', 'itemPic')); return image ? <SafeRemoteImage src={image} alt="" className="h-10 w-10 shrink-0 rounded-lg object-cover" fallback={<div className="h-10 w-10 shrink-0 rounded-lg" style={{ background: 'var(--surface-sunken)' }} />} /> : <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg" style={{ background: 'var(--surface-sunken)', color: 'var(--text-tertiary)' }}><UserRound className="w-4 h-4" /></span> }
function timelineMeta(value: PeerValue) { const normalized = String(value || '').toLowerCase(); if (/listed|上架/.test(normalized)) return { label: '上架', background: 'var(--status-success-bg)', color: 'var(--status-success-text)' }; if (/sold|售出/.test(normalized)) return { label: '售出', background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }; if (/off_shelf|下架/.test(normalized)) return { label: '下架', background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }; return { label: '动态', background: 'var(--surface-control)', color: 'var(--text-secondary)' } }
