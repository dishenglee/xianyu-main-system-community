// @ts-nocheck
// 闲鱼通知消息自带 HTML 片段（strong/font/a），后端原样透传。
// 做个极简白名单：只保留 a/strong/b/em/font/br/p/div/span；
// a 仅保留 href 且强制 target=_blank rel=noopener；font 仅保留 color。

const ALLOWED_TAGS = new Set(['A', 'STRONG', 'B', 'EM', 'I', 'U', 'FONT', 'BR', 'P', 'DIV', 'SPAN'])

function cleanNode(node) {
  const children = Array.from(node.childNodes)
  for (const child of children) {
    if (child.nodeType === 1) {
      const el = child
      if (!ALLOWED_TAGS.has(el.tagName)) {
        const text = document.createTextNode(el.textContent || '')
        el.replaceWith(text)
        continue
      }
      for (const attr of Array.from(el.attributes)) {
        const name = attr.name.toLowerCase()
        if (el.tagName === 'A' && name === 'href') continue
        if (el.tagName === 'FONT' && name === 'color') continue
        el.removeAttribute(attr.name)
      }
      if (el.tagName === 'A') {
        el.setAttribute('target', '_blank')
        el.setAttribute('rel', 'noopener noreferrer')
      }
      if (el.tagName === 'FONT') {
        const color = el.getAttribute('color')
        if (color) {
          el.style.color = color
          el.removeAttribute('color')
        }
      }
      cleanNode(el)
    }
  }
}

// Module-level LRU-ish cache. Every React re-render of the messages page
// re-evaluates summaryText for hundreds of sessions + renderMessageContent
// for every bubble in the open detail panel — without caching, each call
// instantiates a DOMParser which is the dominant cost (tens of seconds on
// busy sessions). Keyed by raw text so identical previews/messages dedupe.
const SANITIZE_CACHE = new Map<string, { html: string | null; text: string }>()
const SANITIZE_CACHE_MAX = 4096

function rememberSanitized(raw: string, result: { html: string | null; text: string }) {
  // Cheap FIFO eviction: drop the oldest insertion once we hit the cap.
  if (SANITIZE_CACHE.size >= SANITIZE_CACHE_MAX) {
    const firstKey = SANITIZE_CACHE.keys().next().value
    if (firstKey !== undefined) SANITIZE_CACHE.delete(firstKey)
  }
  SANITIZE_CACHE.set(raw, result)
  return result
}

export function sanitizeMessageHtml(text) {
  const raw = String(text ?? '')
  const hit = SANITIZE_CACHE.get(raw)
  if (hit) return hit
  if (!/<[a-z][^>]*>/i.test(raw)) {
    return rememberSanitized(raw, { html: null, text: raw })
  }
  try {
    const doc = new DOMParser().parseFromString(`<div>${raw}</div>`, 'text/html')
    const container = doc.body.firstChild
    if (!container) return rememberSanitized(raw, { html: null, text: raw })
    cleanNode(container)
    return rememberSanitized(raw, {
      html: container.innerHTML,
      text: container.textContent || '',
    })
  } catch {
    return rememberSanitized(raw, { html: null, text: raw })
  }
}
