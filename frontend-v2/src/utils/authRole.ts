export function isAdminUser(user?: { role?: string | null } | null): boolean {
  return String(user?.role || '').trim().toLowerCase() === 'admin'
}
