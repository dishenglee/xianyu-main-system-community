import { useEffect, useMemo, useState } from 'react'

import { getAuthToken, getBaseURL } from '../../api/client'
import { useSensitiveHidden } from '../../components/SensitiveValue'
import { isTauriRuntime } from '../../tauriRuntime'

type SafeRemoteImageProps = Omit<React.ImgHTMLAttributes<HTMLImageElement>, 'src' | 'onError'> & {
  src?: string
  fallback: React.ReactNode
  sensitiveItem?: boolean
}

export function isRemoteImageUrl(src: string): boolean {
  return /^https?:\/\//i.test(src.trim()) || /^\/\//.test(src.trim())
}

export function normalizeRemoteImageUrl(src: string): string {
  const trimmed = src.trim()
  if (trimmed.startsWith('//')) return `https:${trimmed}`
  return trimmed
}

export function buildImageProxyUrl(baseURL: string, src: string, token: string): string {
  const params = new URLSearchParams({ url: normalizeRemoteImageUrl(src) })
  if (token) params.set('token', token)
  return `${baseURL}/api/assets/image-proxy?${params.toString()}`
}

export function SafeRemoteImage({ src, fallback, sensitiveItem = false, className, ...imgProps }: SafeRemoteImageProps) {
  const isSensitiveHidden = useSensitiveHidden()
  const originalSrc = useMemo(() => normalizeRemoteImageUrl(String(src || '')), [src])
  const [displaySrc, setDisplaySrc] = useState(() => {
    if (!originalSrc) return ''
    return isTauriRuntime() && isRemoteImageUrl(originalSrc) ? '' : originalSrc
  })
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    let cancelled = false
    setFailed(false)
    if (!originalSrc) {
      setDisplaySrc('')
      return () => {
        cancelled = true
      }
    }
    if (!isTauriRuntime() || !isRemoteImageUrl(originalSrc)) {
      setDisplaySrc(originalSrc)
      return () => {
        cancelled = true
      }
    }
    setDisplaySrc('')
    getBaseURL()
      .then((baseURL) => {
        if (!cancelled) setDisplaySrc(buildImageProxyUrl(baseURL, originalSrc, getAuthToken()))
      })
      .catch(() => {
        if (!cancelled) setDisplaySrc(originalSrc)
      })
    return () => {
      cancelled = true
    }
  }, [originalSrc])

  if (!displaySrc || failed) return <>{fallback}</>

  return (
    <img
      {...imgProps}
      src={displaySrc}
      className={`${sensitiveItem ? 'sensitive-item-image ' : ''}${className || ''}`.trim()}
      data-sensitive-hidden={sensitiveItem ? isSensitiveHidden : undefined}
      onError={() => setFailed(true)}
    />
  )
}
