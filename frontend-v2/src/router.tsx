import { lazy, Suspense, type ReactNode } from 'react'
import { Navigate, createBrowserRouter, useLocation } from 'react-router-dom'
import { AppShell } from './layouts/AppShell'
import { MobileOnlyRoute } from './components/MobileOnlyRoute'

const Dashboard = lazy(() => import('./features/dashboard/Dashboard'))
const DefaultLanding = lazy(() => import('./features/dashboard/DefaultLanding'))
const MessagesRouteLayout = lazy(() => import('./features/messages/MessagesRouteLayout'))
const MessagesPage = lazy(() => import('./features/messages/MessagesPage'))
const MobileChat = lazy(() => import('./features/messages/MobileChat'))
const OrdersPage = lazy(() => import('./features/orders/OrdersPage'))
const AccountsPage = lazy(() => import('./features/accounts/AccountsPage'))
const AccountDetailPage = lazy(() => import('./features/accounts/AccountDetailPage'))
const NotificationsPage = lazy(() => import('./features/notifications/NotificationsPage'))
const ItemsPage = lazy(() => import('./features/items/ItemsPage'))
const KeywordsPage = lazy(() => import('./features/keywords/KeywordsPage'))
const ItemRepliesPage = lazy(() => import('./features/item-replies/ItemRepliesPage'))
const OfflineReplyPage = lazy(() => import('./features/offline-reply/OfflineReplyPage'))
const AiRepliesPage = lazy(() => import('./features/ai-replies/AiRepliesPage'))
const DeliveryPage = lazy(() => import('./features/delivery/DeliveryPage'))
const OtherAutomationPage = lazy(() => import('./features/other-automation/OtherAutomationPage'))
const CommentTaskHub = lazy(() => import('./features/tasks/CommentTaskHub'))
const MobileOverview = lazy(() => import('./features/overview/MobileOverview'))
const LocalProfilePage = lazy(() => import('./features/me/LocalProfilePage'))
const ProxyPage = lazy(() => import('./features/proxy/ProxyPage'))
const LogsPage = lazy(() => import('./features/logs/LogsPage'))
const PeerMonitorPage = lazy(() => import('./features/peer-monitor/PeerMonitorPage'))
const PeerMonitorSearch = lazy(() => import('./features/peer-monitor/PeerMonitorSearch'))
const PeerMonitorCandidatePage = lazy(() => import('./features/peer-monitor/PeerMonitorCandidatePage'))
const PeerMonitorTargetDetailPage = lazy(() => import('./features/peer-monitor/PeerMonitorTargetDetailPage'))
const BundleTransferPage = lazy(() => import('./features/bundle/BundleTransferPage'))
const PromotionPage = lazy(() => import('./features/promotions/PromotionPage'))
const SuperPolishPage = lazy(() => import('./features/super-polish/SuperPolishPage'))
const SuperPolishCampaignListPage = lazy(() => import('./features/super-polish/campaigns/SuperPolishCampaignListPage'))
const SuperPolishCampaignCreatePage = lazy(() => import('./features/super-polish/campaigns/SuperPolishCampaignCreatePage'))
const SuperPolishCampaignDetailPage = lazy(() => import('./features/super-polish/campaigns/SuperPolishCampaignDetailPage'))
const SuperPolishReportsPage = lazy(() => import('./features/super-polish/reports/SuperPolishReportsPage'))
function Loader() {
  return (
    <div className="flex-1 flex items-center justify-center min-h-screen">
      <div
        className="w-6 h-6 rounded-full border-2 border-t-transparent animate-spin"
        style={{ borderColor: 'var(--primary)', borderTopColor: 'transparent' }}
      />
    </div>
  )
}

function wrap(element: ReactNode) {
  return <Suspense fallback={<Loader />}>{element}</Suspense>
}

function LegacySearchRedirect() {
  const location = useLocation()
  return <Navigate to={{ pathname: '/peer-monitor/search', search: location.search }} replace />
}

