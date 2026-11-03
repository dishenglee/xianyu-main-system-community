import type { Card } from '../../api/types'

export type CardImportPayload = Partial<Card> & {
  cardKey: string
  cardContent: string
  cardPoolTag: string
  reusable: boolean
}

export function normalizeCardImportText(rawText: string) {
  return String(rawText || '').replace(/\r\n?/g, '\n').trim()
}

export function splitOneTimeCardLines(rawText: string) {
  return normalizeCardImportText(rawText)
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
}

export function countCardImportEntries(rawText: string, reusable: boolean) {
  if (reusable) return normalizeCardImportText(rawText) ? 1 : 0
  return splitOneTimeCardLines(rawText).length
}

export function buildCardImportPayloads({
  rawText,
  cardPoolTag = '',
  reusable,
  createdAt = Date.now(),
}: {
  rawText: string
  cardPoolTag?: string
  reusable: boolean
  createdAt?: number
}): CardImportPayload[] {
  const tag = String(cardPoolTag || '').trim()
  const prefix = tag || 'CARD'
  const contents = reusable
    ? [normalizeCardImportText(rawText)].filter(Boolean)
    : splitOneTimeCardLines(rawText)

  return contents.map((content, index) => ({
    cardKey: `${prefix}-${createdAt}-${index + 1}`,
    cardContent: content,
    cardPoolTag: tag,
    reusable,
  }))
}
