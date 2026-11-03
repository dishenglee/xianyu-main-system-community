// @ts-nocheck
import { useCallback, useEffect, useRef, useState } from 'react'
import { Activity, BarChart3, ListChecks, RefreshCw } from 'lucide-react'
import { getMessageDailyStats, getOperationJob, getOperationJobs, getOperationLogs } from '../../api/index'
import { TopBar } from '../../components/TopBar'
import { EmptyState } from '../../components/EmptyState'
import { useDevice } from '../../hooks/useDevice'
import { useAuthScope } from '../../hooks/useAuthScope'

const TABS = [
  { key: 'logs', label: '操作日志', icon: Activity },
  { key: 'jobs', label: '后台任务', icon: ListChecks },
  { key: 'stats', label: '消息统计', icon: BarChart3 },
]

function statusSoftStyle(status) {
  const value = String(status || '').toLowerCase()
  if (value === 'success') return { background: 'var(--status-success-bg)', color: 'var(--status-success-text)' }
  if (value === 'partial_success') return { background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }
  if (value === 'failed' || value === 'error') return { background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }
  if (value === 'running') return { background: 'var(--status-info-bg)', color: 'var(--status-info-text)' }
  return { background: 'var(--status-neutral-bg)', color: 'var(--status-neutral-text)' }
}

function formatTime(ts) {
  if (!ts) return '—'
  const value = Number(ts)
  if (!Number.isFinite(value) || value <= 0) return '—'
  return new Date(value * 1000).toLocaleString('zh-CN')
}

function summarizeStats(stats) {
  return {
    days: new Set(stats.map((row) => row.stat_date).filter(Boolean)).size,
    accounts: new Set(stats.map((row) => row.uid).filter(Boolean)).size,
    received: stats.reduce((sum, row) => sum + Number(row.received_message_count || 0), 0),
    ai: stats.reduce((sum, row) => sum + Number(row.ai_reply_count || 0), 0),
  }
}

function MobileSummaryCard({ label, value }) {
  return (
    <div className="mobile-soft-card px-3 py-3">
      <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>{label}</div>
      <div className="mt-2 text-[20px] font-semibold leading-none" style={{ color: 'var(--text-main)' }}>{value}</div>
    </div>
  )
}

