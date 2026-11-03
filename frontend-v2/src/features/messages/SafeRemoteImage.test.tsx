import { act, render } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { storage } = vi.hoisted(() => {
  const storage = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, String(value)),
      removeItem: (key: string) => storage.delete(key),
      clear: () => storage.clear(),
    },
  })
  return { storage }
})

import { useSensitiveVisibilityStore } from '../../store/sensitiveVisibilityStore'

import {
  SafeRemoteImage,
  buildImageProxyUrl,
  isRemoteImageUrl,
  normalizeRemoteImageUrl,
} from './SafeRemoteImage'

describe('SafeRemoteImage helpers', () => {
  beforeEach(() => {
    storage.clear()
    act(() => useSensitiveVisibilityStore.setState({ hiddenByScope: {} }))
  })
  it('detects remote image URLs only', () => {
    expect(isRemoteImageUrl('https://gw.alicdn.com/a.jpg')).toBe(true)
    expect(isRemoteImageUrl('//gw.alicdn.com/a.jpg')).toBe(true)
    expect(isRemoteImageUrl('/assets/local.png')).toBe(false)
    expect(isRemoteImageUrl('data:image/png;base64,aaa')).toBe(false)
  })

  it('normalizes protocol-relative URLs', () => {
    expect(normalizeRemoteImageUrl('//gw.alicdn.com/a.jpg')).toBe('https://gw.alicdn.com/a.jpg')
  })

  it('builds protected local image proxy URLs', () => {
    const proxyUrl = buildImageProxyUrl(
      'http://127.0.0.1:61234',
      'https://gw.alicdn.com/a.jpg?x=1',
      'token-1',
    )
    expect(proxyUrl).toBe(
      'http://127.0.0.1:61234/api/assets/image-proxy?url=https%3A%2F%2Fgw.alicdn.com%2Fa.jpg%3Fx%3D1&token=token-1',
    )
  })

  it('marks product images as hidden through the current page scope', () => {
    const { container } = render(
      <SafeRemoteImage src="/asset/item.jpg" alt="商品名称" sensitiveItem fallback={null} />,
    )

    const image = container.querySelector('img')
    expect(image?.getAttribute('data-sensitive-hidden')).toBe('false')

    act(() => useSensitiveVisibilityStore.setState({ hiddenByScope: { app: true } }))
    expect(image?.getAttribute('data-sensitive-hidden')).toBe('true')
  })
})
