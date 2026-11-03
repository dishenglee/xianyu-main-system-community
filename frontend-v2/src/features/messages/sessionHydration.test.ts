// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import {
  buildSessionIdentityPatch,
  collectAccountsNeedingRemoteSync,
  needsSessionHydration,
} from './sessionHydration'

it('needsSessionHydration returns true when chat session is missing inferred roles', () => {
  assert.equal(needsSessionHydration({
    sessionId: '60169492429',
    sessionType: 1,
    peerNick: '未知',
    itemTitle: '论文辅导',
    itemPic: 'https://img.example/item.png',
    peerRole: '',
    selfRole: '',
  }), true)
})

it('needsSessionHydration returns false for notification sessions', () => {
  assert.equal(needsSessionHydration({
    sessionId: 'notify-3',
    sessionType: 3,
    peerNick: '系统通知',
  }), false)
})

it('buildSessionIdentityPatch keeps inferred roles from session info', () => {
  assert.deepEqual(buildSessionIdentityPatch({
    sessionId: '60169492430',
    sessionType: 1,
    peerUid: '2202577548039',
    peerNick: '未知',
    peerRole: 'seller',
    selfRole: 'buyer',
    itemTitle: '数据库服务',
  }, {
    sessionId: '60169492430',
    sessionType: 1,
    peerNick: '',
    itemTitle: '',
  }), {
    sessionId: '60169492430',
    sessionType: 1,
    peerUid: '2202577548039',
    peerNick: '',
    peerLogo: '',
    itemId: '',
    itemTitle: '数据库服务',
    itemPic: '',
    peerRole: 'seller',
    selfRole: 'buyer',
    summary: '',
  })
})

it('collectAccountsNeedingRemoteSync only includes accounts with stale chat sessions', () => {
  const uids = collectAccountsNeedingRemoteSync(
    [{ uid: '1001' }, { uid: '1002' }, { uid: '1003' }],
    {
      1001: [{
        sessionId: 'chat-1',
        sessionType: 1,
        peerNick: '未知',
        itemTitle: '论文辅导',
        itemPic: 'https://img.example/item.png',
      }],
      1002: [{
        sessionId: 'chat-2',
        sessionType: 1,
        peerNick: '鲨鱼辣椒',
        itemId: '684522991901',
        itemTitle: '数据库服务',
        itemPic: 'https://img.example/item.png',
        peerRole: 'buyer',
        selfRole: 'seller',
      }],
      1003: [{
        sessionId: 'notify-1',
        sessionType: 3,
        peerNick: '系统通知',
      }],
    },
  )

  assert.deepEqual(uids, ['1001'])
})
