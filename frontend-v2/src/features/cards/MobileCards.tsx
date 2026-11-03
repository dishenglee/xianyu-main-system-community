import { TopBar } from '../../components/TopBar'
import CardPoolManager from './CardPoolManager'

export default function MobileCards({ embedded = false }: { embedded?: boolean }) {
  return <div className={embedded ? '' : 'h-full flex flex-col mobile-page-shell'}>{!embedded && <TopBar title="卡密管理" />}<div className={embedded ? '' : 'flex-1 overflow-y-auto mobile-page-body'}><CardPoolManager mobile embedded={embedded} /></div></div>
}
