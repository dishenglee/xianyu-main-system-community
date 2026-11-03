import type { JsonRecord } from '../../model'
import type { DivisionSelection, PromotionMode } from '../create/helpers'

export interface SuperPolishCampaignDetailItem {
  itemId: string
  catId: string
  title: string
  price: number
  imageUrl: string
  publishTime: number | null
  saleCount: number
  supportAd: boolean
  supportAreaTarget: boolean
  areaTarget: boolean
  tags: string[]
  notSupportReasons: string[]
  raw: JsonRecord
}

export interface SuperPolishCampaignSettingsDraft {
  title: string
  bidTarget: string
  promotionMode: PromotionMode
  bidInput: string
  budgetInput: string
  divisions: DivisionSelection[]
  hours: number[]
  creativeOptEnabled: boolean
  selectedPackageOrderPriceByBidTarget: Record<string, number | null>
  selectedPackageByBidTarget: Record<string, CampaignDetailPackageSelection>
}

export interface SuperPolishCampaignEditSettings {
  title: string
  bidTarget: string
  promotionMode: PromotionMode
  bidCent: number | null
  budgetCent: number | null
  divisions: DivisionSelection[]
  hours: number[]
  creativeOptEnabled: boolean
  bidType: string
  selectedPackageOption: CampaignDetailPackageSelection | null
}

export interface CampaignDetailPackageSelection {
  orderPrice: number | null
  bidPrice: number | null
  bidType: string
  orderStartDuration: number | null
  orderEndDuration: number | null
  promotionText: string
  title: string
  packageType: string
  creativeOptimizationAvailable: boolean
}
