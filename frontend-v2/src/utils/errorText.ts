const URL_RE = /https?:\/\/[^\s)]+/gi
const URL_TEST_RE = /https?:\/\/[^\s)]+/i
const SERVICE_HOST_RE = /\b(?:license|sign)\.gooofish\.shop\b/gi

export function sanitizeServiceText(
  value: string,
  fallback = '服务暂时不可用，请稍后再试',
): string {
  const raw = String(value || '').trim()
  if (!raw) return fallback

  if (/bootstrap token is not allowed on this route/i.test(raw)) {
    return '当前授权状态不可用，请退出后重新登录授权账号'
  }
  if (/(invalid license token|unauthorized|http\s*401)/i.test(raw)) {
    return '授权已失效，请退出后重新登录授权账号'
  }
  if (/(forbidden|http\s*403)/i.test(raw)) {
    return '当前授权状态不可用，请退出后重新登录授权账号'
  }
  if (/(error sending request for url|failed to send request|failed to fetch|could not connect to the server|network connection was lost|connection refused|dns error)/i.test(raw)) {
    return '服务连接失败，请检查当前网络后重试'
  }
  if (/(timed out|timeout)/i.test(raw) && (URL_TEST_RE.test(raw) || /request/i.test(raw))) {
    return '服务请求超时，请稍后重试'
  }

  const text = raw
    .replace(URL_RE, '')
    .replace(SERVICE_HOST_RE, '')
    .replace(/\(\s*\)/g, '')
    .replace(/\bapi\/[^\s)]+/gi, '')
    .replace(/\s{2,}/g, ' ')
    .replace(/\s+([,.;:，。；：])/g, '$1')
    .trim()

  return text || fallback
}

export function friendlyLicenseError(
  error: unknown,
  fallback = '操作失败，请稍后再试',
): string {
  const raw = error instanceof Error ? error.message : String(error || '')
  return sanitizeServiceText(raw, fallback)
}
