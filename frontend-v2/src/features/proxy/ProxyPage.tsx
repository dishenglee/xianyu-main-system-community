// @ts-nocheck
import { useState, useEffect, useCallback, useRef } from 'react'
import { Globe, Plus, Activity, Trash2, ScanSearch } from 'lucide-react'
import {
  proxyList, proxyAdd, proxyBatchAdd, proxyRemove,
  proxySetEnabled, proxySetStrategy, proxyCheck, proxyCheckAll,
} from '../../api/index'
import { TopBar } from '../../components/TopBar'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { EmptyState } from '../../components/EmptyState'
import { Modal } from '../../components/Modal'
import { useSafeNotify } from '../../components/Notify'
import { useDevice } from '../../hooks/useDevice'
import { useAppDialog } from '../../components/AppDialog'
import { useAuthScope } from '../../hooks/useAuthScope'

function Toggle({ checked, onChange }) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      className="relative h-7 w-12 rounded-full transition-colors cursor-pointer"
      style={{ background: checked ? 'var(--success)' : 'rgba(118,118,128,0.24)' }}
    >
      <span
        className={`absolute top-0.5 h-6 w-6 rounded-full transition-transform ${checked ? 'translate-x-5' : 'translate-x-0.5'}`}
        style={{
          backgroundColor: 'var(--liquid-highlight)',
          boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.64), 0 2px 6px rgba(25,35,55,0.12)',
        }}
      />
    </button>
  )
}

function MobileMetricCard({ label, value, tone = 'neutral' }) {
  const toneMap = {
    neutral: { color: 'var(--text-main)' },
    success: { color: 'var(--status-success-text)' },
    danger: { color: 'var(--status-danger-text)' },
  }
  const currentTone = toneMap[tone] || toneMap.neutral

  return (
    <div className="mobile-soft-card px-3 py-3">
      <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{label}</div>
      <div className="mt-2 text-[22px] font-semibold leading-none" style={currentTone}>{value}</div>
    </div>
  )
}

function AvailabilityBadge({ alive }) {
  return (
    <span
      className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]"
      style={alive ? { color: 'var(--status-success-text)', background: 'var(--status-success-bg)' } : { color: 'var(--status-danger-text)', background: 'var(--status-danger-bg)' }}
    >
      {alive ? '可用' : '不可用'}
    </span>
  )
}

function maskUrl(url) {
  try {
    const u = new URL(url)
    if (u.password) u.password = '***'
    return u.toString()
  } catch { return url }
}

