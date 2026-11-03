// @ts-nocheck
// Route layout shared by both `/messages` (index list) and
// `/messages/chat/:uid/:sessionId/:sessionType` (mobile detail). Calls
// `useMessagesPageState` exactly once here and exposes its full return value
// through React Context, so the child routes see a single source of truth.
//
// Why: previously each child route mounted its own copy of the state. When
// the user opened MobileChat the list page unmounted and lost its state;
// when they navigated back, the list re-mounted from scratch and raced the
// backend cache. Side-effects of the previous split:
//   * sending a message in MobileChat didn't update the list's summary/ts
//   * the list's unread badge stayed lit even after MobileChat cleared the
//     server-side unread (because the in-memory session.unread reset was
//     thrown away on unmount).
// Lifting the hook into the parent route fixes both — MobileChat now drives
// the same `setSelectedSessionId` / `sendActions` / `historyByKey` the list
// reads. Desktop `AggregateMessages` sits under the same provider and is
// behaviourally unchanged (only its hook call site moved one level up).
import { createContext, useContext, type ReactNode } from 'react'
import { Outlet } from 'react-router-dom'

import { useMessagesPageState } from './hooks/useMessagesPageState'

type MessagesContextValue = ReturnType<typeof useMessagesPageState>

const MessagesContext = createContext<MessagesContextValue | null>(null)

export function MessagesProvider({
  value,
  children,
}: {
  value: MessagesContextValue
  children: ReactNode
}) {
  return (
    <MessagesContext.Provider value={value}>{children}</MessagesContext.Provider>
  )
}

// Strict accessor — throws when consumed outside `MessagesRouteLayout`. We
// intentionally don't return a sentinel/undefined so a missing provider blows
// up loudly during dev instead of silently disabling features.
export function useMessagesContext(): MessagesContextValue {
  const ctx = useContext(MessagesContext)
  if (ctx === null) {
    throw new Error(
      'useMessagesContext must be used inside <MessagesRouteLayout>. ' +
        'Check router.tsx — both /messages and /messages/chat/... should be ' +
        'children of the layout.',
    )
  }
  return ctx
}

export default function MessagesRouteLayout() {
  const state = useMessagesPageState()
  return (
    <MessagesProvider value={state}>
      <Outlet />
    </MessagesProvider>
  )
}
