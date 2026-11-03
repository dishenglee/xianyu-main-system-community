import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import CardPoolManager from './CardPoolManager'

export default function DesktopCards({ embedded = false }: { embedded?: boolean }) {
  return <>{!embedded && <HubTabs tabs={HUB_PRESETS.automation} />}<CardPoolManager embedded={embedded} /></>
}
