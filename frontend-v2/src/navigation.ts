export interface NavItem {
  path: string
  label: string
  routeName: string
  section: string
  /** Only render this item when the logged-in user has the admin role. */
  adminOnly?: boolean
  /** Sub-paths that should still mark this hub tab active in the toolbar.
   * E.g. /proxy and /bundle live under the "账号" hub. */
  subPaths?: string[]
}

export interface NavSection {
  title: string
  items: NavItem[]
}

// Hub-style nav: 一级入口 in the toolbar. Related sub-features (代理/导出/
// 系统通知/关键词等) are exposed in-page via <HubTabs> instead of
// fighting for toolbar space. All original routes still resolve, so deep
// links keep working.
export const NAV_SECTIONS: NavSection[] = [
  {
    title: '业务',
    items: [
      { path: '/dashboard', label: '总览', routeName: 'dashboard', section: '业务' },
      {
        path: '/accounts', label: '账号', routeName: 'accounts', section: '业务',
        subPaths: ['/proxy', '/bundle'],
      },
      {
        path: '/items', label: '商品', routeName: 'items', section: '业务',
      },
      { path: '/peer-monitor', label: '同行监控', routeName: 'peer-monitor', section: '业务', subPaths: ['/search'] },
      { path: '/orders', label: '订单', routeName: 'orders', section: '业务' },
      {
        path: '/messages', label: '消息', routeName: 'messages', section: '业务',
        subPaths: ['/notifications'],
      },
      { path: '/growth', label: '推广', routeName: 'promotion', section: '业务' },
      { path: '/super-polish', label: '超级擦亮', routeName: 'super-polish', section: '业务' },
    ],
  },
  {
    title: '工具',
    items: [
      {
        path: '/keywords', label: '自动化', routeName: 'keywords', section: '工具',
        subPaths: ['/item-replies', '/offline-reply', '/ai-replies', '/tasks', '/delivery', '/other-automation'],
      },
    ],
  },
  {
    title: '系统',
    items: [
      { path: '/logs', label: '平台日志', routeName: 'logs', section: '系统', adminOnly: true },
    ],
  },
]

export const MOBILE_TABS: NavItem[] = [
  { path: '/messages', label: '消息', routeName: 'messages', section: '' },
  { path: '/orders', label: '订单', routeName: 'orders', section: '' },
  { path: '/items', label: '商品', routeName: 'items', section: '' },
  { path: '/overview', label: '工作台', routeName: 'workbench', section: '' },
  { path: '/me', label: '我的', routeName: 'me', section: '' },
]

export function flatNavItems(): NavItem[] {
  return NAV_SECTIONS.flatMap((s) => s.items)
}
