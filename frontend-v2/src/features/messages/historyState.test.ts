// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { mergeHistoryState } from './historyState'

it('mergeHistoryState appends older history without losing current messages', () => {
  const result = mergeHistoryState(
    {
      hasMore: true,
      nextCursor: 100,
      latestCursor: 140,
      messages: [
        { version: 120, ts: 1200, preview: 'mid' },
        { version: 140, ts: 1400, preview: 'latest' },
      ],
    },
    {
      hasMore: true,
      nextCursor: 80,
      latestCursor: 120,
      messages: [
        { version: 80, ts: 800, preview: 'older' },
        { version: 100, ts: 1000, preview: 'old' },
        { version: 120, ts: 1200, preview: 'mid' },
      ],
    },
    'older',
  )

  assert.equal(result.nextCursor, 80)
  assert.equal(result.latestCursor, 140)
  assert.deepEqual(
    result.messages.map((message) => message.version),
    [80, 100, 120, 140],
  )
})

it('mergeHistoryState refreshes latest page without dropping already loaded older history', () => {
  const result = mergeHistoryState(
    {
      hasMore: true,
      nextCursor: 80,
      latestCursor: 140,
      messages: [
        { version: 80, ts: 800, preview: 'older' },
        { version: 100, ts: 1000, preview: 'old' },
        { version: 120, ts: 1200, preview: 'mid' },
        { version: 140, ts: 1400, preview: 'latest' },
      ],
    },
    {
      hasMore: true,
      nextCursor: 120,
      latestCursor: 160,
      messages: [
        { version: 120, ts: 1200, preview: 'mid' },
        { version: 140, ts: 1400, preview: 'latest' },
        { version: 160, ts: 1600, preview: 'newest' },
      ],
    },
    'refresh',
  )

  assert.equal(result.nextCursor, 80)
  assert.equal(result.latestCursor, 160)
  assert.deepEqual(
    result.messages.map((message) => message.version),
    [80, 100, 120, 140, 160],
  )
})

it('mergeHistoryState normalizes message timestamps from seconds and compact datetime strings', () => {
  const result = mergeHistoryState(
    {},
    {
      messages: [
        { messageId: 'a', version: 1710000000, ts: 1710000000, preview: 'seconds' },
        { messageId: 'b', version: 20260415123045, ts: '20260415123045', preview: 'compact' },
      ],
    },
    'replace',
  )

  assert.equal(result.messages[0].ts, 1710000000 * 1000)
  assert.equal(result.messages[1].ts, new Date(2026, 3, 15, 12, 30, 45).getTime())
})

it('mergeHistoryState dedupes self-sent location card echoes even when preview image differs', () => {
  const result = mergeHistoryState(
    {
      messages: [
        {
          messageId: 'client-location-uuid',
          kind: 'card',
          contentType: 30,
          text: '地产上品人家7幢',
          title: '地产上品人家7幢',
          imageUrl: 'https://local-preview.example/location.png',
          cardMeta: {
            kind: 'location',
            title: '地产上品人家7幢',
            content: '红旗中路',
            latitude: '27.867133',
            longitude: '113.164345',
            previewUrl: 'https://local-preview.example/location.png',
          },
          isSelf: true,
          ts: 1_720_000_000_000,
        },
      ],
    },
    {
      messages: [
        {
          messageId: '999.PNM',
          kind: 'card',
          contentType: 30,
          text: '地产上品人家7幢',
          title: '地产上品人家7幢',
          imageUrl: '',
          cardMeta: {
            kind: 'location',
            title: '地产上品人家7幢',
            content: '红旗中路',
            latitude: '27.867133',
            longitude: '113.164345',
            previewUrl: 'https://server-preview.example/location.png',
          },
          isSelf: true,
          ts: 1_720_000_000_500,
        },
      ],
    },
    'refresh',
  )

  assert.equal(result.messages.length, 1)
  assert.equal(result.messages[0].messageId, 'client-location-uuid')
})
