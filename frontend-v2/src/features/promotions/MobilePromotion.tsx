import { TopBar } from '../../components/TopBar'
import { PromotionContent, PromotionRefreshButton } from './PromotionContent'
import { SensitiveVisibilityToggle } from '../../components/SensitiveValue'
import { usePromotionPageState } from './hooks/usePromotionPageState'

export default function MobilePromotion() {
  const state = usePromotionPageState()

  return (
    <div className="h-full flex flex-col mobile-page-shell" style={{ background: 'var(--bg-page)' }}>
      <TopBar
        title="推广"
        subtitle="闲鱼币与鱼小铺曝光"
        right={<><SensitiveVisibilityToggle compact /><PromotionRefreshButton state={state} /></>}
      />
      <div className="mobile-page-body px-3 pt-1 pb-[calc(env(safe-area-inset-bottom)+112px)]">
        <PromotionContent state={state} />
      </div>
    </div>
  )
}
