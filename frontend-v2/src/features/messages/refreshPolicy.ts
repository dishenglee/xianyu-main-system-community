// @ts-nocheck
export function resolveRefreshInterval({
  transportConnected = false,
  activeUids = [],
  connectionStates = {},
  fallbackRefreshInterval = 15000,
  degradedRefreshInterval,
} = {}) {
  if (!transportConnected) {
    return fallbackRefreshInterval
  }

  const hasDegradedAccount = (Array.isArray(activeUids) ? activeUids : []).some((uid) => {
    const status = String(connectionStates?.[uid]?.status || '').trim()
    return status !== 'connected'
  })

  if (!hasDegradedAccount) {
    return null
  }

  const normalizedDegradedInterval = Number(degradedRefreshInterval || 0)
  if (Number.isFinite(normalizedDegradedInterval) && normalizedDegradedInterval > 0) {
    return normalizedDegradedInterval
  }
  return fallbackRefreshInterval
}
