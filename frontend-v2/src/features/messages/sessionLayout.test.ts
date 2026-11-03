// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import {
  getSessionMeta,
  pickDefaultSession,
  shouldShowUnreadBadge,
  splitSessionSections,
} from './sessionLayout'

it('splitSessionSections pins notification sessions above buyer chats', () => {
  const sessions = [
    { sessionId: 'chat-new', sessionType: 1, ts: 300 },
    { sessionId: 'system', sessionType: 3, ts: 100 },
    { sessionId: 'interact', sessionType: 6, ts: 200 },
    { sessionId: 'chat-old', sessionType: 1, ts: 50 },
  ]

  const sections = splitSessionSections(sessions)

  assert.deepEqual(
    sections.notificationSessions.map((session) => session.sessionId),
    ['interact', 'system'],
  )
  assert.deepEqual(
    sections.chatSessions.map((session) => session.sessionId),
    ['chat-new', 'chat-old'],
  )
  assert.deepEqual(
    sections.orderedSessions.map((session) => session.sessionId),
    ['interact', 'system', 'chat-new', 'chat-old'],
  )
})

it('chat session meta uses neutral label instead of assuming buyer inquiry', () => {
  assert.equal(getSessionMeta(1).label, '商品会话')
})

it('system notifications do not show unread badges by default', () => {
  assert.equal(shouldShowUnreadBadge({ sessionType: 3, unread: 9 }), false)
  assert.equal(shouldShowUnreadBadge({ sessionType: 6, unread: 2 }), true)
  assert.equal(shouldShowUnreadBadge({ sessionType: 1, unread: 1 }), true)
})

it('pickDefaultSession keeps current selection when it still exists', () => {
  const sessions = [
    { sessionId: 'chat-new', sessionType: 1, ts: 300 },
    { sessionId: 'system', sessionType: 3, ts: 100 },
    { sessionId: 'interact', sessionType: 6, ts: 200 },
  ]

  const selected = pickDefaultSession(sessions, 'chat-new')

  assert.equal(selected?.sessionId, 'chat-new')
})

it('pickDefaultSession falls back to the first pinned notification', () => {
  const sessions = [
    { sessionId: 'chat-new', sessionType: 1, ts: 300 },
    { sessionId: 'system', sessionType: 3, ts: 100 },
    { sessionId: 'interact', sessionType: 6, ts: 200 },
  ]

  const selected = pickDefaultSession(sessions)

  assert.equal(selected?.sessionId, 'interact')
})

it('splitSessionSections sorts chats by normalized activity time when sortIndex uses compact datetime format', () => {
  const sessions = [
    { sessionId: 'chat-old', sessionType: 1, ts: 1774257346000, sortIndex: 20260323171546 },
    { sessionId: 'chat-new', sessionType: 1, ts: 1775527000000, sortIndex: 1775527000000 },
  ]

  const sections = splitSessionSections(sessions)

  assert.deepEqual(
    sections.chatSessions.map((session) => session.sessionId),
    ['chat-new', 'chat-old'],
  )
})

// 闲鱼 fans seller-workbench (sessionType=53) notifications across 3 platform
// sender accounts, so a single uid ends up with 3 distinct sessionIds for the
// same "工作台通知" stream. The phone collapses these to one entry; mirror
// that in our UI and keep only the most recent sub-session.
it('splitSessionSections collapses sessionType=53 fan-out to one entry per uid', () => {
  const sessions = [
    { sessionId: 'workbench-a', sessionType: 53, ts: 1000, peerNick: '工作台通知', _uid: 'u1' },
    { sessionId: 'workbench-b', sessionType: 53, ts: 3000, peerNick: '工作台通知', _uid: 'u1' },
    { sessionId: 'workbench-c', sessionType: 53, ts: 2000, peerNick: '工作台通知', _uid: 'u1' },
    { sessionId: 'workbench-d', sessionType: 53, ts: 1500, peerNick: '工作台通知', _uid: 'u2' },
    { sessionId: 'chat-x', sessionType: 1, ts: 500, _uid: 'u1' },
  ]

  const sections = splitSessionSections(sessions)

  // u1's three sessionType=53 entries should be collapsed to one — the most
  // recent (workbench-b, ts=3000). u2 keeps its own. chat stays in chats.
  assert.deepEqual(
    sections.notificationSessions.map((session) => session.sessionId),
    ['workbench-b', 'workbench-d'],
  )
  assert.deepEqual(
    sections.chatSessions.map((session) => session.sessionId),
    ['chat-x'],
  )
})

// Reproduce the exact DB shape we observed: per uid, 闲鱼 returns 3 type-53
// sessions for senders 2222215948242 / 2222216136891 / 2222229870221, all
// carrying the same "【重要通知】宝贝违规处罚" payload. Phone shows ONE;
// we previously rendered three. After the fix, only the most recent stays.
it('splitSessionSections dedupes 工作台通知 fan-out matching real upstream layout', () => {
  const sessions = [
    {
      sessionId: '60438068290',
      sessionType: 53,
      _uid: '90000000001',
      ts: 1778545189502,
      peerNick: '工作台通知',
      peerUid: '2222229870221',
      summary: '【重要通知】宝贝违规处罚',
    },
    {
      sessionId: '60649816385',
      sessionType: 53,
      _uid: '90000000001',
      ts: 1778545189543,
      peerNick: '工作台通知',
      peerUid: '2222216136891',
      summary: '【重要通知】宝贝违规处罚',
    },
    {
      sessionId: '60334614175',
      sessionType: 53,
      _uid: '90000000001',
      ts: 1778545189502,
      peerNick: '工作台通知',
      peerUid: '2222215948242',
      summary: '【重要通知】宝贝违规处罚',
    },
  ]

  const sections = splitSessionSections(sessions)

  assert.equal(sections.notificationSessions.length, 1)
  // Sort within the notification section is ts-descending, so the kept entry
  // is the one with the largest ts (60649816385, ts=1778545189543).
  assert.equal(sections.notificationSessions[0].sessionId, '60649816385')
})
