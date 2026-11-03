import { request } from './client'

export type AutomationAction =
  | 'batch_reduce_price'
  | 'batch_restore_price'
  | 'batch_polish'
  | 'coin_signin'
  | 'coin_tasks'
  | 'exposure_tasks'
  | 'coin_promotion'
  | 'exposure_promotion'

export interface AutomationSchedule {
  id: string
  ownerUsername: string
  name: string
  action: AutomationAction
  enabled: boolean
  timeOfDay: string
  timezone: string
  accountScope: 'all' | 'selected'
  accountUids: string[]
  itemScope: 'all' | 'selected'
  itemIds: string[]
  config: Record<string, unknown>
  lastRunAt: number
  lastStatus: string
  lastMessage: string
  nextRunAt: number
  createdAt: number
  updatedAt: number
}

export type SaveAutomationSchedulePayload = Omit<
  AutomationSchedule,
  'ownerUsername' | 'lastRunAt' | 'lastStatus' | 'lastMessage' | 'nextRunAt' | 'createdAt' | 'updatedAt'
>

function asRecord(value: unknown): Record<string, any> {
  return value && typeof value === 'object' ? value as Record<string, any> : {}
}

function stringList(value: unknown): string[] {
  return Array.isArray(value) ? value.map((item) => String(item)).filter(Boolean) : []
}

function normalizeSchedule(value: unknown): AutomationSchedule {
  const item = asRecord(value)
  return {
    id: String(item.id ?? ''),
    ownerUsername: String(item.ownerUsername ?? item.owner_username ?? ''),
    name: String(item.name ?? ''),
    action: String(item.action ?? 'batch_reduce_price') as AutomationAction,
    enabled: Boolean(item.enabled),
    timeOfDay: String(item.timeOfDay ?? item.time_of_day ?? '09:00'),
    timezone: String(item.timezone ?? 'Asia/Shanghai'),
    accountScope: String(item.accountScope ?? item.account_scope ?? 'all') as 'all' | 'selected',
    accountUids: stringList(item.accountUids ?? item.account_uids),
    itemScope: String(item.itemScope ?? item.item_scope ?? 'all') as 'all' | 'selected',
    itemIds: stringList(item.itemIds ?? item.item_ids),
    config: asRecord(item.config),
    lastRunAt: Number(item.lastRunAt ?? item.last_run_at ?? 0),
    lastStatus: String(item.lastStatus ?? item.last_status ?? ''),
    lastMessage: String(item.lastMessage ?? item.last_message ?? ''),
    nextRunAt: Number(item.nextRunAt ?? item.next_run_at ?? 0),
    createdAt: Number(item.createdAt ?? item.created_at ?? 0),
    updatedAt: Number(item.updatedAt ?? item.updated_at ?? 0),
  }
}

export const listAutomationSchedules = async () => {
  const response = await request<{ items: unknown[]; count: number }>('GET', '/api/automation/schedules')
  const items = Array.isArray(response.items) ? response.items.map(normalizeSchedule) : []
  return { ...response, items, count: Number(response.count ?? items.length) }
}

export const saveAutomationSchedule = async (payload: Partial<SaveAutomationSchedulePayload>) => {
  const response = await request<{ item: unknown }>('POST', '/api/automation/schedules', payload)
  return { ...response, item: normalizeSchedule(response.item) }
}

export const deleteAutomationSchedule = (id: string) =>
  request<{ deleted: boolean }>('DELETE', `/api/automation/schedules/${encodeURIComponent(id)}`)

export const runAutomationScheduleOnce = (id: string) =>
  request<{ success: boolean; message: string; item?: AutomationSchedule }>('POST', `/api/automation/schedules/${encodeURIComponent(id)}/run`)
