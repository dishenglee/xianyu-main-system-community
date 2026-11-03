import { useMemo, useState } from 'react'
import {
  BadgeCheck,
  Coins,
  Gift,
  ImageIcon,
  Megaphone,
  RefreshCw,
  Store,
} from 'lucide-react'
import { EmptyState } from '../../components/EmptyState'
import { SensitiveItemImage, SensitiveValue, maskSensitiveText, useSensitiveHidden, useSensitiveText } from '../../components/SensitiveValue'
import { useDevice } from '../../hooks/useDevice'
import type { PromotionPageState } from './hooks/usePromotionPageState'
import {
  asRecord,
  canClaimCoinTask,
  canReceiveExposureTask,
  getClaimedExposureRewardHistoryRows,
  getPromotionPackages,
  isClaimedCoinTask,
  isClaimedExposureTask,
  isCompletedCoinTask,
  needsCoinTaskBatchComplete,
  needsExposureTaskBatchComplete,
  pickAnywhere,
  pickNumber,
  pickString,
  type JsonRecord,
} from './promotionUtils'

const CARD_CLASS = 'surface-card overflow-hidden rounded-[20px] border'
const CARD_BORDER = { borderColor: 'var(--border-light)' }
const HAIRLINE_BORDER = { borderColor: 'var(--border-light)' }
const TASK_COLUMN_CLASS = 'flex min-h-0 flex-col border-b p-4 md:h-[300px] lg:border-b-0'
const TASK_COLUMN_BODY_CLASS = 'min-h-0 flex-1 overflow-y-auto scrollbar-hidden pr-1'
const EXPOSURE_TASK_TYPE_LABELS: Record<string, string> = {
  shop_subscribe_msg: '关注鱼小铺服务号',
  shop_visit_my_data_new: '每天查看小铺数据',
  shop_visit_diagnosis: '查看小铺诊断建议',
  shop_visit_xy_coin: '访问闲鱼币频道',
  shop_main_pubilsh: '发布一个宝贝',
  shop_main_publish: '发布一个宝贝',
  shop_open_groupon: '开启一个“2人小刀”宝贝',
  shop_improvePack_visit: '查看运营提效包功能页',
  shop_order_one_step: '订单量任务',
}
const TASK_STATUS_LABELS: Record<string, string> = {
  ACCEPTED: '待完成',
  INIT: '待完成',
  PENDING: '待完成',
  TODO: '待完成',
  STARTED: '进行中',
  PROCESSING: '进行中',
  AWARD: '可领奖',
  COMPLETE: '可领奖',
  COMPLETED: '可领奖',
  FINISHED: '已完成',
  FINISH: '已完成',
  DONE: '已完成',
  SUCCESS: '已完成',
  RECEIVED: '已领取',
  CLAIMED: '已领取',
  AWARDED: '已领取',
  REWARD_RECEIVED: '已领取',
  RECEIVE_SUCCESS: '已领取',
}

function SectionShell({
  title,
  icon,
  badge,
  action,
  children,
  className = '',
}: {
  title: string
  icon: React.ReactNode
  badge?: React.ReactNode
  action?: React.ReactNode
  children: React.ReactNode
  className?: string
}) {
  return (
    <section aria-label={title} className={`${CARD_CLASS} ${className}`} style={CARD_BORDER}>
      <div
        className="flex min-h-[52px] items-center justify-between gap-3 border-b px-4 py-3"
        style={{ borderColor: 'var(--border-light)', background: 'var(--surface-soft)' }}
      >
        <div className="flex min-w-0 items-center gap-2.5">
          <span
            className="flex h-8 w-8 items-center justify-center rounded-xl"
            style={{ background: 'var(--primary-light)', color: 'var(--primary)' }}
          >
            {icon}
          </span>
          <h2 className="truncate text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>{title}</h2>
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
          {badge}
          {action}
        </div>
      </div>
      {children}
    </section>
  )
}

