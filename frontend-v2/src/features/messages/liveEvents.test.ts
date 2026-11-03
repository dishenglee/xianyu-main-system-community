// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import {
  applyConnectionEvent,
  applyLiveEventToHistory,
  applyLiveEventToSessions,
  buildOutgoingMessageEvent,
} from './liveEvents'

it('applyLiveEventToSessions keeps notifications on top and increments unread for incoming chat', () => {
  const sessions = [
    { sessionId: 'chat-1', sessionType: 1, ts: 10, unread: 0, peerNick: '买家A' },
    { sessionId: 'notify-1', sessionType: 3, ts: 20, unread: 0, summary: '系统通知' },
  ]

  const next = applyLiveEventToSessions(sessions, {
    type: 'message.received',
    uid: 'u1',
    sessionId: 'chat-1',
    sessionType: 1,
    ts: 99,
    text: '新的买家消息',
    peerNick: '买家A',
  })

  assert.deepEqual(next.map((session) => session.sessionId), ['notify-1', 'chat-1'])
  assert.equal(next[1].unread, 1)
  assert.equal(next[1].summary, '新的买家消息')
  assert.equal(next[1].ts, 99)
})

it('applyLiveEventToSessions creates new chats with sender fallback metadata and sorts newest first', () => {
  const sessions = [
    { sessionId: 'chat-1', sessionType: 1, ts: 1774233435000, unread: 0, peerNick: '旧买家' },
    { sessionId: 'notify-1', sessionType: 3, ts: 1775356390283, unread: 0, summary: '系统通知' },
  ]

  const next = applyLiveEventToSessions(sessions, {
    type: 'message.received',
    uid: 'u1',
    sessionId: 'chat-2',
    sessionType: 1,
    ts: 1775384407000,
    text: '新的买家消息',
    senderUid: 'buyer-2',
    senderNick: '买家B',
    senderLogo: 'https://img.example.com/buyer-b.png',
  })

  assert.deepEqual(next.map((session) => session.sessionId), ['notify-1', 'chat-2', 'chat-1'])
  assert.equal(next[1].peerUid, 'buyer-2')
  assert.equal(next[1].peerNick, '买家B')
  assert.equal(next[1].peerLogo, 'https://img.example.com/buyer-b.png')
  assert.equal(next[1].summary, '新的买家消息')
  assert.equal(next[1].unread, 1)
})

it('applyLiveEventToSessions sorts live chats by actual timestamp instead of oversized api sortIndex', () => {
  const sessions = [
    {
      sessionId: 'chat-old',
      sessionType: 1,
      ts: 1774257346000,
      sortIndex: 20260323171546,
      unread: 0,
      peerNick: '旧买家',
    },
    {
      sessionId: 'notify-1',
      sessionType: 3,
      ts: 1775488939471,
      sortIndex: 20260406232219,
      unread: 0,
      summary: '系统通知',
    },
  ]

  const next = applyLiveEventToSessions(sessions, {
    type: 'message.received',
    uid: 'u1',
    sessionId: 'chat-new',
    sessionType: 1,
    ts: 1775527000000,
    text: '刚收到的新咨询',
    senderUid: 'buyer-new',
    senderNick: '新买家',
  })

  assert.deepEqual(next.map((session) => session.sessionId), ['notify-1', 'chat-new', 'chat-old'])
  assert.equal(next[1].peerNick, '新买家')
  assert.equal(next[1].summary, '刚收到的新咨询')
})

it('applyLiveEventToSessions replaces placeholder metadata with live sender fields', () => {
  const sessions = [
    {
      sessionId: 'chat-1',
      sessionType: 1,
      ts: 1775526000000,
      sortIndex: 1775526000000,
      unread: 0,
      peerUid: '',
      peerNick: '未知',
      peerLogo: '',
      itemId: 'auto_1775526000000',
      itemTitle: '闲鱼商品会话',
      itemPic: '',
    },
  ]

  const next = applyLiveEventToSessions(sessions, {
    type: 'message.received',
    uid: 'u1',
    sessionId: 'chat-1',
    sessionType: 1,
    ts: 1775527000000,
    text: '刚收到的新咨询',
    senderUid: 'buyer-new',
    senderNick: '新买家',
    senderLogo: 'https://img.example.com/buyer-new.png',
    itemId: '909812376400',
    itemTitle: '自考地理专业论文辅导',
    itemPic: 'https://img.example.com/item.jpg',
  })

  assert.equal(next[0].peerUid, 'buyer-new')
  assert.equal(next[0].peerNick, '新买家')
  assert.equal(next[0].peerLogo, 'https://img.example.com/buyer-new.png')
  assert.equal(next[0].itemId, '909812376400')
  assert.equal(next[0].itemTitle, '自考地理专业论文辅导')
  assert.equal(next[0].itemPic, 'https://img.example.com/item.jpg')
})

