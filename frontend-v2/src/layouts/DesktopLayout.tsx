import { useEffect, useMemo, useState } from 'react'
import { NavLink, Outlet, useLocation, useNavigate } from 'react-router-dom'
import {
  Globe, LayoutDashboard, Database, ShoppingCart, Activity,
  Mail, MessageSquare, Sparkles, Package2, CreditCard, Send,
  Search as SearchIcon, HeartPulse, ShieldAlert, QrCode,
  ChevronLeft, Bell, Megaphone, Loader2,
  Users,
} from 'lucide-react'
import type { LucideProps } from 'lucide-react'
import { NAV_SECTIONS } from '../navigation'
import { useAccounts } from '../hooks/useAccounts'
import CommandPalette from '../components/CommandPalette'
import { TaskProgressFloat } from '../components/TaskProgressFloat'
import { GlobalMessageNotify } from '../components/GlobalMessageNotify'
import { useUserUnreadStore, selectUserUnreadTotal } from '../store/userUnreadStore'
import { preloadDesktopRoutesWhenIdle } from '../routePreload'
import {
  checkAppUpdate,
  getAppVersionInfo,
  installAppUpdate,
  requestAppRestart,
  type AppUpdateCheckResult,
} from '../features/update/api'
import { friendlyLicenseError, sanitizeServiceText } from '../utils/errorText'
import { isDeleteOnlyAccount } from '../utils/accountAccess'
import mascotUrl from '../assets/branding/yuguanjia-mascot.png'
import wordmarkUrl from '../assets/branding/yuguanjia-wordmark.png'

// -----------------------------------------------------------------------------
// Desktop layout — macOS-style toolbar. Replaces the old 256px NavLink sidebar
// with a single translucent top bar + segmented primary tabs + a "More" popover
// for the long tail of ops / system pages. Routes are unchanged; accounts /
// dashboard / orders / items / messages stay the primary five tabs, everything
// else (tasks / keywords / cards / delivery / proxy / bundle / notifications /
// logs / search) lives behind the "更多" button. The `adminOnly` flag on
// `/logs` is still honoured.
// -----------------------------------------------------------------------------

const ICON_MAP: Record<string, React.ComponentType<LucideProps>> = {
  dashboard: LayoutDashboard,
  accounts: Database,
  items: ShoppingCart,
  orders: Activity,
  messages: Mail,
  tasks: MessageSquare,
  keywords: Sparkles,
  'item-replies': Package2,
  cards: CreditCard,
  delivery: Send,
  search: SearchIcon,
  'peer-monitor': Users,
  proxy: Globe,
  bundle: QrCode,
  logs: HeartPulse,
  notifications: Bell,
  promotion: Megaphone,
  'super-polish': Sparkles,
}

