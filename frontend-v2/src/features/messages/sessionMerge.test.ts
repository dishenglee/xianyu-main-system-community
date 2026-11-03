// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { mergeSessionLists } from './sessionMerge'

it('mergeSessionLists keeps SSE-only sessions that are missing from API refresh', () => {
  const apiSessions = [
    { sessionId: 'api-1', sessionType: 1, ts: 100, summary: 'API 会话' },
  ]
  const existingSessions = [
    { sessionId: 'api-1', sessionType: 1, ts: 100, summary: 'API 会话' },
    {
      sessionId: 'live-1',
      sessionType: 1,
      ts: 200,
      sortIndex: 200,
      summary: 'SSE 新会话',
      peerNick: '新买家',
      itemTitle: '新商品',
    },
  ]

  const merged = mergeSessionLists(apiSessions, existingSessions)

  assert.deepEqual(merged.map((session) => session.sessionId), ['live-1', 'api-1'])
  assert.equal(merged[0].peerNick, '新买家')
  assert.equal(merged[0].itemTitle, '新商品')
})

it('mergeSessionLists keeps newer SSE preview and richer metadata when API data is older or incomplete', () => {
  const apiSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 100,
      sortIndex: 100,
      summary: '旧摘要',
      unread: 0,
      peerNick: '',
      peerLogo: '',
      itemTitle: '',
    },
  ]
  const existingSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 200,
      sortIndex: 200,
      summary: '新摘要',
      unread: 3,
      peerNick: '测试用户甲',
      peerLogo: 'https://img.example.com/peer.jpg',
      itemTitle: '文章/软文写作服务',
    },
  ]

  const merged = mergeSessionLists(apiSessions, existingSessions)

  assert.equal(merged[0].summary, '新摘要')
  assert.equal(merged[0].ts, 200)
  assert.equal(merged[0].unread, 3)
  assert.equal(merged[0].peerNick, '测试用户甲')
  assert.equal(merged[0].peerLogo, 'https://img.example.com/peer.jpg')
  assert.equal(merged[0].itemTitle, '文章/软文写作服务')
})

it('mergeSessionLists keeps live preview but trusts api unread when api refresh has the same timestamp', () => {
  const apiSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 100,
      sortIndex: 100,
      summary: '旧摘要',
      unread: 0,
      peerNick: '买家A',
      peerLogo: '',
      itemTitle: '',
    },
  ]
  const existingSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 100,
      sortIndex: 100,
      summary: '刚收到的新消息',
      unread: 2,
      peerNick: '买家A',
      peerLogo: 'https://img.example.com/buyer.jpg',
      itemTitle: '最新商品',
    },
  ]

  const merged = mergeSessionLists(apiSessions, existingSessions)

  assert.equal(merged[0].summary, '刚收到的新消息')
  assert.equal(merged[0].unread, 0)
  assert.equal(merged[0].peerLogo, 'https://img.example.com/buyer.jpg')
  assert.equal(merged[0].itemTitle, '最新商品')
})

it('mergeSessionLists replaces placeholder unknown metadata with hydrated api values', () => {
  const apiSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 1775527000000,
      sortIndex: 20260407104321,
      summary: '您好，请问接单吗',
      unread: 1,
      peerUid: '2214634332689',
      peerNick: '煎饼狗子',
      peerLogo: 'https://img.example.com/buyer.jpg',
      itemId: '909812376400',
      itemTitle: '自考地理专业论文辅导',
      itemPic: 'https://img.example.com/item.jpg',
    },
  ]
  const existingSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 1775527000000,
      sortIndex: 1775527000000,
      summary: '您好，请问接单吗',
      unread: 1,
      peerUid: '',
      peerNick: '未知',
      peerLogo: '',
      itemId: 'auto_1775527000000',
      itemTitle: '闲鱼商品会话',
      itemPic: '',
    },
  ]

  const merged = mergeSessionLists(apiSessions, existingSessions)

  assert.equal(merged[0].peerUid, '2214634332689')
  assert.equal(merged[0].peerNick, '煎饼狗子')
  assert.equal(merged[0].peerLogo, 'https://img.example.com/buyer.jpg')
  assert.equal(merged[0].itemId, '909812376400')
  assert.equal(merged[0].itemTitle, '自考地理专业论文辅导')
  assert.equal(merged[0].itemPic, 'https://img.example.com/item.jpg')
})

it('mergeSessionLists lets wx hydrated fishNick replace weak placeholder names', () => {
  const apiSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 300,
      sortIndex: 300,
      peerNick: 'san丹530',
      summary: '露营车还在吗',
      itemId: '1042662837101',
      itemTitle: '折叠露营车',
      itemPic: 'https://img.example.com/camp-cart.jpg',
    },
  ]
  const placeholderNames = [
    '你有一条新消息',
    '您有一条新消息',
    '未知',
    '用户_65320912',
    'tbNick_65320912',
    '65320912',
  ]

  for (const peerNick of placeholderNames) {
    const merged = mergeSessionLists(apiSessions, [{
      sessionId: 's1',
      sessionType: 1,
      ts: 300,
      sortIndex: 300,
      peerNick,
      summary: '露营车还在吗',
      itemId: '',
      itemTitle: '',
      itemPic: '',
    }])

    assert.equal(merged[0].peerNick, 'san丹530')
    assert.equal(merged[0].itemId, '1042662837101')
    assert.equal(merged[0].itemTitle, '折叠露营车')
    assert.equal(merged[0].itemPic, 'https://img.example.com/camp-cart.jpg')
  }
})

it('mergeSessionLists fills item fields from fresh api data without losing existing live preview', () => {
  const apiSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 100,
      sortIndex: 100,
      summary: '旧摘要',
      peerNick: 'Chen',
      itemId: '1042662837101',
      itemTitle: '折叠露营车',
      itemPic: 'https://img.example.com/camp-cart.jpg',
    },
  ]
  const existingSessions = [
    {
      sessionId: 's1',
      sessionType: 1,
      ts: 200,
      sortIndex: 200,
      summary: '180',
      peerNick: 'Chen',
      itemId: '',
      itemTitle: '',
      itemPic: '',
    },
  ]

  const merged = mergeSessionLists(apiSessions, existingSessions)

  assert.equal(merged[0].summary, '180')
  assert.equal(merged[0].itemId, '1042662837101')
  assert.equal(merged[0].itemTitle, '折叠露营车')
  assert.equal(merged[0].itemPic, 'https://img.example.com/camp-cart.jpg')
})
