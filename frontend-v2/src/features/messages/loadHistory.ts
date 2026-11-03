import { mergeHistoryState } from './historyState'

export const FULL_HISTORY_PAGE_SIZE = 50
export const FULL_HISTORY_PAGE_LIMIT = 50
// Legacy notice — previously surfaced when we hit the pagination cap while
// talking to upstream mtop. Now that AggregateMessages reads local DB in one
// shot (and the DB has everything the worker persisted), the cap isn't a
// "truncated history" situation. Keep the constant empty so no confusing
// "请到闲鱼 APP 查看" bubble shows up.
export const FULL_HISTORY_LIMIT_NOTICE = ''

export interface HistoryPage<TMessage = unknown> {
  messages?: TMessage[]
  nextCursor?: unknown
  latestCursor?: unknown
  hasMore?: boolean
}

export interface HistoryProgress {
  count: number
  pages: number
}

export interface LoadCompleteHistoryOptions<TMessage = unknown> {
  fetchPage: (
    uid: string,
    sessionId: string,
    sessionType: number,
    num: number,
    cursor: unknown,
  ) => Promise<HistoryPage<TMessage>>
  uid: string
  sessionId: string
  sessionType?: number
  pageSize?: number
  maxPages?: number
  initialCursor?: unknown
  onProgress?: (progress: HistoryProgress) => void
}

export interface LoadCompleteHistoryResult<TMessage = unknown> {
  messages: TMessage[]
  nextCursor: unknown
  latestCursor: unknown
  pages: number
  reachedLimit: boolean
  notice: string
}

function hasCursor(value: unknown): boolean {
  return value !== null && value !== undefined && value !== ''
}

export async function loadCompleteHistory<TMessage = unknown>({
  fetchPage,
  uid,
  sessionId,
  sessionType = 1,
  pageSize = FULL_HISTORY_PAGE_SIZE,
  maxPages = FULL_HISTORY_PAGE_LIMIT,
  initialCursor = 0,
  onProgress,
}: LoadCompleteHistoryOptions<TMessage>): Promise<LoadCompleteHistoryResult<TMessage>> {
  let cursor: unknown = initialCursor
  let latestCursor: unknown = null
  let mergedMessages: TMessage[] = []
  let pages = 0
  let nextCursor: unknown = null

  while (pages < maxPages) {
    const response = await fetchPage(uid, sessionId, sessionType, pageSize, cursor)
    const pageMessages = Array.isArray(response?.messages) ? response.messages : []

    mergedMessages = mergeHistoryState(
      { messages: mergedMessages },
      { messages: pageMessages },
      'refresh',
    ).messages as TMessage[]

    pages += 1
    latestCursor = response?.latestCursor ?? latestCursor
    const prevCursor = cursor
    nextCursor = response?.nextCursor ?? null
    onProgress?.({ count: mergedMessages.length, pages })

    // Break when the backend explicitly says there's nothing more, when it
    // hands back the same cursor we just sent (infinite-loop guard — the MTOP
    // `message.sync` path returns `nextCursor == cursor` once it's out of
    // pages but still populates the field), or when we simply got 0 messages.
    if (response?.hasMore === false) break
    if (!hasCursor(nextCursor)) break
    if (nextCursor === prevCursor) break
    if (pageMessages.length === 0) break
    cursor = nextCursor
  }

  const reachedLimit = pages >= maxPages && hasCursor(nextCursor)

  return {
    messages: mergedMessages,
    nextCursor: reachedLimit ? nextCursor : (hasCursor(nextCursor) ? nextCursor : null),
    latestCursor,
    pages,
    reachedLimit,
    notice: reachedLimit ? FULL_HISTORY_LIMIT_NOTICE : '',
  }
}