function StatusBadge({
  children,
  tone = 'neutral',
}: {
  children: React.ReactNode
  tone?: 'neutral' | 'blue' | 'green' | 'yellow' | 'red'
}) {
  const palette = {
    neutral: { background: 'var(--surface-sunken)', color: 'var(--text-secondary)' },
    blue: { background: 'var(--primary-light)', color: 'var(--primary)' },
    green: { background: 'var(--status-success-bg)', color: 'var(--status-success-text)' },
    yellow: { background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' },
    red: { background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' },
  }[tone]

  return (
    <span className="inline-flex min-h-6 items-center justify-center rounded-full px-2.5 py-1 text-[11px] font-medium leading-none" style={palette}>
      {children}
    </span>
  )
}

function SkeletonBlock({ lines = 4 }: { lines?: number }) {
  return (
    <div className="space-y-3 animate-pulse">
      {Array.from({ length: lines }).map((_, i) => (
        <div
          key={i}
          className="h-4 rounded-md"
          style={{
            background: 'var(--surface-soft)',
            width: i === lines - 1 ? '55%' : '100%',
          }}
        />
      ))}
    </div>
  )
}

function KpiTile({ label, value, note, hot = false }: { label: string; value: React.ReactNode; note?: string; hot?: boolean }) {
  return (
    <div
      className={[
        'min-w-0 border-b border-r px-3 py-3 last:border-r-0 md:border-b-0 md:px-4',
        hot ? 'col-span-2 md:col-span-1' : '',
      ].filter(Boolean).join(' ')}
      style={{
        borderColor: 'var(--border-light)',
        background: hot ? 'var(--primary-light)' : 'transparent',
      }}
    >
      <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>{label}</div>
      <div className="mt-2 truncate text-[20px] font-semibold leading-none md:text-[24px]" style={{ color: 'var(--text-main)' }}>{value}</div>
      {note ? <div className="mt-2 truncate text-[11px]" style={{ color: 'var(--text-muted)' }}>{note}</div> : null}
    </div>
  )
}

function SimpleRow({
  title,
  subtitle,
  action,
}: {
  title: React.ReactNode
  subtitle?: React.ReactNode
  action?: React.ReactNode
}) {
  return (
    <div
      className="flex items-center justify-between gap-3 rounded-[8px] border px-3 py-2.5"
      style={{ background: 'var(--surface-card-strong)', borderColor: 'var(--border-light)' }}
    >
      <div className="min-w-0">
        <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{title}</div>
        {subtitle ? <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>{subtitle}</div> : null}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  )
}

function taskName(task: JsonRecord, fallback = '任务') {
  return pickAnywhere(task, ['taskName', 'name', 'title', 'taskType', 'desc'], fallback)
}

function hasChineseText(value: string) {
  return /[\u3400-\u9fff]/.test(value)
}

function exposureTaskType(task: JsonRecord) {
  return pickString(task, ['taskType'], pickString(asRecord(task.extend), ['taskType'], '')).trim()
}

function chineseTaskText(task: JsonRecord, keys: string[]) {
  const value = pickAnywhere(task, keys, '')
  return hasChineseText(value) ? value : ''
}

function exposureTaskName(task: JsonRecord, fallback = '曝光任务') {
  return chineseTaskText(task, ['taskName', 'name', 'title'])
    || chineseTaskText(task, ['desc', 'description'])
    || EXPOSURE_TASK_TYPE_LABELS[exposureTaskType(task)]
    || fallback
}

function exposureTaskStatusLabel(task: JsonRecord) {
  if (isClaimedExposureTask(task)) return '已领取'
  const status = pickAnywhere(task, ['taskStatus', 'status', 'idleTaskStatus', 'state'], '')
  if (hasChineseText(status)) return status
  return TASK_STATUS_LABELS[status.trim().toUpperCase()] ?? ''
}

function coinTaskStatusLabel(task: JsonRecord) {
  const status = pickAnywhere(task, ['idleTaskStatus', 'taskStatus', 'status', 'state'], '')
  if (hasChineseText(status)) return status
  return TASK_STATUS_LABELS[status.trim().toUpperCase()] ?? ''
}

function coinReward(task: JsonRecord) {
  return pickAnywhere(task, ['awardCoinNum', 'rewardCoinNum', 'rewardCoin', 'reward', 'amount', 'coinNum', 'coinAmount'], '')
}

function exposureReward(task: JsonRecord) {
  return pickAnywhere(task, ['awardExposureNum', 'awardAmount', 'rewardExposure', 'exposureReward', 'reward', 'awardCoinNum', 'exposureNum', 'amount'], '')
}

function busyKeyMatches(state: PromotionPageState, matcher: (key: string) => boolean) {
  return (state.busyKeys ?? [state.busy]).some(matcher)
}

function signinBusy(state: PromotionPageState) {
  return busyKeyMatches(state, (key) => key === 'signin')
}

function coinTaskBusy(state: PromotionPageState) {
  return busyKeyMatches(state, (key) => key === 'coin-safe-view' || key.startsWith('coin-claim-'))
}

function exposureTaskBusy(state: PromotionPageState) {
  return busyKeyMatches(state, (key) => key === 'safe-view' || key.startsWith('exposure-'))
}

function exposureCreateBusy(state: PromotionPageState, itemId: string) {
  return busyKeyMatches(state, (key) => key === `create-exposure-${itemId}`)
}

const SIGNIN_STATUS_KEYS = [
  'signedIn',
  'signedToday',
  'signed',
  'todaySignIn',
  'todaySigned',
  'signInAwarded',
  'signIn',
  'isSignIn',
  'isSignedIn',
  'hasSigned',
]

const SIGNIN_DAYS_KEYS = ['signInDays', 'alreadySignInDays', 'signDays', 'continuousDays', 'days']

function hasExplicitSigninStatus(signin: JsonRecord | null | undefined) {
  if (!signin) return false
  return SIGNIN_STATUS_KEYS.some((key) => signin[key] !== undefined && signin[key] !== null && String(signin[key]).trim() !== '')
}

function pickSigninSource(signinHome: JsonRecord | null, activityUserInfo: JsonRecord | null) {
  if (hasExplicitSigninStatus(signinHome)) return signinHome
  if (hasExplicitSigninStatus(activityUserInfo)) return activityUserInfo
  return signinHome ?? activityUserInfo
}

function isSignedIn(signin: JsonRecord | null) {
  if (!signin) return false
  return SIGNIN_STATUS_KEYS.some((key) => {
    const value = signin[key]
    return value === true || value === 1 || String(value ?? '').toLowerCase() === 'true'
  })
}

function signInDays(signin: JsonRecord | null, fallback?: JsonRecord | null) {
  const primary = pickString(signin, SIGNIN_DAYS_KEYS, '')
  if (primary) return primary
  return pickString(fallback, SIGNIN_DAYS_KEYS, '—')
}

function TaskActionRow({
  title,
  subtitle,
  reward,
  rewardTone = 'blue',
  done = false,
  action,
}: {
  title: string
  subtitle?: string
  reward?: string
  rewardTone?: 'blue' | 'green'
  done?: boolean
  action?: React.ReactNode
}) {
  return (
    <div
      className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-2 rounded-[8px] border p-2.5"
      style={{
        background: done ? 'rgba(52, 199, 89, 0.06)' : 'var(--surface-card-strong)',
        borderColor: 'var(--border-light)',
      }}
    >
      <div className="min-w-0">
        <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{title}</div>
        {subtitle ? <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>{subtitle}</div> : null}
      </div>
      {reward ? (
        <div
          className="rounded-[8px] px-2.5 py-2 text-center text-[11px] font-semibold leading-none"
          style={{
            background: rewardTone === 'green' ? 'var(--status-success-bg)' : 'var(--primary-light)',
            color: rewardTone === 'green' ? 'var(--status-success-text)' : 'var(--primary)',
          }}
        >
          {reward}
        </div>
      ) : null}
      {action ? <div className="justify-self-end">{action}</div> : null}
    </div>
  )
}

function CoinTaskList({ state }: { state: PromotionPageState }) {
  if (state.growth.coinTasks.length === 0) {
    if (state.growth.coinTasksStatus === 'loading') {
      return <div className="rounded-[10px] p-3" style={{ background: 'var(--surface-soft)' }}><SkeletonBlock lines={4} /></div>
    }
    if (state.growth.coinTasksStatus === 'failed') {
      return (
        <EmptyState
          title="闲鱼币任务加载失败"
          description="接口响应超时或失败，刷新后会重新拉取真实任务。"
          action={<button type="button" className="btn-secondary btn-sm" onClick={() => { void state.refresh() }}>重新加载</button>}
        />
      )
    }
    return <EmptyState title="暂无闲鱼币任务" description="平台当前没有返回该账号今日闲鱼币任务。" />
  }

  return (
    <div className="space-y-2">
      {state.growth.coinTasks.slice(0, 8).map((task, index) => {
        const name = taskName(task, '闲鱼币任务')
        const rewardVal = coinReward(task)
        const progressRaw = pickAnywhere(task, ['progressText', 'progress', 'taskProgress', 'progressPercent', 'progressValue'], '')
        const progressNum = pickAnywhere(task, ['progress', 'progressPercent'], '')
        const progressVal = progressRaw || (progressNum !== '—' ? `${progressNum}%` : '')
        const claimable = canClaimCoinTask(task)
        const completed = isCompletedCoinTask(task)
        const claimed = isClaimedCoinTask(task)
        const batchCompletable = needsCoinTaskBatchComplete(task)
        const status = coinTaskStatusLabel(task)
        const actionLabel = claimed ? '已领取' : completed ? '已完成' : claimable ? '领奖' : batchCompletable ? '完成任务' : status || '待完成'
        const subtitleParts: string[] = []
        if (status && !claimed && !completed) subtitleParts.push(status)
        if (progressVal) subtitleParts.push(progressVal)
        const taskId = pickAnywhere(task, ['taskInstanceId', 'taskId', 'id', 'instanceId'], '')
        return (
          <TaskActionRow
            key={`${taskId}-${index}`}
            title={name}
            subtitle={subtitleParts.join(' · ') || undefined}
            reward={rewardVal ? `+${rewardVal}币` : undefined}
            done={completed || claimed}
            action={(
              <button
                onClick={() => { void state.claimCoinTaskReward(task, index) }}
                aria-label={`${actionLabel} ${name}`}
                disabled={coinTaskBusy(state) || completed || claimed || (!claimable && !batchCompletable)}
                className="btn-secondary btn-sm disabled:opacity-40"
              >
                <Gift className="w-3 h-3 mr-1" />
                {actionLabel}
              </button>
            )}
          />
        )
      })}
    </div>
  )
}

function CoinDetailList({ state }: { state: PromotionPageState }) {
  if (state.growth.coinUserDetailList.length === 0) {
    return <EmptyState title="暂无闲鱼币明细" description="还没有拿到该账号的闲鱼币流水数据。" />
  }
  return (
    <div className="space-y-2">
      {state.growth.coinUserDetailList.slice(0, 8).map((record, index) => {
        const title = pickString(record, ['describe', 'bizType', 'title'], '闲鱼币流水')
        const scoreRaw = Number(record.score ?? record.amount ?? record.coinNum ?? 0)
        const scoreText = Number.isFinite(scoreRaw) ? `${scoreRaw > 0 ? '+' : ''}${scoreRaw}` : pickString(record, ['score', 'amount'], '—')
        const timeText = pickString(record, ['time', 'gmtCreate', 'createTime'], '—')
        return <SimpleRow key={`coin-detail-${index}`} title={title} subtitle={<><SensitiveValue kind="money" value={`${scoreText} 币`} /> · {timeText}</>} />
      })}
    </div>
  )
}

function ExposureTaskList({ state }: { state: PromotionPageState }) {
  if (state.growth.exposureTasks.length === 0) {
    if (state.growth.exposureTasksStatus === 'loading') {
      return <div className="rounded-[10px] p-3" style={{ background: 'var(--surface-soft)' }}><SkeletonBlock lines={4} /></div>
    }
    if (state.growth.exposureTasksStatus === 'failed') {
      return (
        <EmptyState
          title="曝光任务加载失败"
          description="接口响应超时或失败，刷新后会重新拉取真实任务。"
          action={<button type="button" className="btn-secondary btn-sm" onClick={() => { void state.refresh() }}>重新加载</button>}
        />
      )
    }
    return <EmptyState title="暂无鱼小铺曝光任务" description="平台当前没有返回该账号今日鱼小铺曝光任务。" />
  }

  return (
    <div className="space-y-2">
      {state.growth.exposureTasks.slice(0, 8).map((task, index) => {
        const name = exposureTaskName(task)
        const status = exposureTaskStatusLabel(task)
        const rewardExp = exposureReward(task)
        const progressRaw = pickAnywhere(task, ['progressText', 'progress', 'taskProgress', 'progressPercent', 'progressValue'], '')
        const taskId = pickAnywhere(task, ['taskInstanceId', 'taskId', 'id', 'instanceId'], '')
        const claimable = canReceiveExposureTask(task)
        const batchCompletable = needsExposureTaskBatchComplete(task)
        const claimed = isClaimedExposureTask(task)
        const subtitleParts: string[] = []
        if (status) subtitleParts.push(status)
        if (progressRaw) subtitleParts.push(progressRaw)
        const actionLabel = claimed ? '已领取' : claimable ? '领奖' : batchCompletable ? '完成任务' : '已完成'
        return (
          <TaskActionRow
            key={`${taskId}-${index}`}
            title={name}
            subtitle={subtitleParts.join(' · ') || undefined}
            reward={rewardExp ? `+${rewardExp}曝光` : undefined}
            rewardTone="green"
            done={claimed || (!claimable && !batchCompletable)}
            action={(
              <button
                onClick={() => { void state.receiveExposureTaskReward(task, index) }}
                aria-label={`${actionLabel} ${name}`}
                disabled={exposureTaskBusy(state) || claimed || (!claimable && !batchCompletable)}
                className="btn-secondary btn-sm disabled:opacity-40"
              >
                {actionLabel}
              </button>
            )}
          />
        )
      })}
    </div>
  )
}

function ExposureDetailList({ state }: { state: PromotionPageState }) {
  const rows = state.growth.exposureHistory.length > 0
    ? state.growth.exposureHistory
    : getClaimedExposureRewardHistoryRows(state.growth.exposureTasks)
  if (rows.length === 0) {
    return <EmptyState title="暂无曝光明细" description="还没有拿到该账号的鱼小铺曝光明细。" />
  }
  return (
    <div className="space-y-2">
      {rows.slice(0, 8).map((record, index) => {
        const title = pickString(record, ['itemTitle', 'title', 'itemName'], pickString(record, ['itemId'], '曝光明细'))
        const exposureNum = pickString(record, ['exposureNum', 'exposureCnt', 'cnt', 'consumedExposure'], '—')
        const date = pickString(record, ['date', 'ds', 'gmtCreate', 'createTime'], '—')
        return <SimpleRow key={`exposure-detail-${index}`} title={<SensitiveValue kind="item-title" value={title} />} subtitle={`${exposureNum} 曝光 · ${date}`} />
      })}
    </div>
  )
}

function TodayTaskBoard({ state }: { state: PromotionPageState }) {
  const { isMobile } = useDevice()
  const [coinTab, setCoinTab] = useState<'tasks' | 'details'>('tasks')
  const [exposureTab, setExposureTab] = useState<'tasks' | 'details'>('tasks')
  const signin = pickSigninSource(state.growth.signin, state.growth.coinActivityUserInfo)
  const todaySigned = isSignedIn(signin)
  const days = signInDays(signin, state.growth.coinActivityUserInfo)
  const coinCompletableCount = state.growth.coinTasks.filter(needsCoinTaskBatchComplete).length
  const coinClaimableCount = state.growth.coinTasks.filter(canClaimCoinTask).length
  const exposureCompletableCount = state.growth.exposureTasks.filter((task) => needsExposureTaskBatchComplete(task)).length
  const exposureClaimableCount = state.growth.exposureTasks.filter(canReceiveExposureTask).length

  if (state.loading) {
    return (
      <SectionShell title="今日任务台" icon={<Coins className="h-4 w-4" />}>
        <div className="p-4"><SkeletonBlock lines={7} /></div>
      </SectionShell>
    )
  }

  if (!state.selectedUid) {
    return (
      <SectionShell title="今日任务台" icon={<Coins className="h-4 w-4" />}>
        <div className="p-4"><EmptyState title="请先选择账号" description="在上方选择一个已登录账号查看今日任务。" /></div>
      </SectionShell>
    )
  }

  return (
    <SectionShell
      title="今日任务台"
      icon={<Coins className="h-4 w-4" />}
      badge={(
        <>
          <StatusBadge tone="blue">闲鱼币可处理 {coinCompletableCount + coinClaimableCount}</StatusBadge>
          <StatusBadge tone="green">曝光可处理 {exposureCompletableCount + exposureClaimableCount}</StatusBadge>
        </>
      )}
    >
      <div className={isMobile ? 'space-y-3 p-3' : 'grid lg:grid-cols-[0.9fr_1.2fr_1.25fr]'}>
        <div
          role="group"
          aria-label="签到领闲鱼币区"
          className={`${TASK_COLUMN_CLASS} ${isMobile ? 'rounded-[16px] border p-3' : 'lg:border-r'}`}
          style={{
            ...HAIRLINE_BORDER,
            background: 'linear-gradient(180deg, rgba(255,255,255,0.78), var(--primary-light))',
          }}
        >
          <div className="mb-3 flex items-center justify-between gap-2">
            <h3 className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>签到领闲鱼币</h3>
            <StatusBadge tone={todaySigned ? 'green' : 'blue'}>{todaySigned ? '已签到' : '未签到'}</StatusBadge>
          </div>
          <div className={isMobile ? 'min-h-0' : TASK_COLUMN_BODY_CLASS}>
            <div className="grid grid-cols-[56px_minmax(0,1fr)] items-center gap-3 md:grid-cols-[64px_minmax(0,1fr)]">
              <div
                className="grid h-14 w-14 place-items-center rounded-full md:h-16 md:w-16"
                style={{
                  background: todaySigned
                    ? 'conic-gradient(var(--success) 0 360deg, var(--surface-sunken) 0)'
                    : 'conic-gradient(var(--primary) 0 56deg, var(--surface-sunken) 56deg 360deg)',
                }}
              >
                <span className="grid h-10 w-10 place-items-center rounded-full text-[12px] font-semibold md:h-12 md:w-12 md:text-[13px]" style={{ background: 'var(--surface-card-strong)', color: 'var(--text-main)' }}>
                  {days}天
                </span>
              </div>
              <div className="min-w-0">
                <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                  {todaySigned ? '今日签到状态已同步' : '今日可签到领取闲鱼币'}
                </div>
                <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                  连续签到、今日状态和奖励流水会在操作完成后重新拉取。
                </div>
                <button
                  onClick={() => { void state.runCoinSignin() }}
                  disabled={signinBusy(state) || todaySigned}
                  className="btn-primary btn-sm mt-3 disabled:opacity-40"
                >
                  <BadgeCheck className="w-3.5 h-3.5 mr-1" />
                  {todaySigned ? '已签到' : '签到领币'}
                </button>
              </div>
            </div>
            <div className="mt-4 grid grid-cols-2 gap-2">
              <div className="rounded-[8px] border px-3 py-2" style={{ background: 'var(--surface-card-strong)', borderColor: 'var(--border-light)' }}>
                <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>余额</div>
                <div className="mt-1 text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}><SensitiveValue kind="money" value={state.growth.coinBalance} /></div>
              </div>
              <div className="rounded-[8px] border px-3 py-2" style={{ background: 'var(--surface-card-strong)', borderColor: 'var(--border-light)' }}>
                <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>今日状态</div>
                <div className="mt-1 text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>{todaySigned ? '已签到' : '未签到'}</div>
              </div>
            </div>
          </div>
        </div>

        <div role="group" aria-label="闲鱼币任务区" className={`${TASK_COLUMN_CLASS} ${isMobile ? 'rounded-[12px] border p-3' : 'lg:border-r'}`} style={HAIRLINE_BORDER}>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h3 className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>闲鱼币任务</h3>
            <div className="flex flex-wrap items-center gap-2">
              <StatusBadge tone="blue">{coinCompletableCount + coinClaimableCount} 可处理</StatusBadge>
              <button
                onClick={() => { void state.completeCoinTaskViews(state.growth.coinTasks) }}
                disabled={coinTaskBusy(state) || (coinCompletableCount + coinClaimableCount) === 0}
                className="btn-primary btn-sm disabled:opacity-40"
              >
                <Gift className="w-3.5 h-3.5 mr-1" />
                一键领币
              </button>
            </div>
          </div>
          <div className={isMobile ? 'max-h-[260px] overflow-y-auto scrollbar-hidden pr-1' : TASK_COLUMN_BODY_CLASS}>
            <div className="mb-3 flex rounded-[8px] p-0.5" style={{ background: 'var(--surface-soft)' }}>
              {([
                ['tasks', '任务'],
                ['details', '明细'],
              ] as const).map(([tab, label]) => (
                <button
                  key={tab}
                  onClick={() => {
                    setCoinTab(tab)
                    if (tab === 'details' && state.growth.coinUserDetailList.length === 0) {
                      void state.refreshCoinDetails()
                    }
                  }}
                  className="rounded-[7px] px-3 py-1.5 text-[12px] font-medium"
                  style={{
                    background: coinTab === tab ? 'var(--surface-card)' : 'transparent',
                    color: coinTab === tab ? 'var(--text-main)' : 'var(--text-muted)',
                  }}
                >
                  {label}
                </button>
              ))}
            </div>
            {coinTab === 'tasks' ? <CoinTaskList state={state} /> : null}
            {coinTab === 'details' ? <CoinDetailList state={state} /> : null}
          </div>
        </div>

        <div role="group" aria-label="鱼小铺曝光任务区" className={`${TASK_COLUMN_CLASS} ${isMobile ? 'rounded-[12px] border p-3' : ''}`} style={isMobile ? HAIRLINE_BORDER : undefined}>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h3 className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>鱼小铺曝光任务</h3>
            <div className="flex flex-wrap items-center gap-2">
              <StatusBadge tone="green">{exposureClaimableCount} 可领</StatusBadge>
              <button
                onClick={() => { void state.completeExposureViewTasks(state.growth.exposureTasks) }}
                disabled={exposureTaskBusy(state) || exposureCompletableCount === 0}
                className="btn-primary btn-sm disabled:opacity-40"
              >
                一键领取曝光
              </button>
            </div>
          </div>
          <div className={isMobile ? 'max-h-[260px] overflow-y-auto scrollbar-hidden pr-1' : TASK_COLUMN_BODY_CLASS}>
            <div className="mb-3 flex rounded-[8px] p-0.5" style={{ background: 'var(--surface-soft)' }}>
              {([
                ['tasks', '任务'],
                ['details', '明细'],
              ] as const).map(([tab, label]) => (
                <button
                  key={tab}
                  onClick={() => {
                    setExposureTab(tab)
                    if (tab === 'details' && state.growth.exposureHistory.length === 0) {
                      void state.refreshExposureHistory()
                    }
                  }}
                  className="rounded-[7px] px-3 py-1.5 text-[12px] font-medium"
                  style={{
                    background: exposureTab === tab ? 'var(--surface-card)' : 'transparent',
                    color: exposureTab === tab ? 'var(--text-main)' : 'var(--text-muted)',
                  }}
                >
                  {label}
                </button>
              ))}
            </div>
            {exposureTab === 'tasks' ? <ExposureTaskList state={state} /> : null}
            {exposureTab === 'details' ? <ExposureDetailList state={state} /> : null}
          </div>
        </div>
      </div>
    </SectionShell>
  )
}

function itemIdOf(item: JsonRecord) {
  return pickString(item, ['item_id', 'itemId', 'id'], '')
}

function itemTitleOf(item: JsonRecord, fallback = '未命名商品') {
  return pickString(item, ['title', 'itemTitle', 'name', 'desc'], itemIdOf(item) || fallback)
}

function itemPriceOf(item: JsonRecord) {
  return pickString(item, ['price', 'priceText', 'soldPrice'], '')
}

function itemStatusOf(item: JsonRecord) {
  const raw = pickString(item, ['status', 'itemStatus', 'statusDesc'], '')
  if (!raw || raw === '—') return ''
  if (hasChineseText(raw)) return raw
  const normalized = raw.trim().toUpperCase()
  const statusMap: Record<string, string> = {
    SOLD_OUT: '已售罄',
    OFF_SALE: '已下架',
    SOLD: '已售出',
  }
  return statusMap[normalized] ?? ''
}

function itemImageOf(item: JsonRecord) {
  return pickString(item, ['picUrl', 'pic_url', 'imageUrl', 'image', 'majorPic', 'mainPic', 'coverUrl', 'cover'], '')
}

function promotionItemIdOf(record: JsonRecord) {
  const itemVO = asRecord(record.itemVO)
  const itemDTO = asRecord(record.itemDTO)
  const detail = asRecord(record.detail)
  return pickString(record, ['itemId', 'item_id'], '')
    || pickString(itemVO, ['itemId', 'item_id'], '')
    || pickString(itemDTO, ['itemId', 'item_id'], '')
    || pickString(detail, ['itemId', 'item_id'], '')
}

function promotionForItem(promotions: JsonRecord[], itemId: string) {
  if (!itemId) return null
  return promotions.find((promotion) => promotionItemIdOf(asRecord(promotion)) === itemId) ?? null
}

function coinPromotionStatusLabel(promotion: JsonRecord) {
  if (promotion.pendingLocal === true) return '已开启'
  const text = pickString(promotion, ['status', 'statusDescTitle', 'statusDesc'], '')
  return hasChineseText(text) && !text.includes('等待远端同步') ? text : '已开启'
}

function ProductThumb({ item, title }: { item: JsonRecord; title: string }) {
  const image = itemImageOf(item)
  const safeAlt = useSensitiveText('item-title', title)
  return (
    <div className="h-[54px] w-[54px] shrink-0 overflow-hidden rounded-[8px] border" style={{ background: 'var(--surface-sunken)', borderColor: 'var(--border-light)' }}>
      {image ? (
        <SensitiveItemImage src={image} alt={safeAlt} className="h-full w-full object-cover" />
      ) : (
        <div className="grid h-full w-full place-items-center">
          <ImageIcon className="h-4 w-4" style={{ color: 'var(--text-tertiary)' }} />
        </div>
      )}
    </div>
  )
}

function packageCost(pkg: JsonRecord) {
  return pickNumber(pkg, ['costCoinNum', 'cost', 'coinNum', 'nowPrice'])
}

function packageExposure(pkg: JsonRecord) {
  return pickNumber(pkg, ['exposureNum', 'maxPageView', 'promotionNum'])
}

function packageDays(pkg: JsonRecord) {
  return pickNumber(pkg, ['days', 'durationDays'], '')
}

function numberFromDisplay(raw: unknown) {
  const normalized = String(raw ?? '').replace(/[^\d.-]/g, '').trim()
  if (!normalized || normalized === '-' || normalized === '.' || normalized === '-.') return null
  const value = Number(normalized)
  return Number.isFinite(value) ? value : null
}

type PackagePickerOption = {
  key: string
  label: string
  meta?: string
}

function SensitivePackageOption({ option }: { option: PackagePickerOption }) {
  const label = useSensitiveText('money', option.meta ? `${option.label} · ${option.meta}` : option.label)
  return <option value={option.key}>{label}</option>
}

function NativePackageSelect({
  ownerLabel,
  options,
  selectedKey,
  emptyLabel,
  ariaPrefix,
  onSelect,
}: {
  ownerLabel: string
  options: PackagePickerOption[]
  selectedKey: string
  emptyLabel: string
  ariaPrefix: string
  onSelect: (key: string) => void
}) {
  const safeOwnerLabel = useSensitiveText('item-title', ownerLabel)
  if (options.length === 0) {
    return (
      <select
        aria-label={`${ariaPrefix} ${safeOwnerLabel}`}
        disabled
        value=""
        className="h-9 w-full min-w-0 rounded-[8px] border px-2.5 text-[12px] font-medium disabled:opacity-70"
        style={{ background: 'var(--surface-soft)', borderColor: 'var(--border-light)', color: 'var(--text-secondary)' }}
      >
        <option value="">{emptyLabel}</option>
      </select>
    )
  }

  const selected = options.find((option) => option.key === selectedKey) ?? options[0]
  return (
    <div className="min-w-0">
      <select
        aria-label={`${ariaPrefix} ${safeOwnerLabel}`}
        value={selected.key}
        onChange={(event) => onSelect(event.target.value)}
        className="h-9 w-full min-w-0 rounded-[8px] border px-2.5 text-[12px] font-semibold outline-none"
        style={{
          background: 'var(--surface-soft)',
          borderColor: 'color-mix(in srgb, var(--primary) 18%, var(--border-light))',
          color: 'var(--text-main)',
        }}
      >
        {options.map((option) => <SensitivePackageOption key={option.key} option={option} />)}
      </select>
      {selected.meta ? <div className="mt-1 break-words text-[10px] leading-4" style={{ color: 'var(--text-muted)' }}>{selected.meta}</div> : null}
    </div>
  )
}

function coinPackageOptions(packages: JsonRecord[]): PackagePickerOption[] {
  return packages.map((pkg, index) => {
    const num = pickString(pkg, ['promotionNum'], String(index + 1))
    const days = packageDays(pkg)
    const exposure = packageExposure(pkg)
    const cost = packageCost(pkg)
    const name = pickString(pkg, ['name', 'title'], '')
    return {
      key: num,
      label: `${exposure}曝光 / ${cost}币`,
      meta: name || (days ? `${days}天` : ''),
    }
  })
}

function exposureAmountOptions(options: JsonRecord[]): PackagePickerOption[] {
  const rows: PackagePickerOption[] = []
  options.forEach((option, index) => {
    const amount = Number(pickNumber(option, ['exposureNum', 'amount', 'num'], '0'))
    if (!Number.isFinite(amount) || amount <= 0) return
    const label = pickString(option, ['title', 'name', 'desc'], index === 0 ? '平台档位' : `档位 ${index + 1}`)
    rows.push({
      key: String(amount),
      label: `${amount} 普通曝光`,
      meta: label,
    })
  })
  return rows
}

function PromotionItemRow({
  state,
  item,
}: {
  state: PromotionPageState
  item: PromotionPageState['promotionItems'][number]
}) {
  const record = asRecord(item)
  const itemId = itemIdOf(record)
  const title = itemTitleOf(record)
  const safeTitle = useSensitiveText('item-title', title)
  const price = itemPriceOf(record)
  const status = itemStatusOf(record)
  const view = state.promotionItemViews[itemId] ?? null
  const packages = getPromotionPackages(view)
  const selectedPromotionNum = state.promotionItemSelectedNums[itemId] || String(packages[0]?.promotionNum ?? '')
  const selectedPackage = packages.find((pkg) => String(pkg.promotionNum ?? '') === selectedPromotionNum) ?? packages[0]
  const pickerOptions = coinPackageOptions(packages)
  const packageLoading = state.promotionItemPackageLoading[itemId] === true
  const packageQueried = Object.prototype.hasOwnProperty.call(state.promotionItemViews, itemId)
  const activePromotion = promotionForItem(state.growth.promotions, itemId)
  const activePromotionStatus = activePromotion ? coinPromotionStatusLabel(asRecord(activePromotion)) : ''
  const packageStatus = activePromotionStatus || (packages.length > 0 ? '可开启' : packageLoading || !packageQueried ? '套餐加载中' : '暂无套餐')

  return (
    <div
      className="grid min-w-0 grid-cols-[54px_minmax(0,1fr)] items-center gap-3 border-b px-3 py-3 last:border-b-0 md:grid-cols-[56px_minmax(0,1.1fr)_minmax(180px,1.35fr)_82px_70px] md:px-4"
      style={{
        borderColor: 'var(--border-light)',
      background: packages.length > 0 ? 'color-mix(in srgb, var(--primary) 5%, var(--surface-card-strong))' : 'var(--surface-card-strong)',
      }}
    >
      <ProductThumb item={record} title={title} />
      <div className="min-w-0">
        <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}><SensitiveValue kind="item-title" value={title} /></div>
        <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
          商品 ID <SensitiveValue kind="order-id" value={itemId} />
          {price && price !== '—' ? <> · ¥<SensitiveValue kind="money" value={price} /></> : null}
          {status && status !== '—' ? <> · {status}</> : null}
        </div>
      </div>
      <div className="col-span-2 md:col-span-1">
        <NativePackageSelect
          ownerLabel={title}
          options={pickerOptions}
          selectedKey={selectedPromotionNum}
          emptyLabel={packageLoading || !packageQueried ? '加载套餐' : '暂无套餐'}
          ariaPrefix="选择套餐"
          onSelect={(num) => state.setPromotionItemPromotionNum(record, num)}
        />
      </div>
      <div className="col-span-1 md:col-span-1">
        <StatusBadge tone={activePromotion ? 'green' : packages.length > 0 ? 'green' : packageLoading ? 'blue' : 'neutral'}>{packageStatus}</StatusBadge>
      </div>
      {activePromotion ? (
        <span className="btn-secondary btn-sm col-span-1 shrink-0 opacity-70 md:col-span-1">已开启</span>
      ) : packages.length === 0 ? (
        <span className="btn-secondary btn-sm col-span-1 shrink-0 opacity-60 md:col-span-1">{packageLoading || !packageQueried ? '加载中' : '暂无套餐'}</span>
      ) : (
        <button
          onClick={() => { void state.submitPromotionForItem(item) }}
          aria-label={`开启推广 ${safeTitle}`}
          disabled={!itemId || !!state.busy || !selectedPackage}
          className="btn-primary btn-sm col-span-1 shrink-0 disabled:opacity-40 md:col-span-1"
        >
          开启
        </button>
      )}
    </div>
  )
}

function CoinPromotionBuilder({ state }: { state: PromotionPageState }) {
  const selected = useMemo(() => {
    return state.promotionItems.find((item) => {
      const id = itemIdOf(asRecord(item))
      return id && getPromotionPackages(state.promotionItemViews[id] ?? null).length > 0
    }) ?? state.promotionItems[0]
  }, [state.promotionItemViews, state.promotionItems])
  const selectedRecord = asRecord(selected)
  const selectedId = itemIdOf(selectedRecord)
  const selectedPackages = getPromotionPackages(state.promotionItemViews[selectedId] ?? null)
  const selectedNum = state.promotionItemSelectedNums[selectedId] || String(selectedPackages[0]?.promotionNum ?? '')
  const selectedPackage = selectedPackages.find((pkg) => String(pkg.promotionNum ?? '') === selectedNum) ?? selectedPackages[0]
  const selectedPackageLoading = selectedId ? state.promotionItemPackageLoading[selectedId] === true : false
  const selectedPackageLabel = selectedNum
    ? `当前选择 ${selectedNum} 档`
    : selectedPackageLoading
      ? '套餐自动读取中'
      : '暂无可用套餐'

  return (
    <SectionShell
      title="闲鱼币推广"
      icon={<Megaphone className="h-4 w-4" />}
      badge={<StatusBadge tone="blue">选商品 · 选套餐 · 确认扣币</StatusBadge>}
      className="min-w-0"
      action={(
        <button
          onClick={() => { void state.refreshPromotionItems() }}
          disabled={!!state.busy || state.promotionItemsLoading}
          className="btn-ghost btn-sm disabled:opacity-40"
        >
          <RefreshCw className={`w-3.5 h-3.5 mr-1 ${state.promotionItemsLoading ? 'animate-spin' : ''}`} />
          商品
        </button>
      )}
    >
      <div
        className="grid gap-3 border-b px-4 py-3 lg:grid-cols-[minmax(0,1fr)_132px_132px]"
        style={{ borderColor: 'var(--border-light)', background: 'linear-gradient(180deg, rgba(255,255,255,0.90), var(--primary-light))' }}
      >
        <div className="min-w-0">
          <div className="truncate text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>
            {selected ? <SensitiveValue kind="item-title" value={itemTitleOf(selectedRecord)} /> : '请选择商品查询推广套餐'}
          </div>
          <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
            {selectedId
              ? <>商品 ID <SensitiveValue kind="order-id" value={selectedId} /> · {selectedPackageLabel}</>
              : '会展示当前账号在售商品，套餐从闲鱼币推广真实接口读取。'}
          </div>
        </div>
        <div className="rounded-[12px] border px-3 py-2" style={{ background: 'rgba(255,255,255,0.62)', borderColor: 'var(--border-light)' }}>
          <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>预计扣币</div>
          <div className="mt-1 text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}><SensitiveValue kind="money" value={selectedPackage ? packageCost(selectedPackage) : '—'} /></div>
        </div>
        <div className="rounded-[12px] border px-3 py-2" style={{ background: 'rgba(255,255,255,0.62)', borderColor: 'var(--border-light)' }}>
          <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>预计曝光</div>
          <div className="mt-1 text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}><SensitiveValue kind="money" value={selectedPackage ? packageExposure(selectedPackage) : '—'} /></div>
        </div>
      </div>
      <div>
        <div
          className="hidden border-b px-4 py-2 text-center text-[11px] font-semibold md:grid md:grid-cols-[56px_minmax(0,1.1fr)_minmax(180px,1.35fr)_82px_70px] md:place-items-center md:gap-3"
          style={{ borderColor: 'var(--border-light)', color: 'var(--text-secondary)', background: 'var(--surface-soft)' }}
        >
          <span className="w-full text-center">图片</span>
          <span className="w-full text-center">商品</span>
          <span className="w-full text-center">套餐</span>
          <span className="w-full text-center">状态</span>
          <span className="w-full text-center">操作</span>
        </div>
        {state.promotionItemsLoading ? (
          <div className="p-4"><SkeletonBlock lines={5} /></div>
        ) : state.promotionItems.length > 0 ? (
          state.promotionItems.slice(0, 8).map((item) => (
            <PromotionItemRow key={itemIdOf(asRecord(item))} state={state} item={item} />
          ))
        ) : (
          <div className="p-4"><EmptyState title="暂无在售商品" description="刷新商品列表后，会展示当前账号可查询闲鱼币推广套餐的商品。" /></div>
        )}
      </div>
    </SectionShell>
  )
}

function FishShopExposureBuilder({ state }: { state: PromotionPageState }) {
  const isSensitiveHidden = useSensitiveHidden()
  const [selectedItemId, setSelectedItemId] = useState('')
  const amountOptions = state.exposureAmountList
  const displayAmountOptions = amountOptions.length ? amountOptions : [{ exposureNum: 100, title: '默认档位' }]
  const amountValues = displayAmountOptions
    .map((item) => Number(pickNumber(item, ['exposureNum', 'amount', 'num'], '0')))
    .filter((value) => Number.isFinite(value) && value > 0)
  const [selectedAmount, setSelectedAmount] = useState(0)
  const effectiveSelectedAmount = amountValues.includes(selectedAmount)
    ? selectedAmount
    : amountValues.includes(100)
      ? 100
      : amountValues[0] ?? 0
  const exposureItems = state.growth.exposeableItems.length > 0 ? state.growth.exposeableItems : state.promotionItems
  const fallbackItem = exposureItems[0]
  const selectedItem = exposureItems.find((item) => itemIdOf(asRecord(item)) === selectedItemId) ?? fallbackItem
  const selectedRecord = asRecord(selectedItem)
  const selectedId = itemIdOf(selectedRecord)
  const available = state.exposureUserAccount ?? numberFromDisplay(state.exposureFlow)
  const insufficient = available !== null && effectiveSelectedAmount > 0 && available < effectiveSelectedAmount
  const disabledReason = !state.exposureHasQuantity
    ? '今日曝光已抢完'
    : insufficient
      ? '可用曝光不足'
        : ''

  return (
    <SectionShell
      title="鱼小铺曝光推广"
      icon={<Store className="h-4 w-4" />}
      badge={<StatusBadge tone={disabledReason ? 'yellow' : 'green'}>{disabledReason || '选商品 · 选曝光量 · 确认扣曝光'}</StatusBadge>}
      className="min-w-0"
    >
      <div
        className="grid gap-3 border-b px-4 py-3 md:grid-cols-[minmax(0,1fr)_120px]"
        style={{ borderColor: 'var(--border-light)', background: 'linear-gradient(180deg, rgba(255,255,255,0.90), var(--primary-light))' }}
      >
        <div className="min-w-0">
          <div className="truncate text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}>
            {selectedItem ? <SensitiveValue kind="item-title" value={itemTitleOf(selectedRecord)} /> : '选择商品开启普通曝光'}
          </div>
          <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
            {selectedId
              ? <>商品 ID <SensitiveValue kind="order-id" value={selectedId} /> · 当前可用 <SensitiveValue kind="money" value={state.exposureFlow} /> 曝光 · 每天每人可加曝光1次 · 预计3天内推广完成</>
              : '曝光余额和档位从平台真实接口读取。'}
          </div>
        </div>
        <div className="rounded-[12px] border px-3 py-2" style={{ background: 'rgba(255,255,255,0.62)', borderColor: 'var(--border-light)' }}>
          <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>消耗曝光</div>
          <div className="mt-1 text-[15px] font-semibold" style={{ color: 'var(--text-main)' }}><SensitiveValue kind="money" value={effectiveSelectedAmount || '—'} /></div>
        </div>
      </div>
      <div>
        <div
          className="hidden border-b px-4 py-2 text-center text-[11px] font-semibold md:grid md:grid-cols-[56px_minmax(0,1.1fr)_minmax(180px,1.35fr)_82px_76px] md:place-items-center md:gap-3"
          style={{ borderColor: 'var(--border-light)', color: 'var(--text-secondary)', background: 'var(--surface-soft)' }}
        >
          <span className="w-full text-center">图片</span>
          <span className="w-full text-center">商品</span>
          <span className="w-full text-center">曝光量</span>
          <span className="w-full text-center">状态</span>
          <span className="w-full text-center">操作</span>
        </div>
        {state.promotionItemsLoading ? (
          <div className="p-4"><SkeletonBlock lines={4} /></div>
        ) : exposureItems.length > 0 ? (
          exposureItems.slice(0, 6).map((item) => {
            const record = asRecord(item)
            const itemId = itemIdOf(record)
            const title = itemTitleOf(record)
            const safeTitle = isSensitiveHidden ? maskSensitiveText('item-title', title) : title
            const price = itemPriceOf(record)
            const isSelected = itemId === selectedId
            return (
              <div
                key={`exposure-${itemId}`}
                className="grid min-w-0 grid-cols-[54px_minmax(0,1fr)] items-center gap-3 border-b px-3 py-3 last:border-b-0 md:grid-cols-[56px_minmax(0,1.1fr)_minmax(180px,1.35fr)_82px_76px] md:px-4"
                style={{
                  borderColor: 'var(--border-light)',
                  background: isSelected ? 'rgba(52,199,89,0.06)' : 'var(--surface-card-strong)',
                }}
              >
                <ProductThumb item={record} title={title} />
                <div className="min-w-0">
                  <div className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}><SensitiveValue kind="item-title" value={title} /></div>
                  <div className="mt-1 text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
                    {price && price !== '—' ? <>¥<SensitiveValue kind="money" value={price} /> · </> : null}商品 ID <SensitiveValue kind="order-id" value={itemId || '—'} />
                  </div>
                </div>
                <div className="col-span-2 md:col-span-1">
                  <NativePackageSelect
                    ownerLabel={title}
                    options={exposureAmountOptions(displayAmountOptions)}
                    selectedKey={String(effectiveSelectedAmount || '')}
                    emptyLabel="暂无曝光档位"
                    ariaPrefix="选择曝光量"
                    onSelect={(amount) => {
                      setSelectedItemId(itemId)
                      setSelectedAmount(Number(amount))
                    }}
                  />
                </div>
                <div className="col-span-1 md:col-span-1">
                  <StatusBadge tone={disabledReason ? 'yellow' : 'green'}>{disabledReason || '可开启'}</StatusBadge>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    setSelectedItemId(itemId)
                    void state.submitExposureForItem(item, effectiveSelectedAmount)
                  }}
                  disabled={!itemId || exposureCreateBusy(state, itemId) || !!disabledReason || !effectiveSelectedAmount}
                  aria-label={`开启曝光 ${safeTitle}`}
                  className="btn-primary btn-sm col-span-1 disabled:opacity-40 md:col-span-1"
                >
                  开启曝光
                </button>
              </div>
            )
          })
        ) : (
          <div className="p-4"><EmptyState title="暂无可选商品" description="刷新商品列表后，会展示当前账号可开启鱼小铺曝光的商品。" /></div>
        )}
      </div>
    </SectionShell>
  )
}

