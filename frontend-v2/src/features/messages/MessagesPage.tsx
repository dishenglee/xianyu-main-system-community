// Index child of /messages. Picks the desktop vs mobile renderer; both
// consume the shared `MessagesContext` mounted by `MessagesRouteLayout`, so
// switching between this and MobileChat preserves session/history state.
import { useDevice } from '../../hooks/useDevice'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { AggregateMessages } from './AggregateMessages'
import { MobileMessages } from './MobileMessages'

export default function MessagesPage() {
  const { isMobile, isTablet } = useDevice()
  if (isMobile || isTablet) return <MobileMessages />
  return (
    <div className="flex h-full min-h-0 flex-col">
      <HubTabs tabs={HUB_PRESETS.messages} />
      <div className="min-h-0 flex-1 px-4 py-3 md:px-6">
        <AggregateMessages />
      </div>
    </div>
  )
}
