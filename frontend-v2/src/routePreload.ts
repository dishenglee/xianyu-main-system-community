type RoutePreloadTask = () => Promise<unknown>

const desktopRoutePreloadTasks: RoutePreloadTask[] = [
  () => import('./features/accounts/AccountsPage'),
  () => import('./features/items/ItemsPage'),
  () => import('./features/orders/OrdersPage'),
  () => import('./features/messages/MessagesRouteLayout'),
  () => import('./features/messages/MessagesPage'),
  () => import('./features/peer-monitor/PeerMonitorPage'),
  () => import('./features/peer-monitor/PeerMonitorSearch'),
  () => import('./features/promotions/PromotionPage'),
  () => import('./features/super-polish/SuperPolishPage'),
  () => import('./features/keywords/KeywordsPage'),
  () => import('./features/item-replies/ItemRepliesPage'),
  () => import('./features/offline-reply/OfflineReplyPage'),
  () => import('./features/ai-replies/AiRepliesPage'),
  () => import('./features/tasks/CommentTaskHub'),
  () => import('./features/delivery/DeliveryPage'),
  () => import('./features/notifications/NotificationsPage'),
]

let desktopPreloadStarted = false

function scheduleIdle(callback: () => void) {
  if (typeof window === 'undefined') return () => {}
  const win = window as Window & {
    requestIdleCallback?: (cb: IdleRequestCallback, options?: IdleRequestOptions) => number
    cancelIdleCallback?: (handle: number) => void
  }
  if (typeof win.requestIdleCallback === 'function') {
    const id = win.requestIdleCallback(callback, { timeout: 2500 })
    return () => win.cancelIdleCallback?.(id)
  }
  const id = window.setTimeout(callback, 1200)
  return () => window.clearTimeout(id)
}

export function preloadDesktopRoutesWhenIdle() {
  if (desktopPreloadStarted) return () => {}
  desktopPreloadStarted = true
  let cancelled = false
  let cancelCurrent = () => {}
  let index = 0

  const runNext = () => {
    if (cancelled) return
    const task = desktopRoutePreloadTasks[index]
    index += 1
    if (!task) return
    void task()
      .catch(() => {})
      .finally(() => {
        if (!cancelled) cancelCurrent = scheduleIdle(runNext)
      })
  }

  cancelCurrent = scheduleIdle(runNext)
  return () => {
    cancelled = true
    cancelCurrent()
  }
}
