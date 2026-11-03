import { NavLink, useLocation } from 'react-router-dom'
import { useMemo } from 'react'
import { useDevice } from '../hooks/useDevice'
import { useAuthStore } from '../store/authStore'
import { isAdminUser } from '../utils/authRole'

export interface HubTab {
  to: string
  label: string
  /** Optional alternate paths that should still mark this tab active. */
  match?: string[]
}

interface Props {
  tabs?: HubTab[]
  /** Optional right-side slot for page-level actions (原 TopBar 的 right 按钮). */
  right?: React.ReactNode
  /** Optional title to replace the leftmost slot (when page has no sub-tabs). */
  title?: React.ReactNode
}

/**
 * Sticky in-page sub-toolbar — used by hub pages (账号 / 商品 / 消息 / 自动化)
 * to expose related sub-features (代理/导出，系统通知，关键词/商品回复/
 * 评论任务/卡密/发货规则) without changing the page itself. Each tab is a real
 * route link, so deep links keep working. 页面自己的功能按钮通过 `right`
 * prop 注入到这一行的右侧 —— 替代原每页各自的 <TopBar>。
 */
export function HubTabs({ tabs, right, title }: Props) {
  const location = useLocation()
  const { isMobile } = useDevice()
  const user = useAuthStore((state) => state.user)
  const visibleTabs = useMemo(
    () => (isAdminUser(user) ? tabs : tabs?.filter((tab) => tab.to !== '/proxy')),
    [tabs, user],
  )

  function isActive(t: HubTab) {
    if (location.pathname === t.to) return true
    if (t.match?.includes(location.pathname)) return true
    return false
  }

  return (
    <div
      className={isMobile ? 'hub-tabs sticky z-30 px-3' : 'hub-tabs sticky z-30 px-4 md:px-6'}
      style={{
        top: 0,
        height: isMobile ? 54 : 44,
      }}
    >
      <div
        className={isMobile ? 'hub-tabs-inner h-full flex items-center gap-2 overflow-x-auto mobile-toolbar-scroll' : 'hub-tabs-inner h-full flex items-center gap-1 overflow-x-auto'}
        style={{ scrollbarWidth: 'none' }}
      >
        {title ? (
          <span
            className={isMobile ? 'hub-tabs-title shrink-0 text-[13px] font-semibold tracking-[0.08em] uppercase' : 'hub-tabs-title shrink-0 text-[14px] font-semibold'}
          >
            {title}
          </span>
        ) : null}

        {visibleTabs && visibleTabs.length > 0 ? visibleTabs.map((t) => {
          const active = isActive(t)
          return (
            <NavLink
              key={t.to}
              to={t.to}
              end
              className={[
                'hub-tab shrink-0 cursor-pointer transition-colors',
                isMobile
                  ? 'rounded-full px-3.5 py-2 text-[12px] font-semibold'
                  : 'px-3.5 py-1.5 rounded-[8px] text-[13.5px] font-medium',
                active ? 'hub-tab-active' : '',
              ].filter(Boolean).join(' ')}
            >
              {t.label}
            </NavLink>
          )
        }) : null}

        {right ? (
          <div className={isMobile ? 'ml-auto shrink-0 flex items-center gap-1.5' : 'ml-auto shrink-0 flex items-center gap-2'}>
            {right}
          </div>
        ) : null}
      </div>
    </div>
  )
}

// Shared tab presets so all pages in a hub stay in sync.
export const HUB_PRESETS: Record<string, HubTab[]> = {
  accounts: [
    { to: '/accounts',        label: '账号列表' },
    { to: '/proxy',           label: '网络代理' },
    { to: '/bundle',          label: '账号导出/导入' },
  ],
  items: [
    { to: '/items',           label: '我的商品' },
  ],
  messages: [
    { to: '/messages',        label: '对话' },
    { to: '/notifications',   label: '消息通知' },
  ],
  automation: [
    { to: '/keywords',        label: '关键词回复' },
    { to: '/item-replies',    label: '默认/商品回复' },
    { to: '/offline-reply',   label: '不在线回复' },
    { to: '/ai-replies',      label: 'AI回复' },
    { to: '/tasks',           label: '评论任务' },
    { to: '/delivery',        label: '自动发货' },
    { to: '/other-automation', label: '定时任务' },
  ],
}
