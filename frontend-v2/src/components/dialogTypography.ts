import { isValidElement, type CSSProperties, type ReactNode } from 'react'

function extractDialogText(node: ReactNode): string {
  if (node == null || typeof node === 'boolean') return ''
  if (typeof node === 'string' || typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(extractDialogText).join('')
  if (isValidElement<{ children?: ReactNode }>(node)) {
    return extractDialogText(node.props.children)
  }
  return ''
}

export function dialogMessageTextStyle(message: ReactNode): CSSProperties {
  const text = extractDialogText(message).replace(/\s+/g, ' ').trim()
  const length = text.length

  if (length >= 48) {
    return { fontSize: '13px', lineHeight: '20px' }
  }

  if (length >= 24) {
    return { fontSize: '15px', lineHeight: '24px' }
  }

  return { fontSize: '18px', lineHeight: '28px' }
}
