import { createContext, useContext } from 'react'
import type { CSSProperties, ElementType, ImgHTMLAttributes, ReactNode } from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { useLocation } from 'react-router-dom'
import { useSensitiveVisibilityStore } from '../store/sensitiveVisibilityStore'

export type SensitiveValueKind =
  | 'account'
  | 'uid'
  | 'item-title'
  | 'money'
  | 'order-id'
  | 'buyer'
  | 'phone'
  | 'address'

interface SensitiveValueProps {
  /** Documents the value category at call sites and makes privacy reviews easy. */
  kind: SensitiveValueKind
  value?: ReactNode
  children?: ReactNode
  className?: string
  style?: CSSProperties
  title?: string
  as?: ElementType
  mask?: ReactNode
}

const DEFAULT_MASK = '••••••'
const SensitiveScopeContext = createContext<string | null>(null)

/** Maps nested routes to the page family that owns their sensitive display state. */
export function resolveSensitiveScope(pathname = typeof window === 'undefined' ? '/' : window.location.pathname) {
  const path = String(pathname || '/').replace(/\/+$/, '') || '/'
  if (path === '/messages' || path.startsWith('/messages/')) return 'messages'
  if (path === '/accounts' || path.startsWith('/accounts/')) return 'accounts'
  if (path === '/items' || path.startsWith('/items/')) return 'items'
  if (path === '/orders' || path.startsWith('/orders/')) return 'orders'
  if (path === '/notifications' || path.startsWith('/notifications/')) return 'notifications'
  if (path === '/search' || path.startsWith('/search/')) return 'search'
  if (path === '/growth' || path.startsWith('/growth/') || path.startsWith('/super-polish')) return 'growth'
  if (path === '/me' || path.startsWith('/me/')) return 'me'
  return path.split('/').filter(Boolean)[0] || 'app'
}

/** Bridges React Router changes into the shared display components. */
export function SensitiveScopeProvider({ children }: { children: ReactNode }) {
  const { pathname } = useLocation()
  return (
    <SensitiveScopeContext.Provider value={resolveSensitiveScope(pathname)}>
      {children}
    </SensitiveScopeContext.Provider>
  )
}

/** Reads the scope from router context, with a window fallback for isolated component tests. */
export function useSensitiveScope() {
  return useContext(SensitiveScopeContext) ?? resolveSensitiveScope()
}

export function useSensitiveHidden() {
  const scope = useSensitiveScope()
  return useSensitiveVisibilityStore((state) => Boolean(state.hiddenByScope[scope]))
}

function textValue(value: unknown) {
  return String(value ?? '')
}

function maskWithEdges(value: string, leading: number, trailing: number) {
  const characters = Array.from(value)
  if (characters.length <= 1) return DEFAULT_MASK
  if (characters.length <= 2) return `${characters.slice(0, 1).join('')}******`
  return `${characters.slice(0, leading).join('')}******${characters.slice(-trailing).join('')}`
}

function maskIdentifier(value: string) {
  const prefix = value.match(/^[^\p{L}\p{N}]*/u)?.[0] ?? ''
  const identifier = value.slice(prefix.length)
  if (Array.from(identifier).length <= 7) return `${prefix}${Array.from(identifier).slice(0, 1).join('')}******`
  return `${prefix}${maskWithEdges(identifier, 3, 4)}`
}

function maskPhone(value: string) {
  const characters = Array.from(value)
  if (characters.length <= 7) return maskWithEdges(value, 1, 1)
  return `${characters.slice(0, 3).join('')}****${characters.slice(-4).join('')}`
}

function maskMoney(value: string) {
  const hasDigits = /[0-9０-９]/.test(value)
  if (!hasDigits) return DEFAULT_MASK
  return value.replace(/[0-9０-９]/g, '*')
}

/** Returns a shape-preserving masked string without exposing the original value. */
export function maskSensitiveText(kind: SensitiveValueKind, value: unknown) {
  const text = textValue(value)
  if (!text) return DEFAULT_MASK

  switch (kind) {
    case 'phone':
      return maskPhone(text)
    case 'uid':
    case 'order-id':
      return maskIdentifier(text)
    case 'money':
      return maskMoney(text)
    case 'address':
      return `${Array.from(text).slice(0, 3).join('')}******`
    case 'account':
    case 'item-title':
    case 'buyer':
      return maskWithEdges(text, 1, 1)
  }
}

/** Declaratively hides a value without putting page-specific masking rules in UI modules. */
export function SensitiveValue({
  kind,
  value,
  children,
  className,
  style,
  title,
  as: Component = 'span',
  mask,
}: SensitiveValueProps) {
  const isSensitiveHidden = useSensitiveHidden()
  const content = children ?? value
  const shouldMask = isSensitiveHidden
  const maskedContent = mask ?? maskSensitiveText(kind, content)
  const maskedTitle = typeof maskedContent === 'string' || typeof maskedContent === 'number'
    ? String(maskedContent)
    : '敏感信息已隐藏'

  return (
    <Component
      className={className}
      style={style}
      title={shouldMask ? maskedTitle : title}
      aria-label={shouldMask ? '敏感信息已隐藏' : undefined}
      data-sensitive-value={kind}
    >
      {shouldMask ? maskedContent : content}
    </Component>
  )
}

/** Use this for DOM attributes and native controls that cannot render React nodes. */
export function useSensitiveText(kind: SensitiveValueKind, value: unknown, mask?: string) {
  const isSensitiveHidden = useSensitiveHidden()
  return isSensitiveHidden ? (mask ?? maskSensitiveText(kind, value)) : textValue(value)
}

/** Prevents avatar fallback initials from revealing a sensitive account name. */
export function useSensitiveAvatarInitial(value: unknown) {
  const isSensitiveHidden = useSensitiveHidden()
  if (isSensitiveHidden) return '•'
  return Array.from(textValue(value))[0]?.toUpperCase() ?? '?'
}

/** Reusable product-image privacy treatment for item list cards. */
export function SensitiveItemImage({ className = '', ...props }: ImgHTMLAttributes<HTMLImageElement>) {
  const isSensitiveHidden = useSensitiveHidden()

  return (
    <img
      {...props}
      className={`sensitive-item-image ${className}`.trim()}
      data-sensitive-hidden={isSensitiveHidden}
    />
  )
}

export function SensitiveVisibilityToggle({ compact = false, className = '' }: { compact?: boolean; className?: string }) {
  const scope = useSensitiveScope()
  const isSensitiveHidden = useSensitiveHidden()
  const toggleSensitiveVisibility = useSensitiveVisibilityStore((state) => state.toggleSensitiveVisibility)
  const label = isSensitiveHidden ? '显示敏感信息' : '隐藏敏感信息'
  const Icon = isSensitiveHidden ? Eye : EyeOff

  return (
    <button
      type="button"
      onClick={() => toggleSensitiveVisibility(scope)}
      aria-label={label}
      aria-pressed={isSensitiveHidden}
      title={label}
      className={`sensitive-visibility-toggle ${className}`.trim()}
    >
      <Icon className="w-5 h-5" aria-hidden="true" />
      {!compact ? <span>{isSensitiveHidden ? '显示信息' : '隐藏信息'}</span> : null}
    </button>
  )
}