export default function ProxyPage() {
  const { isMobile } = useDevice()
  const notify = useSafeNotify()
  const { showConfirm } = useAppDialog()
  const [proxies, setProxies] = useState([])
  const [enabled, setEnabled] = useState(false)
  const [strategy, setStrategy] = useState('round-robin')
  const [proxyTotal, setProxyTotal] = useState(0)
  const [proxyAlive, setProxyAlive] = useState(0)
  const [checking, setChecking] = useState(false)
  const [showAdd, setShowAdd] = useState(false)
  const [addMode, setAddMode] = useState('single')
  const [addForm, setAddForm] = useState({ url: '', label: '' })
  const [batchText, setBatchText] = useState('')
  const loadSeqRef = useRef(0)
  const unavailableCount = Math.max(proxyTotal - proxyAlive, 0)
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    setProxies([])
    setEnabled(false)
    setStrategy('round-robin')
    setProxyTotal(0)
    setProxyAlive(0)
    setChecking(false)
    setShowAdd(false)
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  async function loadProxies() {
    const scope = captureScope()
    const seq = ++loadSeqRef.current
    try {
      const r = await proxyList()
      if (!isScopeCurrent(scope) || loadSeqRef.current !== seq) return
      const d = r?.data || r || {}
      setProxies(d.proxies || [])
      setEnabled(Boolean(d.enabled))
      setStrategy(d.strategy || 'round-robin')
      setProxyTotal(d.total ?? 0)
      setProxyAlive(d.alive ?? 0)
    } catch (e) { console.error(e) }
  }

  useEffect(() => { loadProxies() }, [authToken])

  async function toggleEnabled() {
    const scope = captureScope()
    try {
      await proxySetEnabled(!enabled)
      if (!isScopeCurrent(scope)) return
      setEnabled(!enabled)
      notify(!enabled ? '代理已开启' : '代理已关闭', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '切换代理状态失败', 'error') }
  }

  async function changeStrategy(s) {
    setStrategy(s)
    const scope = captureScope()
    try {
      await proxySetStrategy(s)
      if (!isScopeCurrent(scope)) return
      notify('轮换策略已更新', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '切换策略失败', 'error') }
  }

  async function doAdd() {
    const scope = captureScope()
    try {
      if (addMode === 'single') {
        await proxyAdd(addForm.url.trim(), addForm.label.trim())
      } else {
        const lines = batchText.split('\n').map((l) => l.trim()).filter(Boolean)
        await proxyBatchAdd(lines)
      }
      if (!isScopeCurrent(scope)) return
      setShowAdd(false)
      setAddForm({ url: '', label: '' })
      setBatchText('')
      await loadProxies()
      notify(addMode === 'single' ? '代理已添加' : '代理已批量添加', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '添加代理失败', 'error') }
  }

  async function removeOne(url) {
    const confirmed = await showConfirm('确定删除此代理？', {
      title: '删除代理',
      confirmLabel: '删除',
      danger: true,
    })
    if (!confirmed) return
    const scope = captureScope()
    try {
      await proxyRemove(url)
      if (!isScopeCurrent(scope)) return
      await loadProxies()
      notify('代理已删除', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '删除代理失败', 'error') }
  }

  async function checkOne(url) {
    setChecking(true)
    const scope = captureScope()
    try {
      const r = await proxyCheck(url)
      if (!isScopeCurrent(scope)) return
      const d = r?.data || r || {}
      notify(
        d.alive
          ? `可用，延迟 ${d.latency_ms}ms，IP: ${d.ip}`
          : `不可用：${d.error || '超时'}`,
        d.alive ? 'success' : 'error',
        4200,
      )
      await loadProxies()
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '代理检测失败', 'error') }
    finally { if (isScopeCurrent(scope)) setChecking(false) }
  }

  async function checkAll() {
    setChecking(true)
    const scope = captureScope()
    try {
      await proxyCheckAll()
      if (!isScopeCurrent(scope)) return
      await loadProxies()
      notify('已完成全部代理检测', 'success')
    } catch (e) { if (isScopeCurrent(scope)) notify(e?.message || '批量检测失败', 'error') }
    finally { if (isScopeCurrent(scope)) setChecking(false) }
  }

  const addProxyModal = (
    <Modal open={showAdd} onClose={() => setShowAdd(false)} title="添加代理">
      <div className={`space-y-4 py-2 ${isMobile ? 'pb-4' : ''}`}>
        <div className={`flex gap-2 ${isMobile ? 'overflow-x-auto pb-1' : ''}`} style={isMobile ? { scrollbarWidth: 'none' } : undefined}>
          {['single', 'batch'].map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => setAddMode(m)}
              className={`text-[13px] font-medium transition ${isMobile ? 'mobile-pill shrink-0 px-4 py-2' : 'rounded-xl px-4 py-2'}`}
              style={addMode === m
                ? { backgroundColor: 'var(--primary-light)', color: 'var(--primary)', border: '1px solid var(--primary)' }
                : { backgroundColor: isMobile ? 'var(--surface-control)' : 'var(--bg-page)', color: 'var(--text-secondary)', border: '1px solid var(--border-light)' }}
            >
              {m === 'single' ? '单个添加' : '批量添加'}
            </button>
          ))}
        </div>

        {addMode === 'single' ? (
          <>
            <label className="block">
              <span className="label">代理地址</span>
              <input
                className={`input ${isMobile ? 'mt-2 rounded-[16px] border-[var(--control-border)] px-4 py-3' : ''}`}
                value={addForm.url}
                onChange={(e) => setAddForm((f) => ({ ...f, url: e.target.value }))}
                placeholder="http://user:pass@host:port"
              />
            </label>
            <label className="block">
              <span className="label">备注标签（可选）</span>
              <input
                className={`input ${isMobile ? 'mt-2 rounded-[16px] border-[var(--control-border)] px-4 py-3' : ''}`}
                value={addForm.label}
                onChange={(e) => setAddForm((f) => ({ ...f, label: e.target.value }))}
                placeholder="如：香港节点-01"
              />
            </label>
          </>
        ) : (
          <label className="block">
            <span className="label">代理地址列表（每行一个）</span>
            <textarea
              className={`input min-h-[180px] resize-y font-mono text-[13px] ${isMobile ? 'mt-2 rounded-[18px] border-[var(--control-border)] px-4 py-3' : ''}`}
              value={batchText}
              onChange={(e) => setBatchText(e.target.value)}
              placeholder="http://user:pass@host1:port"
            />
          </label>
        )}

        <p
          className="liquid-notice liquid-notice-info rounded-2xl px-4 py-3 text-[12px]"
        >
          http / socks5
        </p>

        <div className="flex items-center justify-end gap-2 pt-2 md:gap-3">
          <button type="button" onClick={() => setShowAdd(false)} className="btn-secondary btn-sm flex-1 md:flex-none">取消</button>
          <button type="button" onClick={doAdd} className="btn-primary btn-sm flex-1 md:flex-none">添加</button>
        </div>
      </div>
    </Modal>
  )

  if (isMobile) {
    return (
      <>
        <div className="h-full flex flex-col mobile-page-shell" style={{ backgroundColor: 'var(--bg-page)' }}>
          <TopBar
            title="代理 IP 管理"
            right={(
              <div className="flex items-center gap-1">
                <button type="button" onClick={checkAll} disabled={checking} className="btn-ghost btn-sm disabled:opacity-40" aria-label="全部检测">
                  <Activity className={`h-4 w-4 ${checking ? 'animate-spin' : ''}`} />
                </button>
                <button type="button" onClick={() => setShowAdd(true)} className="btn-primary btn-sm" aria-label="添加代理">
                  <Plus className="h-4 w-4" />
                </button>
              </div>
            )}
          />

          <HubTabs tabs={HUB_PRESETS.accounts} />

          <div className="flex-1 overflow-y-auto mobile-page-body">
            <div className="mobile-page-stack">
              <section className="mobile-surface-card px-4 py-4">
              <div className="mobile-section-head items-start">
                <div className="mobile-section-copy">
                  <h2 className="mobile-section-title">代理池</h2>
                </div>
                <span
                  className={`mobile-pill shrink-0 ${enabled ? '' : 'mobile-muted-pill'}`}
                  style={enabled ? { color: 'var(--status-success-text)' } : { color: 'var(--text-secondary)' }}
                >
                  {enabled ? '运行中' : '未开启'}
                </span>
              </div>

              <div className="mt-4 grid grid-cols-3 gap-3">
                <MobileMetricCard label="代理总数" value={proxyTotal} />
                <MobileMetricCard label="可用节点" value={proxyAlive} tone="success" />
                <MobileMetricCard label="不可用" value={unavailableCount} tone="danger" />
              </div>

              <div className="mobile-soft-card mt-4 p-3.5">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>代理状态</div>
                    <div className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                      {enabled ? '当前请求将按现有策略经过代理池。' : '当前请求仍按原样直连。'}
                    </div>
                  </div>
                  <Toggle checked={enabled} onChange={toggleEnabled} />
                </div>

                <div className="mt-4 space-y-2">
                  <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>轮换策略</div>
                  <select
                    value={strategy}
                    onChange={(e) => changeStrategy(e.target.value)}
                    className="input w-full rounded-[16px] border-[var(--control-border)] px-4 py-3"
                  >
                    <option value="round-robin">轮询</option>
                    <option value="random">随机</option>
                    <option value="latency">最低延迟</option>
                  </select>
                </div>
              </div>
            </section>

            {!proxies.length ? (
              <section className="mobile-surface-card px-5 py-8 text-center">
                <div
                  className="mx-auto flex h-14 w-14 items-center justify-center rounded-full"
                  style={{ background: 'linear-gradient(135deg, var(--primary-light), rgba(255,255,255,0.9))', color: 'var(--primary)' }}
                >
                  <Globe className="h-6 w-6" />
                </div>
                <h2 className="mobile-section-title mt-4 text-[1.2rem]">暂无代理池</h2>
                <p className="mobile-section-note mt-2">先添加代理地址</p>
                <button type="button" onClick={() => setShowAdd(true)} className="btn-primary mt-5 w-full py-3 text-[15px]">
                  添加代理
                </button>
              </section>
            ) : (
              <>
                <section className="mobile-surface-card px-4 py-4">
                  <div className="mobile-section-head items-start">
                    <div className="mobile-section-copy">
                      <h2 className="mobile-section-title">节点</h2>
                    </div>
                    <span className="mobile-pill mobile-muted-pill shrink-0">{proxies.length} 条</span>
                  </div>
                </section>

                <div className="space-y-3">
                  {proxies.map((p) => (
                    <article key={p.url} className="mobile-surface-card p-3.5">
                      <div className="flex items-start gap-3">
                        <div
                          className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl"
                          style={{ backgroundColor: p.alive ? 'var(--status-success-bg)' : 'var(--status-danger-bg)' }}
                        >
                          <Globe className="h-5 w-5" style={{ color: p.alive ? 'var(--status-success-text)' : 'var(--status-danger-text)' }} />
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="flex items-start justify-between gap-2">
                            <div className="min-w-0 break-all font-mono text-[12.5px] font-medium leading-5" style={{ color: 'var(--text-main)' }}>
                              {maskUrl(p.url)}
                            </div>
                            <AvailabilityBadge alive={p.alive} />
                          </div>

                          <div className="mt-3 flex flex-wrap gap-2">
                            {p.label ? (
                              <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]" style={{ color: 'var(--primary)' }}>
                                {p.label}
                              </span>
                            ) : null}
                            <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]">成功 {p.success_count || 0}</span>
                            {p.avg_latency_ms > 0 ? (
                              <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]">延迟 {p.avg_latency_ms}ms</span>
                            ) : null}
                            {p.fail_count > 0 ? (
                              <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]" style={{ color: 'var(--status-danger-text)' }}>
                                连续失败 {p.fail_count}
                              </span>
                            ) : null}
                          </div>
                        </div>
                      </div>

                      <div className="mt-3 grid grid-cols-2 gap-2">
                        <button
                          type="button"
                          onClick={() => checkOne(p.url)}
                          disabled={checking}
                          className="flex items-center justify-center gap-2 rounded-[16px] border px-3 py-2.5 text-[12px] font-semibold transition active:scale-[0.99] disabled:opacity-40"
                          style={{ borderColor: 'var(--primary-light)', background: 'var(--primary-light)', color: 'var(--primary)' }}
                        >
                          <ScanSearch className="h-4 w-4" />
                          检测
                        </button>
                        <button
                          type="button"
                          onClick={() => removeOne(p.url)}
                          className="flex items-center justify-center gap-2 rounded-[16px] border px-3 py-2.5 text-[12px] font-semibold transition active:scale-[0.99]"
                          style={{ borderColor: 'color-mix(in srgb, var(--danger) 16%, transparent)', background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
                        >
                          <Trash2 className="h-4 w-4" />
                          删除
                        </button>
                      </div>
                    </article>
                  ))}
                </div>
              </>
              )}
            </div>
          </div>
        </div>
        {addProxyModal}
      </>
    )
  }

  return (
    <>
      <TopBar
        title="代理 IP 管理"
        right={(
          <div className="flex items-center gap-1.5 md:gap-2">
            <button type="button" onClick={checkAll} disabled={checking} className="btn-ghost btn-sm disabled:opacity-40" aria-label="全部检测">
              <Activity className={`h-3.5 w-3.5 ${checking ? 'animate-spin' : ''}`} />
              <span className="hidden md:inline">全部检测</span>
            </button>
            <button type="button" onClick={() => setShowAdd(true)} className="btn-primary btn-sm" aria-label="添加代理">
              <Plus className="h-3.5 w-3.5" />
              <span className="hidden md:inline">添加代理</span>
            </button>
          </div>
        )}
      />

      <HubTabs tabs={HUB_PRESETS.accounts} />

      <div className="flex-1 overflow-y-auto px-4 py-4 md:px-6">
        <div className="card mb-4 p-3 md:p-4">
          <div className="flex flex-col gap-3 md:flex-row md:flex-wrap md:items-center md:justify-between md:gap-4">
            <div className="flex flex-wrap items-center gap-3 md:gap-6">
              <div className="flex items-center gap-2 md:gap-3">
                <span className="text-[12px] font-medium md:text-[13px]" style={{ color: 'var(--text-secondary)' }}>代理状态</span>
                <Toggle checked={enabled} onChange={toggleEnabled} />
                <span className="text-[12px] font-semibold md:text-[13px]" style={{ color: enabled ? 'var(--status-success-text)' : 'var(--text-muted)' }}>
                  {enabled ? '已开启' : '已关闭'}
                </span>
              </div>
              <div className="flex min-w-0 flex-1 items-center gap-2 md:flex-none">
                <span className="shrink-0 text-[12px] font-medium md:text-[13px]" style={{ color: 'var(--text-secondary)' }}>轮换策略</span>
                <select value={strategy} onChange={(e) => changeStrategy(e.target.value)} className="input flex-1 md:flex-none" style={{ width: 160, padding: '6px 12px' }}>
                  <option value="round-robin">轮询</option>
                  <option value="random">随机</option>
                  <option value="latency">最低延迟</option>
                </select>
              </div>
            </div>
            <div className="flex items-center justify-around gap-4 border-t pt-2 md:justify-end md:gap-6 md:border-0 md:pt-0" style={{ borderColor: 'var(--border-light)' }}>
              <div className="text-center">
                <div className="font-mono text-[18px] font-bold md:text-[22px]" style={{ color: 'var(--primary)' }}>{proxyTotal}</div>
                <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>总数</div>
              </div>
              <div className="text-center">
                <div className="font-mono text-[18px] font-bold md:text-[22px]" style={{ color: 'var(--success)' }}>{proxyAlive}</div>
                <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>可用</div>
              </div>
              <div className="text-center">
                <div className="font-mono text-[18px] font-bold md:text-[22px]" style={{ color: 'var(--danger)' }}>{unavailableCount}</div>
                <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>不可用</div>
              </div>
            </div>
          </div>
        </div>

        {!proxies.length ? (
          <EmptyState icon={Globe} title="暂无代理" description="点击右上角添加" />
        ) : (
          <div className="space-y-3">
            {proxies.map((p) => (
              <div key={p.url} className="card group flex flex-col gap-3 p-3 md:flex-row md:items-center md:justify-between md:gap-0 md:p-4">
                <div className="flex min-w-0 items-center gap-3 md:gap-4">
                  <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl md:h-10 md:w-10" style={{ backgroundColor: p.alive ? 'var(--status-success-bg)' : 'var(--status-danger-bg)' }}>
                    <Globe className="h-4 w-4 md:h-5 md:w-5" style={{ color: p.alive ? 'var(--status-success-text)' : 'var(--danger)' }} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-mono text-[12px] font-medium md:text-[13px]" style={{ color: 'var(--text-main)' }}>{maskUrl(p.url)}</div>
                    <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] md:mt-0.5 md:gap-3" style={{ color: 'var(--text-muted)' }}>
                      {p.label && <span className="rounded-full bg-[var(--primary-light)] px-2 py-0.5 text-[var(--primary)]">{p.label}</span>}
                      <span>成功 {p.success_count || 0} 次</span>
                      {p.avg_latency_ms > 0 && <span>延迟 {p.avg_latency_ms}ms</span>}
                      {p.fail_count > 0 && <span style={{ color: 'var(--danger)' }}>连续失败 {p.fail_count}</span>}
                    </div>
                  </div>
                </div>
                <div className="flex shrink-0 items-center justify-between gap-2 md:justify-end">
                  <span
                    className="rounded-full border px-2 py-1 text-[11px] font-medium md:px-2.5 md:text-[12px]"
                    style={{
                      borderColor: p.alive ? 'color-mix(in srgb, var(--success) 24%, transparent)' : 'color-mix(in srgb, var(--danger) 24%, transparent)',
                      background: p.alive ? 'var(--status-success-bg)' : 'var(--status-danger-bg)',
                      color: p.alive ? 'var(--status-success-text)' : 'var(--status-danger-text)',
                    }}
                  >
                    {p.alive ? '可用' : '不可用'}
                  </span>
                  <div className="flex gap-1 transition-opacity md:opacity-0 md:group-hover:opacity-100">
                    <button type="button" onClick={() => checkOne(p.url)} disabled={checking} className="rounded-lg p-2 transition hover:bg-[var(--primary-light)] active:bg-[var(--primary-light)]" title="检测" aria-label="检测">
                      <Activity className="h-4 w-4" style={{ color: 'var(--primary)' }} />
                    </button>
                    <button type="button" onClick={() => removeOne(p.url)} className="rounded-lg p-2 transition hover:bg-[var(--status-danger-bg)] active:bg-[var(--status-danger-bg)]" title="删除" aria-label="删除">
                      <Trash2 className="h-4 w-4" style={{ color: 'var(--danger)' }} />
                    </button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {addProxyModal}
    </>
  )
}
