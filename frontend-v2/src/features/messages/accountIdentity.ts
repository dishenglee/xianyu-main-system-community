// @ts-nocheck
export function resolveAccountIdentity(account, sessions = []) {
  const uid = String(account?.uid || '').trim()
  const fallback = sessions.find((session) => (
    String(session?.selfUid || '').trim() === uid
      && (String(session?.selfNick || '').trim() || String(session?.selfLogo || '').trim())
  )) || {}

  return {
    nick: String(fallback.selfNick || '').trim() || String(account?.nick || '').trim() || uid,
    avatar: String(fallback.selfLogo || '').trim() || String(account?.avatar || '').trim(),
  }
}