export function DesktopLayout() {
  const navigate = useNavigate()
  const location = useLocation()
  const isAdmin = true
  const userUnread = useUserUnreadStore(selectUserUnreadTotal)
  const { accounts, loadAccounts } = useAccounts()
  const [appVersion, setAppVersion] = useState('—')
  const [updateResult, setUpdateResult] = useState<AppUpdateCheckResult | null>(null)
  const [updateChecking, setUpdateChecking] = useState(false)
  const [updateInstalling, setUpdateInstalling] = useState(false)
  const [updatePopoverOpen, setUpdatePopoverOpen] = useState(false)
  const [updateMessage, setUpdateMessage] = useState<{ kind: 'ok' | 'err'; text: string } | null>(null)

  const allTabs = useMemo(() => {
    const items: { path: string; label: string; routeName: string; subPaths?: string[] }[] = []
    for (const sec of NAV_SECTIONS) {
      for (const it of sec.items) {
        if (it.adminOnly && !isAdmin) continue
        items.push({ path: it.path, label: it.label, routeName: it.routeName, subPaths: it.subPaths })
      }
    }
    return items
  }, [isAdmin])

  const canGoBack = location.pathname !== '/' && location.pathname !== '/dashboard'

  useEffect(() => preloadDesktopRoutesWhenIdle(), [])

  useEffect(() => {
    void loadAccounts()
  }, [loadAccounts])

  useEffect(() => {
    let cancelled = false
    async function loadVersionAndUpdateState() {
      try {
        const info = await getAppVersionInfo()
        if (cancelled) return
        setAppVersion(info?.current_version || '—')
      } catch {
        if (!cancelled) setAppVersion('—')
      }
      try {
        if (!cancelled) setUpdateChecking(true)
        const result = await checkAppUpdate()
        if (cancelled) return
        setUpdateResult(result)
        setAppVersion(result.current_version || '—')
      } catch (err) {
        if (!cancelled) setUpdateMessage({ kind: 'err', text: friendlyLicenseError(err, '检查更新失败') })
      } finally {
        if (!cancelled) setUpdateChecking(false)
      }
    }
    void loadVersionAndUpdateState()
    return () => {
      cancelled = true
    }
  }, [])

  const usedAccountCount = accounts.filter((account) => (
    String(account?.status || '').trim() !== 'disabled' && !isDeleteOnlyAccount(account)
  )).length
  const hasUpdate = Boolean(updateResult?.available && updateResult.version)

  const handleInstallUpdate = async () => {
    setUpdateMessage(null)
    setUpdateInstalling(true)
    try {
      const result = updateResult?.available ? updateResult : await checkAppUpdate()
      setUpdateResult(result)
      setAppVersion(result.current_version || appVersion)
      if (!result.configured && !result.policy_configured) {
        setUpdateMessage({ kind: 'err', text: '当前版本尚未配置更新服务' })
        return
      }
      if (result.blocked && !result.available) {
        setUpdateMessage({
          kind: 'err',
          text: sanitizeServiceText(result.block_reason || '当前版本已停用，请先更新客户端', '当前版本已停用，请先更新客户端'),
        })
        return
      }
      if (!result.available || !result.version) {
        setUpdateMessage({ kind: 'ok', text: `当前已是最新版本 v${result.current_version}` })
        return
      }
      setUpdateMessage({ kind: 'ok', text: `正在更新到 v${result.version}…` })
      const installResult = await installAppUpdate({ targetVersion: result.version })
      setUpdateMessage({
        kind: 'ok',
        text: installResult?.message || '安装包已打开，请按系统提示完成安装。',
      })
      if (installResult?.restart_required) {
        await requestAppRestart()
      }
    } catch (err) {
      setUpdateMessage({ kind: 'err', text: friendlyLicenseError(err, '更新失败') })
    } finally {
      setUpdateInstalling(false)
    }
  }

  return (
    <div className="desktop-shell flex flex-col">
      <header className="desktop-toolbar-wrap">
      <div className="desktop-toolbar">
        {/* left: logo + back + breadcrumb */}
        <div className="desktop-toolbar-brand">
          <img
            src={mascotUrl}
            alt="鱼管家客服吉祥物"
            className="desktop-toolbar-app-icon"
          />
          <div className="desktop-toolbar-brand-copy">
            <img
              src={wordmarkUrl}
              alt="鱼管家品牌字标"
              className="desktop-toolbar-wordmark"
            />
            <div className="desktop-toolbar-subtitle text-[11px]">桌面工作台</div>
          </div>
          {canGoBack ? (
            <button
              type="button"
              onClick={() => navigate(-1)}
              className="desktop-toolbar-action ml-1 px-2.5 cursor-pointer"
              aria-label="返回上一页"
              title="返回"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
          ) : null}
        </div>

        <div className="desktop-toolbar-nav">
          <nav
            className="desktop-toolbar-segment max-w-full overflow-x-auto"
            style={{ scrollbarWidth: 'none' }}
          >
            {allTabs.map((t) => {
              const Icon = ICON_MAP[t.routeName] ?? ShieldAlert
              const isActive = t.path === '/'
                ? location.pathname === '/'
                : location.pathname === t.path
                  || location.pathname.startsWith(t.path + '/')
                  || (t.subPaths || []).some((p) => location.pathname === p || location.pathname.startsWith(p + '/'))
              const showBadge = t.routeName === 'messages' && userUnread > 0
              return (
                <NavLink
                  key={t.path}
                  to={t.path}
                  end={t.path === '/'}
                  className={[
                    'desktop-toolbar-tab shrink-0 px-3.5 py-2 rounded-[14px] text-[12px] font-medium flex items-center gap-1.5 cursor-pointer transition-all',
                    isActive ? 'desktop-toolbar-tab-active' : '',
                  ].filter(Boolean).join(' ')}
                >
                  <Icon className="w-4 h-4" />
                  {t.label}
                  {showBadge && (
                    <span
                      className="ml-0.5 inline-flex items-center justify-center rounded-full text-[10px] font-semibold leading-none text-white"
                      style={{
                        backgroundColor: 'var(--danger)',
                        minWidth: 16,
                        height: 16,
                        padding: '0 5px',
                      }}
                    >
                      {userUnread > 99 ? '99+' : userUnread}
                    </span>
                  )}
                </NavLink>
              )
            })}
          </nav>
        </div>

        <div className="desktop-toolbar-spacer" aria-hidden="true" />

      </div>

      </header>

      <main className="desktop-shell-main flex flex-col flex-1 min-h-0 overflow-hidden">
        <Outlet />
      </main>
      <footer className="desktop-status-bar-wrap">
        <div className="desktop-status-bar">
          <div className="desktop-status-left">
            <div
              className={[
                'desktop-version-entry',
                updatePopoverOpen ? 'desktop-version-entry-open' : '',
              ].filter(Boolean).join(' ')}
              onMouseEnter={() => setUpdatePopoverOpen(true)}
              onMouseLeave={() => setUpdatePopoverOpen(false)}
            >
              <button
                type="button"
                className="desktop-status-item desktop-status-version desktop-version-button cursor-pointer"
                onClick={() => setUpdatePopoverOpen((open) => !open)}
                aria-label={hasUpdate ? `发现新版本 ${updateResult?.version}` : `当前版本 V${appVersion}`}
              >
                版本 V{appVersion}
                {hasUpdate ? <span className="desktop-version-dot" /> : null}
              </button>
              <VersionUpdatePopover
                currentVersion={appVersion}
                checking={updateChecking}
                installing={updateInstalling}
                result={updateResult}
                message={updateMessage}
                onInstall={() => void handleInstallUpdate()}
              />
            </div>
          </div>
          <div className="desktop-status-right">
            <span className="desktop-status-item desktop-status-username">本机用户</span>
            <span className="desktop-status-item">{usedAccountCount} 个闲鱼账号</span>
          </div>
        </div>
      </footer>
      <CommandPalette />
      <TaskProgressFloat />
      <GlobalMessageNotify startupSessionRefresh="cached-first" />
    </div>
  )
}

