// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { sortAccountCards } from './accountCards'

it('sortAccountCards places unread accounts first, then sorts by latest chat time', () => {
  const cards = [
    { uid: 'a', unread: 0, latestChatTs: 100 },
    { uid: 'b', unread: 2, latestChatTs: 90 },
    { uid: 'c', unread: 1, latestChatTs: 120 },
  ]

  const sorted = sortAccountCards(cards)

  assert.deepEqual(sorted.map((card) => card.uid), ['c', 'b', 'a'])
})

it('sortAccountCards keeps a locked detail card at its previous index while unread state changes', () => {
  const cards = [
    { uid: 'a', unread: 0, latestChatTs: 100 },
    { uid: 'b', unread: 2, latestChatTs: 90 },
    { uid: 'c', unread: 0, latestChatTs: 110 },
  ]

  const sorted = sortAccountCards(cards, { a: 0 })

  assert.deepEqual(sorted.map((card) => card.uid), ['a', 'b', 'c'])
})

it('sortAccountCards restores natural order when no lock is present', () => {
  const cards = [
    { uid: 'a', unread: 0, latestChatTs: 100 },
    { uid: 'b', unread: 2, latestChatTs: 90 },
    { uid: 'c', unread: 0, latestChatTs: 110 },
  ]

  const sorted = sortAccountCards(cards)

  assert.deepEqual(sorted.map((card) => card.uid), ['b', 'c', 'a'])
})

it('sortAccountCards keeps multiple locked cards in stable slots', () => {
  const cards = [
    { uid: 'a', unread: 0, latestChatTs: 100 },
    { uid: 'b', unread: 1, latestChatTs: 95 },
    { uid: 'c', unread: 2, latestChatTs: 110 },
    { uid: 'd', unread: 0, latestChatTs: 90 },
  ]

  const sorted = sortAccountCards(cards, { a: 1, d: 3 })

  assert.deepEqual(sorted.map((card) => card.uid), ['c', 'a', 'b', 'd'])
})
