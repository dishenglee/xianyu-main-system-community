import { request } from './client'

export type PeerMonitorTarget = {
  id: string
  sellerId?: string
  sellerNick?: string
  sellerAvatar?: string
  area?: string
  itemCount?: number | null
  activeItemCount?: number | null
  status?: 'active' | 'paused' | 'error' | string
  lastRefreshAt?: string | null
  lastChangeAt?: string | null
  lastError?: string | null
  changeCountToday?: number | null
}

export type PeerMonitorEvent = {
  id: string
  type: 'new_item' | 'removed_item' | 'price_changed' | 'profile_changed' | 'refresh_failed' | string
  itemId?: string
  title?: string
  previousValue?: string | number | null
  currentValue?: string | number | null
  occurredAt?: string
}

export type PeerMonitorTargetDetail = PeerMonitorTarget & {
  events?: PeerMonitorEvent[]
  items?: Array<Record<string, unknown>>
  refreshHistory?: Array<Record<string, unknown>>
}

/**
 * Peer monitor API contract. The backend owns target identity resolution and
 * snapshot comparisons; callers only pass the originating account/item facts.
 */
export const listPeerMonitorTargets = () => request<PeerMonitorTarget[] | { targets?: PeerMonitorTarget[]; data?: PeerMonitorTarget[] }>('GET', '/api/peer-monitor/targets')
export const getPeerMonitorTarget = (targetId: string) => request<PeerMonitorTargetDetail>('GET', `/api/peer-monitor/targets/${encodeURIComponent(targetId)}`)
export const refreshPeerMonitorTarget = (targetId: string) => request('POST', `/api/peer-monitor/targets/${encodeURIComponent(targetId)}/refresh`)
export const updatePeerMonitorTarget = (targetId: string, payload: { status: 'active' | 'paused' }) => request('PATCH', `/api/peer-monitor/targets/${encodeURIComponent(targetId)}`, payload)
export const getPeerMonitorCandidate = (payload: { sourceUid: string; itemId: string; sourceItem?: Record<string, unknown> | null }) => request<Record<string, unknown>>('POST', '/api/peer-monitor/candidates/resolve', {
  source_uid: payload.sourceUid,
  item_id: payload.itemId,
  source_item: payload.sourceItem ?? null,
})
export const getPeerMonitorCandidateFromLink = (link: string) => request<Record<string, unknown>>('POST', '/api/peer-monitor/candidates/resolve-link', { link })
export const getPeerMonitorCandidateReviews = (sellerId: string, page = 1, sourceUid?: string) => {
  const sourceUidQuery = sourceUid?.trim() ? `&source_uid=${encodeURIComponent(sourceUid.trim())}` : ''
  return request<Record<string, unknown>>('GET', `/api/peer-monitor/candidates/${encodeURIComponent(sellerId)}/reviews?page=${page}&rate_type=0&extra_tag=${sourceUidQuery}`)
}
export const createPeerMonitorTarget = (payload: Record<string, unknown>) => request<PeerMonitorTarget | { target?: PeerMonitorTarget }>('POST', '/api/peer-monitor/targets', payload)