export const router = createBrowserRouter([
  {
    path: '/',
    element: <AppShell />,
    children: [
      { index: true, element: wrap(<DefaultLanding />) },
      { path: 'dashboard', element: wrap(<Dashboard />) },
      {
        path: 'messages',
        element: wrap(<MessagesRouteLayout />),
        // Both children share the layout's MessagesContext so MobileChat and
        // MobileMessages drive the same hook instance. Without this, sending
        // a message in MobileChat wouldn't update the list summary, and the
        // list's unread badge survived MobileChat's mark-as-read.
        children: [
          { index: true, element: wrap(<MessagesPage />) },
          {
            path: 'chat/:uid/:sessionId/:sessionType',
            element: wrap(
              <MobileOnlyRoute fallback="/messages">
                <MobileChat />
              </MobileOnlyRoute>,
            ),
          },
        ],
      },
      { path: 'orders', element: wrap(<OrdersPage />) },
      // Stub pages — will be replaced in S04/S05
      { path: 'accounts', element: wrap(<AccountsPage />) },
      { path: 'accounts/:uid', element: wrap(<AccountDetailPage />) },
      { path: 'notifications', element: wrap(<NotificationsPage />) },
      { path: 'items', element: wrap(<ItemsPage />) },
      { path: 'tasks', element: wrap(<CommentTaskHub />) },
      { path: 'keywords', element: wrap(<KeywordsPage />) },
      { path: 'item-replies', element: wrap(<ItemRepliesPage />) },
      { path: 'offline-reply', element: wrap(<OfflineReplyPage />) },
      { path: 'ai-replies', element: wrap(<AiRepliesPage />) },
      { path: 'cards', element: <Navigate to="/delivery" replace /> },
      { path: 'delivery', element: wrap(<DeliveryPage />) },
      { path: 'other-automation', element: wrap(<OtherAutomationPage />) },
      {
        path: 'overview',
        element: wrap(
          <MobileOnlyRoute fallback="/">
            <MobileOverview />
          </MobileOnlyRoute>,
        ),
      },
      {
        path: 'me',
        element: wrap(
          <MobileOnlyRoute fallback="/">
            <LocalProfilePage />
          </MobileOnlyRoute>,
        ),
      },
      { path: 'peer-monitor', element: wrap(<PeerMonitorPage />) },
      { path: 'peer-monitor/search', element: wrap(<PeerMonitorSearch />) },
      { path: 'peer-monitor/import', element: wrap(<PeerMonitorCandidatePage />) },
      { path: 'peer-monitor/items/:itemId', element: wrap(<PeerMonitorCandidatePage />) },
      { path: 'peer-monitor/items/:itemId/home', element: wrap(<PeerMonitorCandidatePage sellerHome />) },
      { path: 'peer-monitor/targets/:targetId', element: wrap(<PeerMonitorTargetDetailPage />) },
      { path: 'search', element: <LegacySearchRedirect /> },
      { path: 'proxy', element: wrap(<ProxyPage />) },
      { path: 'logs', element: wrap(<LogsPage />) },
      { path: 'bundle', element: wrap(<BundleTransferPage />) },
      { path: 'growth', element: wrap(<PromotionPage />) },
      {
        path: 'super-polish',
        element: wrap(<SuperPolishPage />),
        children: [
          { index: true, element: <Navigate to="campaigns" replace /> },
          { path: 'campaigns', element: wrap(<SuperPolishCampaignListPage />) },
          { path: 'campaigns/new', element: wrap(<SuperPolishCampaignCreatePage />) },
          { path: 'campaigns/:campaignId', element: wrap(<SuperPolishCampaignDetailPage />) },
          { path: 'account', element: <Navigate to="../campaigns" replace /> },
          { path: 'reports', element: wrap(<SuperPolishReportsPage />) },
        ],
      },
      // Legacy comment-* paths now redirect into the unified hub.
      { path: 'comments', element: <Navigate to="/tasks?tab=targets" replace /> },
      { path: 'comment-targets', element: <Navigate to="/tasks?tab=targets" replace /> },
      { path: 'comment-candidates', element: <Navigate to="/tasks?tab=candidates" replace /> },
      { path: 'comment-library', element: <Navigate to="/tasks?tab=library" replace /> },
      { path: 'comment-delete-queue', element: <Navigate to="/tasks?tab=delete-queue" replace /> },
    ],
  },
  { path: '*', element: <Navigate to="/" replace /> },
])
