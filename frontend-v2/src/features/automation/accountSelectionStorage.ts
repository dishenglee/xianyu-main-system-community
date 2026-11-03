import { useAuthStore } from '../../store/authStore'

const PREFIX = 'xy.accountSelection.v1.'

function hashScope(value: string): string {
  let hash = 0
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash * 31 + value.charCodeAt(i)) % 2_147_483_647
  }
  return hash.toString(36)
}

function ownerScope() {
  const state = useAuthStore.getState()
  return state.user?.username || (state.token ? `token:${hashScope(state.token)}` : 'anonymous')
}

function keyOf(storageKey: string) {
  return `${PREFIX}${ownerScope()}.${storageKey}`
}

function normalizeUids(selectedUids: readonly string[] = []) {
  const seen = new Set<string>()
  const result: string[] = []

  for (const uid of selectedUids) {
    const normalized = String(uid || '').trim()
    if (!normalized || seen.has(normalized)) continue
    seen.add(normalized)
    result.push(normalized)
  }

  return result
}

export function loadStoredAccountSelection(storageKey: string) {
  try {
    const raw = localStorage.getItem(keyOf(storageKey))
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? normalizeUids(parsed) : []
  } catch {
    return []
  }
}

export function saveStoredAccountSelection(storageKey: string, selectedUids: readonly string[]) {
  try {
    localStorage.setItem(keyOf(storageKey), JSON.stringify(normalizeUids(selectedUids)))
  } catch {
    /* ignore */
  }
}
