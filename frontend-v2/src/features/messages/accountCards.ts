// @ts-nocheck
function hasUnread(card = {}) {
  return Number(card.unread || 0) > 0 ? 1 : 0
}

function latestChatTs(card = {}) {
  const value = Number(card.latestChatTs || 0)
  return Number.isFinite(value) ? value : 0
}

function compareAccountCards(left = {}, right = {}) {
  const unreadDiff = hasUnread(right) - hasUnread(left)
  if (unreadDiff !== 0) return unreadDiff
  return latestChatTs(right) - latestChatTs(left)
}

function clampIndex(index, maxIndex) {
  const numeric = Number(index)
  if (!Number.isFinite(numeric)) return 0
  if (maxIndex <= 0) return 0
  return Math.min(Math.max(Math.trunc(numeric), 0), maxIndex)
}

function placeLockedCard(slots, card, preferredIndex) {
  if (!slots.length) return

  let index = clampIndex(preferredIndex, slots.length - 1)
  while (index < slots.length && slots[index]) {
    index += 1
  }

  if (index >= slots.length) {
    index = slots.length - 1
    while (index >= 0 && slots[index]) {
      index -= 1
    }
  }

  if (index >= 0 && index < slots.length) {
    slots[index] = card
  }
}

export function sortAccountCards(cards = [], lockedPositionsByUid = {}) {
  const normalizedCards = Array.isArray(cards) ? [...cards] : []
  const sortedCards = normalizedCards.sort(compareAccountCards)
  const lockEntries = Object.entries(lockedPositionsByUid || {})
    .filter(([uid]) => normalizedCards.some((card) => card.uid === uid))
    .sort((left, right) => Number(left[1] || 0) - Number(right[1] || 0))

  if (!lockEntries.length) {
    return sortedCards
  }

  const slots = new Array(sortedCards.length)
  const cardsByUid = new Map(normalizedCards.map((card) => [card.uid, card]))
  const lockedUids = new Set()

  for (const [uid, index] of lockEntries) {
    const card = cardsByUid.get(uid)
    if (!card) continue
    placeLockedCard(slots, card, index)
    lockedUids.add(uid)
  }

  const unlockedCards = sortedCards.filter((card) => !lockedUids.has(card.uid))
  let nextUnlockedIndex = 0
  for (let slotIndex = 0; slotIndex < slots.length; slotIndex += 1) {
    if (slots[slotIndex]) continue
    slots[slotIndex] = unlockedCards[nextUnlockedIndex]
    nextUnlockedIndex += 1
  }

  return slots.filter(Boolean)
}