function VersionUpdatePopover({
  currentVersion,
  checking,
  installing,
  result,
  message,
  onInstall,
}: {
  currentVersion: string
  checking: boolean
  installing: boolean
  result: AppUpdateCheckResult | null
  message: { kind: 'ok' | 'err'; text: string } | null
  onInstall: () => void
}) {
  const hasUpdate = Boolean(result?.available && result.version)
  const notes = result?.body
    ?.split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, 5) ?? []

  return (
    <div className="desktop-version-popover" role="dialog" aria-label="版本更新">
      <div className="desktop-version-popover-head">
        <div>
          <div className="desktop-version-popover-title">
            {hasUpdate ? `发现新版本 v${result?.version}` : `当前版本 v${currentVersion}`}
          </div>
          <div className="desktop-version-popover-sub">
            {checking ? '正在检查更新…' : hasUpdate ? `当前版本 v${result?.current_version || currentVersion}` : '当前已是最新版本'}
          </div>
        </div>
        {checking ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
      </div>

      {notes.length > 0 ? (
        <div className="desktop-version-notes">
          <div className="desktop-version-notes-title">更新日志</div>
          {notes.map((line) => (
            <div key={line} className="desktop-version-note-line">{line}</div>
          ))}
        </div>
      ) : hasUpdate ? (
        <div className="desktop-version-empty-note">暂无更新日志</div>
      ) : null}

      {message ? (
        <div className={`desktop-version-message desktop-version-message-${message.kind}`}>
          {message.text}
        </div>
      ) : null}

      {hasUpdate ? (
        <button
          type="button"
          className="desktop-version-update-button cursor-pointer"
          onClick={onInstall}
          disabled={installing}
        >
          {installing ? (
            <>
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
              更新中…
            </>
          ) : (
            '立即更新'
          )}
        </button>
      ) : null}
    </div>
  )
}
