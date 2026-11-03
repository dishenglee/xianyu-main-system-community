// @ts-nocheck
export function getSearchViewState({ searching, resultCount }) {
  const hasResults = Number(resultCount || 0) > 0
  return {
    showEmptyState: !searching && !hasResults,
    showFullscreenLoading: searching && !hasResults,
    showInlineRefreshing: searching && hasResults,
  }
}