it('applyLiveEventToHistory appends live messages and dedupes by message id', () => {
  const session = { sessionId: 'chat-1', sessionType: 1, peerNick: '买家A', peerUid: 'peer-1' }
  const history = {
    sessionId: 'chat-1',
    sessionType: 1,
    hasMore: true,
    nextCursor: 1,
    latestCursor: 10,
    messages: [
      { messageId: 'm-1', version: 10, ts: 10, text: '旧消息', preview: '旧消息', isSelf: false },
    ],
  }

  const event = {
    type: 'message.received',
    uid: 'u1',
    sessionId: 'chat-1',
    sessionType: 1,
    messageId: 'm-2',
    version: 20,
    ts: 20,
    text: '新消息',
    peerNick: '买家A',
    peerUid: 'peer-1',
  }

  const merged = applyLiveEventToHistory(history, session, event)
  assert.equal(merged.messages.length, 2)
  assert.equal(merged.messages[1].messageId, 'm-2')
  assert.equal(merged.latestCursor, 20)

  const deduped = applyLiveEventToHistory(merged, session, event)
  assert.equal(deduped.messages.length, 2)
})

it('applyLiveEventToHistory uses sender fallback metadata for new incoming chat records', () => {
  const merged = applyLiveEventToHistory({}, { sessionId: 'chat-2', sessionType: 1 }, {
    type: 'message.received',
    uid: 'u1',
    sessionId: 'chat-2',
    sessionType: 1,
    messageId: 'm-9',
    version: 20,
    ts: 1775384407000,
    text: '你好',
    senderUid: 'buyer-9',
    senderNick: '买家九号',
    senderLogo: 'https://img.example.com/buyer-9.png',
  })

  assert.equal(merged.peerUid, 'buyer-9')
  assert.equal(merged.peerNick, '买家九号')
  assert.equal(merged.peerLogo, 'https://img.example.com/buyer-9.png')
  assert.equal(merged.messages[0].text, '你好')
})

it('buildOutgoingMessageEvent reuses backend meta so local optimistic message can dedupe with SSE', () => {
  const event = buildOutgoingMessageEvent({
    uid: 'u1',
    session: {
      sessionId: '60169492429',
      sessionType: 1,
      itemId: '',
      itemTitle: '',
      itemPic: '',
    },
    history: {
      itemId: '1039221387850',
      itemTitle: '文章写作',
      itemPic: 'https://img.example.com/item.jpg',
    },
    content: '可以，发我要求',
    response: {
      ret: ['SUCCESS::调用成功'],
      data: {
        messageId: 'http-sent-1',
        ts: 1775530000123,
        itemId: '1039221387850',
      },
    },
  })

  assert.equal(event.type, 'message.sent')
  assert.equal(event.uid, 'u1')
  assert.equal(event.sessionId, '60169492429')
  assert.equal(event.messageId, 'http-sent-1')
  assert.equal(event.ts, 1775530000123)
  assert.equal(event.version, 1775530000123)
  assert.equal(event.text, '可以，发我要求')
  assert.equal(event.itemId, '1039221387850')
  assert.equal(event.itemTitle, '文章写作')
  assert.equal(event.itemPic, 'https://img.example.com/item.jpg')
  assert.equal(event.isSelf, true)
})

it('applyConnectionEvent stores latest status per uid', () => {
  const next = applyConnectionEvent({}, {
    type: 'connection.connected',
    uid: 'u1',
    reason: '',
    snapshot: true,
  })

  assert.equal(next.u1.status, 'connected')
  assert.equal(next.u1.snapshot, true)
})

it('applyConnectionEvent accepts auth failures and preserves backend update time', () => {
  const next = applyConnectionEvent({}, {
    type: 'connection.auth_failed',
    uid: 'u2',
    reason: 'risk control',
    updatedAt: 1775530000123,
  })

  assert.equal(next.u2.status, 'auth_failed')
  assert.equal(next.u2.reason, 'risk control')
  assert.equal(next.u2.updatedAt, 1775530000123)
})
