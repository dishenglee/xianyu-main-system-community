// @ts-nocheck
import { describe, it } from 'vitest'
import assert from 'node:assert'

import { getSearchViewState } from './searchUiState'

describe('getSearchViewState', () => {
  it('uses full-page loading for the initial search with no results', () => {
    assert.deepEqual(
      getSearchViewState({ searching: true, resultCount: 0 }),
      {
        showEmptyState: false,
        showFullscreenLoading: true,
        showInlineRefreshing: false,
      },
    )
  })

  it('keeps the current results visible during remote filter refreshes', () => {
    assert.deepEqual(
      getSearchViewState({ searching: true, resultCount: 12 }),
      {
        showEmptyState: false,
        showFullscreenLoading: false,
        showInlineRefreshing: true,
      },
    )
  })

  it('shows the empty state only when idle and there are no results', () => {
    assert.deepEqual(
      getSearchViewState({ searching: false, resultCount: 0 }),
      {
        showEmptyState: true,
        showFullscreenLoading: false,
        showInlineRefreshing: false,
      },
    )
  })
})
