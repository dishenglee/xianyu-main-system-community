import { TopBar } from '../../components/TopBar'
import { SuperPolishRouteLayout } from './SuperPolishRouteLayout'

export default function MobileSuperPolish() {
  return (
    <div className="h-full flex flex-col mobile-page-shell" style={{ background: 'var(--bg-page)' }}>
      <TopBar title="超级擦亮" subtitle="已建计划、新建计划与计划报表" />
      <SuperPolishRouteLayout mobile />
    </div>
  )
}
