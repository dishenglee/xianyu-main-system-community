import { readPath } from '../model'

interface HomeCampaignSummary {
  total: number
  active: number
  paused: number
}

function readCount(source: unknown, paths: string[][]): number | null {
  for (const path of paths) {
    const numeric = Number(readPath(source, path))
    if (Number.isFinite(numeric)) {
      return numeric
    }
  }
  return null
}

export function summarizeHomeCampaignInfo(campaignInfo: unknown): HomeCampaignSummary {
  const explicitPaused = readCount(campaignInfo, [['pausedCampaignNum'], ['pausedCampaignCount']])
  const systemPaused = readCount(campaignInfo, [['systemPauseCampaignCount']]) ?? 0
  const userPaused = readCount(campaignInfo, [['userPauseCampaignCount']]) ?? 0
  const paused = explicitPaused ?? (systemPaused + userPaused)

  const explicitActive = readCount(campaignInfo, [['activeCampaignNum'], ['activeCampaignCount']])
  const explicitTotal = readCount(campaignInfo, [
    ['allCampaignNum'],
    ['totalCampaignNum'],
    ['campaignCount'],
    ['allCampaignCount'],
  ])

  let active = explicitActive ?? 0
  if (explicitActive === null) {
    if (explicitTotal !== null && explicitTotal < paused) {
      active = Math.max(explicitTotal, 0)
    } else if (explicitTotal !== null) {
      active = Math.max(explicitTotal - paused, 0)
    }
  }

  let total = active + paused
  if (explicitTotal !== null) {
    total = explicitTotal < paused
      ? explicitTotal + paused
      : Math.max(explicitTotal, active + paused)
  }

  return { total, active, paused }
}
