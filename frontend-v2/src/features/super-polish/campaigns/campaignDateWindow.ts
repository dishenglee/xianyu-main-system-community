import { daysAgoInput, todayInput } from '../model'

interface CampaignListWindow {
  startDate: string
  endDate: string
}

interface NormalizeWindowOptions extends CampaignListWindow {
  today?: string
}

const DEFAULT_LIST_WINDOW_DAYS = 365
const LEGACY_LIST_WINDOW_DAYS = 6

export function builtCampaignDefaultWindow(): CampaignListWindow {
  return {
    startDate: daysAgoInput(DEFAULT_LIST_WINDOW_DAYS),
    endDate: todayInput(),
  }
}

export function normalizeLegacyCampaignListWindow({
  startDate,
  endDate,
  today = todayInput(),
}: NormalizeWindowOptions): CampaignListWindow {
  const legacyStartDate = shiftDateInput(today, -LEGACY_LIST_WINDOW_DAYS)
  if (startDate !== legacyStartDate || endDate !== today) {
    return { startDate, endDate }
  }

  return {
    startDate: shiftDateInput(today, -DEFAULT_LIST_WINDOW_DAYS),
    endDate: today,
  }
}

function shiftDateInput(value: string, deltaDays: number) {
  const [year, month, day] = value.split('-').map(Number)
  const next = new Date(Date.UTC(year, month - 1, day + deltaDays))
  return next.toISOString().slice(0, 10)
}
