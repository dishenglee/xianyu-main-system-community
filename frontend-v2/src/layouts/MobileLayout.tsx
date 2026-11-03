import { NavLink, Outlet, useLocation } from 'react-router-dom'
import {
  MessageSquare, ListChecks, Search, LayoutDashboard, User,
  Mail, Receipt, Package, Database,
} from 'lucide-react'
import type { LucideProps } from 'lucide-react'
import { MOBILE_TABS } from '../navigation'
import { TaskProgressFloat } from '../components/TaskProgressFloat'
import { GlobalMessageNotify } from '../components/GlobalMessageNotify'
import { useUserUnreadStore, selectUserUnreadTotal } from '../store/userUnreadStore'

const TAB_ICON_MAP: Record<string, React.ComponentType<LucideProps>> = {
  messages: MessageSquare,
  orders: Receipt,
  items: Package,
  dashboard: LayoutDashboard,
  workbench: LayoutDashboard,
  me: User,
  // Kept so legacy configs still resolve to a sane icon.
  tasks: ListChecks,
  search: Search,
  accounts: Database,
  mail: Mail,
}

export function MobileLayout() {
  const userUnread = useUserUnreadStore(selectUserUnreadTotal)
  const location = useLocation()
  const fixedViewport = location.pathname.startsWith('/messages/chat/')
  const hideBottomNav = fixedViewport

  return (
    <div className="mobile-layout-root flex flex-col" style={{ backgroundColor: 'var(--bg-page)' }}>
      <main className={`mobile-shell-main flex-1 min-h-0 overflow-hidden ${fixedViewport ? 'mobile-shell-main-fixed' : 'mobile-shell-main-tabbed'}`}>
        <Outlet />
      </main>

      {!hideBottomNav ? (
        <nav
          className="fixed bottom-0 left-0 right-0 z-50 flex border-t mobile-bottom-nav"
          style={{
            paddingBottom: 'env(safe-area-inset-bottom)',
          }}
        >
          {MOBILE_TABS.map((tab) => {
            const Icon = TAB_ICON_MAP[tab.routeName] ?? LayoutDashboard
            const showBadge = tab.routeName === 'messages' && userUnread > 0
            return (
              <NavLink
                key={tab.path}
                to={tab.path}
                end={tab.path === '/'}
                aria-label={showBadge ? `${tab.label} ${userUnread} 条未读` : tab.label}
                className="mobile-tab-link flex-1 flex flex-col items-center justify-center py-2 gap-0.5 cursor-pointer transition-colors"
              >
                {({ isActive }) => (
                  <div className={isActive ? 'mobile-tab-link-active flex w-full flex-col items-center justify-center gap-1 rounded-[20px] px-1 py-2.5' : 'flex w-full flex-col items-center justify-center gap-1 rounded-[20px] px-1 py-2.5'}>
                    <span className="relative inline-flex">
                      <Icon
                        className="w-[22px] h-[22px]"
                        style={{ color: isActive ? '#f4c400' : '#7f8898' }}
                      />
                      {showBadge && (
                        <span
                          className="absolute -top-1.5 -right-2 inline-flex items-center justify-center rounded-full text-[10px] font-semibold leading-none text-white"
                          style={{
                            backgroundColor: 'var(--danger)',
                            minWidth: 16,
                            height: 16,
                            padding: '0 4px',
                          }}
                        >
                          {userUnread > 99 ? '99+' : userUnread}
                        </span>
                      )}
                    </span>
                    <span
                      className="text-[11px] font-medium"
                      style={{ color: isActive ? 'var(--text-main)' : '#7f8898' }}
                    >
                      {tab.label}
                    </span>
                  </div>
                )}
              </NavLink>
            )
          })}
        </nav>
      ) : null}
      <TaskProgressFloat />
      <GlobalMessageNotify startupSessionRefresh="deferred" />
    </div>
  )
}
