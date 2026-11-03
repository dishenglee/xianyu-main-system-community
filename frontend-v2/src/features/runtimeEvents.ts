export const XIANYU_ITEMS_SYNCED_EVENT = 'xianyu:items-synced'
export const XIANYU_ORDERS_SYNCED_EVENT = 'xianyu:orders-synced'
export const XIANYU_ACCOUNTS_SYNCED_EVENT = 'xianyu:accounts-synced'
export const XIANYU_MESSAGES_SYNCED_EVENT = 'xianyu:messages-synced'

type SyncDetail = {
  uid?: string
  uids?: string[]
  source?: string
  total?: number
}

function detailToUids(detail?: SyncDetail) {
  if (!detail) return []
  if (Array.isArray(detail.uids)) return detail.uids.map((v) => String(v)).filter(Boolean)
  if (detail.uid) return [String(detail.uid)]
  return []
}

export function emitItemsSynced(detail: SyncDetail = {}) {
  window.dispatchEvent(new CustomEvent(XIANYU_ITEMS_SYNCED_EVENT, { detail }))
}

export function emitOrdersSynced(detail: SyncDetail = {}) {
  window.dispatchEvent(new CustomEvent(XIANYU_ORDERS_SYNCED_EVENT, { detail }))
}

export function emitAccountsSynced(detail: SyncDetail = {}) {
  window.dispatchEvent(new CustomEvent(XIANYU_ACCOUNTS_SYNCED_EVENT, { detail }))
}

export function emitMessagesSynced(detail: SyncDetail = {}) {
  window.dispatchEvent(new CustomEvent(XIANYU_MESSAGES_SYNCED_EVENT, { detail }))
}

export function listenSyncEvent(
  eventName: string,
  onSync: (uids: string[], detail?: SyncDetail) => void,
) {
  const handler = (event: Event) => {
    const custom = event as CustomEvent<SyncDetail>
    const detail = custom.detail
    onSync(detailToUids(detail), detail)
  }
  window.addEventListener(eventName, handler)
  return () => window.removeEventListener(eventName, handler)
}

export function listenBatchedSyncEvent(
  eventName: string,
  waitMs: number,
  onSync: (uids: string[], details: SyncDetail[]) => void | Promise<void>,
) {
  let timer: ReturnType<typeof setTimeout> | null = null
  let queuedUids = new Set<string>()
  let queuedDetails: SyncDetail[] = []

  const flush = () => {
    const uids = [...queuedUids]
    const details = queuedDetails
    queuedUids = new Set<string>()
    queuedDetails = []
    timer = null
    void onSync(uids, details)
  }

  const unsubscribe = listenSyncEvent(eventName, (uids, detail) => {
    for (const uid of uids) queuedUids.add(uid)
    if (detail) queuedDetails.push(detail)
    if (timer) clearTimeout(timer)
    timer = setTimeout(flush, waitMs)
  })

  return () => {
    unsubscribe()
    if (timer) clearTimeout(timer)
  }
}
