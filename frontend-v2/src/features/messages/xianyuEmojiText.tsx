import { XIANYU_EMOJI_MAP } from './xianyuEmojis'
import { SafeRemoteImage } from './SafeRemoteImage'

export function containsXianyuEmoji(value: string): boolean {
  const pattern = /\[([^\[\]\n]{1,16})\]/g
  let match: RegExpExecArray | null
  while ((match = pattern.exec(value)) !== null) {
    if (XIANYU_EMOJI_MAP[match[1]]) return true
  }
  return false
}

export function renderXianyuEmojiText(raw: string) {
  const pattern = /\[([^\[\]\n]{1,16})\]/g
  const parts: any[] = []
  let lastIndex = 0
  let match: RegExpExecArray | null
  let idx = 0

  while ((match = pattern.exec(raw)) !== null) {
    if (match.index > lastIndex) {
      parts.push(raw.slice(lastIndex, match.index))
    }

    const token = match[1]
    const emojiUrl = XIANYU_EMOJI_MAP[token]
    if (emojiUrl) {
      parts.push(
        <SafeRemoteImage
          key={`xe-${idx}`}
          src={emojiUrl}
          alt={`[${token}]`}
          title={token}
          className="inline-block h-5 w-5 align-middle"
          style={{ verticalAlign: '-4px' }}
          draggable={false}
          fallback={(
            <span
              className="mx-0.5 inline-block rounded px-1 align-middle text-[12px]"
              style={{ backgroundColor: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
            >
              [{token}]
            </span>
          )}
        />,
      )
    } else {
      parts.push(raw.slice(match.index, pattern.lastIndex))
    }
    lastIndex = pattern.lastIndex
    idx += 1
  }

  if (lastIndex < raw.length) parts.push(raw.slice(lastIndex))
  if (!parts.length) return raw
  return <>{parts}</>
}