export function PromotionContent({ state }: { state: PromotionPageState }) {
  const { isMobile } = useDevice()
  const signin = pickSigninSource(state.growth.signin, state.growth.coinActivityUserInfo)
  return (
    <div className={isMobile ? 'space-y-3' : 'space-y-4'}>
      <div className="grid grid-cols-2 overflow-hidden rounded-[20px] border surface-card md:grid-cols-5" style={CARD_BORDER}>
        <KpiTile label="闲鱼币余额" value={<SensitiveValue kind="money" value={state.growth.coinBalance} />} note="可用于闲鱼币推广" hot />
        <KpiTile label="今日签到" value={isSignedIn(signin) ? '已签' : '未签'} note={`${signInDays(signin, state.growth.coinActivityUserInfo)} 天连续`} />
        <KpiTile label="闲鱼币任务" value={String(state.growth.coinTasks.length)} note="可处理任务数" />
        <KpiTile label="鱼小铺曝光" value={<SensitiveValue kind="money" value={state.exposureFlow} />} note="当前可用" />
        <KpiTile label="已购推广" value={String(state.growth.promotions.length)} note="平台返回记录" />
      </div>

      <TodayTaskBoard state={state} />

      <div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <CoinPromotionBuilder state={state} />
        <FishShopExposureBuilder state={state} />
      </div>
    </div>
  )
}

export function PromotionRefreshButton({ state }: { state: PromotionPageState }) {
  return (
    <button
      onClick={state.refresh}
      disabled={!state.selectedUid || state.loading}
      className="btn-ghost btn-sm disabled:opacity-40"
    >
      <RefreshCw className={`w-3.5 h-3.5 ${state.loading ? 'animate-spin' : ''}`} />
      刷新
    </button>
  )
}