export default function LogsPage() {
  const { isMobile } = useDevice()
  const [tab, setTab] = useState('logs')
  const [loading, setLoading] = useState(false)
  const [keyword, setKeyword] = useState('')
  const [logs, setLogs] = useState([])
  const [jobs, setJobs] = useState([])
  const [selectedJob, setSelectedJob] = useState(null)
  const [stats, setStats] = useState([])
  const loadSeqRef = useRef(0)
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1
    setLogs([])
    setJobs([])
    setSelectedJob(null)
    setStats([])
    setLoading(false)
  }, [])
  const { authToken, captureScope, isScopeCurrent } = useAuthScope(clearPageState)

  async function load() {
    const scope = captureScope(tab)
    const seq = ++loadSeqRef.current
    const isCurrent = () => isScopeCurrent(scope, tab) && loadSeqRef.current === seq
    setLoading(true)
    try {
      if (tab === 'logs') {
        const response = await getOperationLogs({ keyword, limit: 200 })
        if (!isCurrent()) return
        setLogs(Array.isArray(response?.items) ? response.items : [])
      } else if (tab === 'jobs') {
        const response = await getOperationJobs()
        if (!isCurrent()) return
        const rows = Array.isArray(response?.items) ? response.items : []
        setJobs(rows)
        if (rows[0]?.id) {
          const detail = await getOperationJob(rows[0].id)
          if (!isCurrent()) return
          setSelectedJob(detail)
        } else {
          setSelectedJob(null)
        }
      } else {
        const response = await getMessageDailyStats({ limit: 90 })
        if (!isCurrent()) return
        setStats(Array.isArray(response?.items) ? response.items : [])
      }
    } finally {
      if (isCurrent()) setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [tab, authToken])

  async function openJob(id) {
    const scope = captureScope(tab)
    const detail = await getOperationJob(id)
    if (isScopeCurrent(scope, tab)) setSelectedJob(detail)
  }

  const statSummary = summarizeStats(stats)

  if (isMobile) {
    return (
      <div className="h-full flex flex-col mobile-page-shell" style={{ backgroundColor: 'var(--bg-page)' }}>
        <TopBar
          title="平台日志"
          right={(
            <button type="button" onClick={() => void load()} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40" aria-label="刷新">
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            </button>
          )}
        />

        <div className="flex-1 overflow-y-auto mobile-page-body">
          <div className="mobile-page-stack">
            <section className="mobile-surface-card px-4 py-4">
              <div className="mobile-section-head items-start">
                <div className="mobile-section-copy">
                  <h2 className="mobile-section-title">平台日志</h2>
                </div>
                <span className="mobile-pill mobile-muted-pill shrink-0">
                  {tab === 'logs' ? `${logs.length} 条` : tab === 'jobs' ? `${jobs.length} 项` : `${stats.length} 行`}
                </span>
              </div>

              <div className="mt-4 flex gap-2 overflow-x-auto pb-1" style={{ scrollbarWidth: 'none' }}>
                {TABS.map((item) => {
                  const Icon = item.icon
                  const active = tab === item.key
                  return (
                    <button
                      key={item.key}
                      type="button"
                      onClick={() => setTab(item.key)}
                      className={`mobile-pill shrink-0 px-3 py-2 ${active ? '' : 'mobile-muted-pill'}`}
                      style={active
                        ? {
                            color: 'var(--primary)',
                            background: 'var(--primary-light)',
                            borderColor: 'color-mix(in srgb, var(--primary) 24%, transparent)',
                            fontWeight: 700,
                          }
                        : { color: 'var(--text-main)' }}
                    >
                      <Icon className="h-3.5 w-3.5" />
                      {item.label}
                    </button>
                  )
                })}
              </div>

              {tab === 'logs' ? (
                <div className="mobile-soft-card mt-4 p-3.5">
                  <div className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>搜索日志</div>
                  <input
                    className="input mt-3 rounded-[16px] border-[var(--control-border)] px-4 py-3 text-[14px]"
                    placeholder="搜索账号 / 商品 / 文案关键词"
                    value={keyword}
                    onChange={(event) => setKeyword(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') void load()
                    }}
                  />
                </div>
              ) : null}

              {tab === 'stats' ? (
                <div className="mt-4 grid grid-cols-2 gap-3">
                  <MobileSummaryCard label="统计天数" value={statSummary.days} />
                  <MobileSummaryCard label="账号数" value={statSummary.accounts} />
                  <MobileSummaryCard label="收到消息" value={statSummary.received} />
                  <MobileSummaryCard label="AI 回复" value={statSummary.ai} />
                </div>
              ) : null}
            </section>

            {loading ? (
              <section className="mobile-surface-card flex min-h-[220px] items-center justify-center px-6">
                <div className="flex items-center gap-2 text-[13px]" style={{ color: 'var(--text-secondary)' }}>
                  <RefreshCw className="h-4 w-4 animate-spin" />
                  加载中…
                </div>
              </section>
            ) : tab === 'logs' ? (
              !logs.length ? (
                <EmptyState icon={Activity} title="暂无操作日志" description="当前筛选下没有记录" />
              ) : (
                <>
                  <section className="mobile-surface-card px-4 py-4">
                    <div className="mobile-section-copy">
                      <h2 className="mobile-section-title">日志</h2>
                    </div>
                  </section>
                  <div className="space-y-3">
                    {logs.map((log) => (
                      <article key={log.id} className="mobile-surface-card p-3.5">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]" style={statusSoftStyle(log.status)}>
                            {log.status || 'queued'}
                          </span>
                          <span className="text-[11px] font-semibold" style={{ color: 'var(--text-main)' }}>
                            {log.action || 'unknown'}
                          </span>
                          <span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                            {log.account_nick || log.uid || '系统'}
                          </span>
                          <span className="ml-auto text-[10.5px]" style={{ color: 'var(--text-secondary)' }}>
                            {formatTime(log.created_at)}
                          </span>
                        </div>
                        <div className="mt-3 text-[13px] leading-6" style={{ color: 'var(--text-main)' }}>
                          {log.message || '—'}
                        </div>
                        <div className="mt-2 text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                          {log.target_title || log.target_id || '—'}
                        </div>
                      </article>
                    ))}
                  </div>
                </>
              )
            ) : tab === 'jobs' ? (
              !jobs.length ? (
                <EmptyState icon={ListChecks} title="暂无后台任务" description="暂无执行记录" />
              ) : (
                <>
                  <section className="mobile-surface-card px-4 py-4">
                    <div className="mobile-section-copy">
                      <h2 className="mobile-section-title">任务</h2>
                    </div>
                  </section>
                  <div className="space-y-3">
                    {jobs.map((job) => {
                      const active = selectedJob?.job?.id === job.id
                      return (
                        <button
                          key={job.id}
                          type="button"
                          onClick={() => void openJob(job.id)}
                          className="mobile-surface-card w-full p-3.5 text-left transition"
                          style={active ? { borderColor: 'rgba(39,87,200,0.24)', boxShadow: '0 0 0 1px rgba(39,87,200,0.16) inset' } : undefined}
                        >
                          <div className="flex items-center gap-2">
                            <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]" style={statusSoftStyle(job.status)}>
                              {job.status}
                            </span>
                            <span className="min-w-0 truncate text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                              {job.summary?.title || job.action}
                            </span>
                          </div>
                          <div className="mt-3 grid grid-cols-3 gap-2 text-[11px]">
                            <div className="mobile-soft-card px-2.5 py-2 text-center">总数 {job.total_count}</div>
                            <div className="mobile-soft-card px-2.5 py-2 text-center" style={{ color: 'var(--status-success-text)' }}>成功 {job.success_count}</div>
                            <div className="mobile-soft-card px-2.5 py-2 text-center" style={{ color: 'var(--status-danger-text)' }}>失败 {job.fail_count}</div>
                          </div>
                        </button>
                      )
                    })}
                  </div>

                  <section className="mobile-surface-card overflow-hidden">
                    <div className="border-b px-4 py-4" style={{ borderColor: 'var(--glass-border)' }}>
                      <div className="mobile-section-copy">
                        <h2 className="mobile-section-title">任务明细</h2>
                      </div>
                    </div>
                    {!selectedJob?.job ? (
                      <div className="px-4 py-8 text-[12px]" style={{ color: 'var(--text-secondary)' }}>选择上方任务查看明细</div>
                    ) : (
                      <div className="space-y-3 p-3">
                        <div className="mobile-soft-card p-3.5">
                          <div className="flex items-center gap-2 flex-wrap">
                            <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]" style={statusSoftStyle(selectedJob.job.status)}>
                              {selectedJob.job.status}
                            </span>
                            <span className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                              {selectedJob.job.summary?.title || selectedJob.job.action}
                            </span>
                            <span className="ml-auto text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                              {formatTime(selectedJob.job.created_at)}
                            </span>
                          </div>
                        </div>

                        {(selectedJob.items || []).map((row) => (
                          <div key={row.id} className="mobile-soft-card p-3.5">
                            <div className="flex items-center gap-2 flex-wrap">
                              <span className="mobile-pill mobile-muted-pill px-2.5 py-1 text-[10px]" style={statusSoftStyle(row.status)}>
                                {row.status}
                              </span>
                              <span className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                                {row.account_nick || row.uid}
                              </span>
                              <span className="ml-auto text-[10.5px]" style={{ color: 'var(--text-secondary)' }}>
                                {row.target_title || row.target_id}
                              </span>
                            </div>
                            <div className="mt-2 text-[11px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                              {row.message || row.error_code || '—'}
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </section>
                </>
              )
            ) : (
              !stats.length ? (
                <EmptyState icon={BarChart3} title="暂无消息统计" description="暂无统计数据" />
              ) : (
                <>
                  <section className="mobile-surface-card px-4 py-4">
                    <div className="mobile-section-copy">
                      <h2 className="mobile-section-title">消息统计</h2>
                    </div>
                  </section>
                  <div className="space-y-3">
                    {stats.map((row) => (
                      <article key={`${row.stat_date}-${row.uid}`} className="mobile-surface-card p-3.5">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{row.account_nick || row.uid}</span>
                          <span className="ml-auto text-[11px]" style={{ color: 'var(--text-secondary)' }}>{row.stat_date}</span>
                        </div>
                        <div className="mt-3 grid grid-cols-2 gap-2 text-[11px]">
                          <div className="mobile-soft-card px-3 py-2">收到消息 {row.received_message_count}</div>
                          <div className="mobile-soft-card px-3 py-2">互动消息 {row.interaction_message_count}</div>
                          <div className="mobile-soft-card px-3 py-2">通知消息 {row.notification_message_count}</div>
                          <div className="mobile-soft-card px-3 py-2">默认回复 {row.default_reply_count}</div>
                          <div className="mobile-soft-card px-3 py-2">自动回复 {row.auto_reply_count}</div>
                          <div className="mobile-soft-card px-3 py-2">AI 回复 {row.ai_reply_count}</div>
                          <div className="mobile-soft-card px-3 py-2">手动回复 {row.manual_reply_count}</div>
                          <div className="mobile-soft-card px-3 py-2">图片回复 {row.image_reply_count}</div>
                          <div className="mobile-soft-card px-3 py-2">自动发货 {row.auto_delivery_count}</div>
                          <div className="mobile-soft-card px-3 py-2">回复失败 {row.reply_failure_count}</div>
                          <div className="mobile-soft-card px-3 py-2">撤回次数 {row.recall_count}</div>
                        </div>
                      </article>
                    ))}
                  </div>
                </>
              )
            )}
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="h-full flex flex-col" style={{ backgroundColor: 'var(--bg-page)' }}>
      <TopBar
        title="平台日志"
        right={(
          <button type="button" onClick={() => void load()} disabled={loading} className="btn-ghost btn-sm disabled:opacity-40">
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          </button>
        )}
      />

      <div className="flex gap-2 overflow-x-auto px-3 pt-3 md:px-4" style={{ scrollbarWidth: 'none' }}>
        {TABS.map((item) => {
          const Icon = item.icon
          return (
            <button
              key={item.key}
              type="button"
              onClick={() => setTab(item.key)}
              className={`flex items-center gap-1.5 whitespace-nowrap rounded-full px-3 py-2 text-[12px] font-medium ${
                tab === item.key ? 'border font-semibold' : 'border bg-[var(--surface-soft)]'
              }`}
              style={tab === item.key
                ? {
                    color: 'var(--primary)',
                    background: 'var(--primary-light)',
                    borderColor: 'color-mix(in srgb, var(--primary) 24%, transparent)',
                  }
                : {
                    color: 'var(--text-main)',
                    borderColor: 'var(--control-border)',
                  }}
            >
              <Icon className="w-3.5 h-3.5" />
              {item.label}
            </button>
          )
        })}
      </div>

      {tab === 'logs' ? (
        <div className="px-3 py-3 md:px-4">
          <input
            className="input text-[13px] md:text-[14px]"
            placeholder="搜索账号 / 商品 / 文案关键词"
            value={keyword}
            onChange={(event) => setKeyword(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') void load()
            }}
          />
        </div>
      ) : null}

      <div className="flex-1 overflow-y-auto px-3 pb-4 md:px-4">
        {loading ? (
          <div className="flex items-center justify-center gap-2 py-20" style={{ color: 'var(--text-secondary)' }}>
            <RefreshCw className="w-4 h-4 animate-spin" /> 加载中…
          </div>
        ) : tab === 'logs' ? (
          !logs.length ? (
            <EmptyState icon={Activity} title="暂无操作日志" />
          ) : (
            <div className="space-y-2">
              {logs.map((log) => (
                <div key={log.id} className="card p-3">
                  <div className="mb-1.5 flex items-center gap-2 flex-wrap">
                    <span className="rounded px-2 py-0.5 text-[10px] font-bold" style={statusSoftStyle(log.status)}>
                      {log.status || 'queued'}
                    </span>
                    <span className="text-[11px] font-medium" style={{ color: 'var(--text-main)' }}>
                      {log.action || 'unknown'}
                    </span>
                    <span className="text-[11px]" style={{ color: 'var(--text-muted)' }}>
                      {log.account_nick || log.uid || '系统'}
                    </span>
                    <span className="ml-auto text-[10.5px]" style={{ color: 'var(--text-muted)' }}>
                      {formatTime(log.created_at)}
                    </span>
                  </div>
                  <div className="text-[12.5px]" style={{ color: 'var(--text-main)' }}>
                    {log.message || '—'}
                  </div>
                  <div className="mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                    {log.target_title || log.target_id || '—'}
                  </div>
                </div>
              ))}
            </div>
          )
        ) : tab === 'jobs' ? (
          !jobs.length ? (
            <EmptyState icon={ListChecks} title="暂无后台任务" />
          ) : (
            <div className="grid gap-3 md:grid-cols-[320px_minmax(0,1fr)]">
              <div className="space-y-2">
                {jobs.map((job) => (
                  <button
                    key={job.id}
                    type="button"
                    onClick={() => void openJob(job.id)}
                    className="card w-full p-3 text-left"
                    style={selectedJob?.job?.id === job.id ? { borderColor: 'var(--primary)', boxShadow: '0 0 0 1px var(--primary) inset' } : {}}
                  >
                    <div className="mb-1 flex items-center gap-2">
                      <span className="rounded px-2 py-0.5 text-[10px] font-bold" style={statusSoftStyle(job.status)}>
                        {job.status}
                      </span>
                      <span className="truncate text-[12px] font-medium">{job.summary?.title || job.action}</span>
                    </div>
                    <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>
                      总数 {job.total_count} · 成功 {job.success_count} · 失败 {job.fail_count}
                    </div>
                  </button>
                ))}
              </div>
              <div className="card min-h-[240px] p-3">
                {!selectedJob?.job ? (
                  <div className="text-[12px]" style={{ color: 'var(--text-muted)' }}>选择左侧任务查看明细</div>
                ) : (
                  <>
                    <div className="mb-3 flex items-center gap-2 flex-wrap">
                      <span className="rounded px-2 py-0.5 text-[10px] font-bold" style={statusSoftStyle(selectedJob.job.status)}>
                        {selectedJob.job.status}
                      </span>
                      <span className="text-[13px] font-semibold">{selectedJob.job.summary?.title || selectedJob.job.action}</span>
                      <span className="ml-auto text-[11px]" style={{ color: 'var(--text-muted)' }}>
                        {formatTime(selectedJob.job.created_at)}
                      </span>
                    </div>
                    <div className="space-y-2">
                      {(selectedJob.items || []).map((row) => (
                        <div key={row.id} className="rounded-[12px] px-3 py-2" style={{ background: 'var(--surface-soft)' }}>
                          <div className="flex items-center gap-2 flex-wrap">
                            <span className="rounded px-2 py-0.5 text-[10px] font-bold" style={statusSoftStyle(row.status)}>
                              {row.status}
                            </span>
                            <span className="text-[12px] font-medium">{row.account_nick || row.uid}</span>
                            <span className="ml-auto text-[11px]" style={{ color: 'var(--text-muted)' }}>
                              {row.target_title || row.target_id}
                            </span>
                          </div>
                          <div className="mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                            {row.message || row.error_code || '—'}
                          </div>
                        </div>
                      ))}
                    </div>
                  </>
                )}
              </div>
            </div>
          )
        ) : (
          !stats.length ? (
            <EmptyState icon={BarChart3} title="暂无消息统计" />
          ) : (
            <div className="space-y-2">
              {stats.map((row) => (
                <div key={`${row.stat_date}-${row.uid}`} className="card p-3">
                  <div className="mb-2 flex items-center gap-2 flex-wrap">
                    <span className="text-[13px] font-semibold">{row.account_nick || row.uid}</span>
                    <span className="ml-auto text-[11px]" style={{ color: 'var(--text-muted)' }}>{row.stat_date}</span>
                  </div>
                  <div className="grid grid-cols-2 gap-2 text-[12px] md:grid-cols-4">
                    <div>收到消息：{row.received_message_count}</div>
                    <div>互动消息：{row.interaction_message_count}</div>
                    <div>通知消息：{row.notification_message_count}</div>
                    <div>默认回复：{row.default_reply_count}</div>
                    <div>自动回复：{row.auto_reply_count}</div>
                    <div>AI回复：{row.ai_reply_count}</div>
                    <div>手动回复：{row.manual_reply_count}</div>
                    <div>图片回复：{row.image_reply_count}</div>
                    <div>自动发货：{row.auto_delivery_count}</div>
                    <div>回复失败：{row.reply_failure_count}</div>
                    <div>撤回次数：{row.recall_count}</div>
                  </div>
                </div>
              ))}
            </div>
          )
        )}
      </div>
    </div>
  )
}
