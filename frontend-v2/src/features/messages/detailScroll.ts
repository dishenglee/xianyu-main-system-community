// @ts-nocheck
function toNonNegativeInt(value) {
  const numeric = Number(value || 0)
  if (!Number.isFinite(numeric) || numeric <= 0) return 0
  return Math.trunc(numeric)
}

export function resolveOpenDetailScrollTarget(messageCount = 0, unreadCount = 0) {
  const total = toNonNegativeInt(messageCount)
  const unread = toNonNegativeInt(unreadCount)

  if (total <= 0) {
    return { mode: 'top', index: 0 }
  }

  if (unread <= 0) {
    return { mode: 'bottom', index: total - 1 }
  }

  return {
    mode: 'anchor',
    index: Math.max(total - unread, 0),
  }
}
