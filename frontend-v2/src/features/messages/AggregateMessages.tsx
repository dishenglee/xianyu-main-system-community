// @ts-nocheck
// Desktop UI shell for the messages page. All business state + handlers come
// from the shared MessagesContext (provided by MessagesRouteLayout); this
// file is purely the three-column desktop layout that consumes it.
// Identical behaviour to the previous direct `useMessagesPageState()` call —
// the hook now runs one level up so /messages and /messages/chat/... share
// state, but the desktop layout never navigates to MobileChat so its UX is
// unchanged.
import { MessageSquare } from 'lucide-react'

import { EmptyState } from '../../components/EmptyState'
import { useMessagesContext } from './MessagesRouteLayout'
import { AccountsPanel, ConversationPanel, SessionListPanel } from './MessagePanels'

export function AggregateMessages() {
  const {
    // state
    activeAccounts, sessionsByUid,
    selectedAccountUid, selectedSessionId,
    composerText, sending, refreshing,
    itemPriceByItemId, headinfoBySessionId,
    detailScrollRef, imageInputRef,
    replyTarget,
    platformMessagesEnabled,

    // computed
    filteredSessions, pinnedSessions, groupedSessions,
    selectedSession, selectedItem, selectedMessages,
    selectedHistory, selectedLoadingState, totalUnread,

    // setters
    setSelectedAccountUid, setComposerText,
    setReplyTarget,

    // actions
    selectSession, recallMessage, loadAllSessions,
    handleSend, handleImagePick, handleSendLocation, handleSendItem,
  } = useMessagesContext()

  return !activeAccounts.length ? (
    <EmptyState icon={MessageSquare} title="暂无账号" description="请先在账号管理中添加账号" />
  ) : (
    <div className="desktop-messages-grid grid h-full min-h-0 grid-cols-1 gap-3 lg:grid-cols-[200px_minmax(340px,380px)_minmax(0,1fr)]">
      <AccountsPanel
        activeAccounts={activeAccounts}
        selectedAccountUid={selectedAccountUid}
        setSelectedAccountUid={setSelectedAccountUid}
        sessionsByUid={sessionsByUid}
        platformMessagesEnabled={platformMessagesEnabled}
        totalUnread={totalUnread}
      />

      <SessionListPanel
        activeAccounts={activeAccounts}
        selectedAccountUid={selectedAccountUid}
        totalUnread={totalUnread}
        filteredSessions={filteredSessions}
        pinnedSessions={pinnedSessions}
        groupedSessions={groupedSessions}
        selectedSessionId={selectedSessionId}
        selectSession={selectSession}
        refreshing={refreshing}
        onRefresh={() => loadAllSessions({ force: true })}
      />

      <ConversationPanel
        selectedSession={selectedSession}
        selectedItem={selectedItem}
        selectedMessages={selectedMessages}
        selectedHistory={selectedHistory}
        selectedLoadingState={selectedLoadingState}
        headinfoBySessionId={headinfoBySessionId}
        itemPriceByItemId={itemPriceByItemId}
        detailScrollRef={detailScrollRef}
        imageInputRef={imageInputRef}
        composerText={composerText}
        setComposerText={setComposerText}
        sending={sending}
        onImagePick={handleImagePick}
        onSend={handleSend}
        onSendLocation={handleSendLocation}
        onSendItem={handleSendItem}
        onRecallMessage={recallMessage}
        replyTarget={replyTarget}
        setReplyTarget={setReplyTarget}
      />
    </div>
  )
}
