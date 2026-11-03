import { describe, expect, it } from 'vitest'
import {
  applyLiveEventSnapshot,
  pruneSnapshotToUids,
  reconcilePolledSessions,
} from './globalMessageNotifyPolling'

describe('globalMessageNotifyPolling', () => {
  it('suppresses first-poll historical unread and notifies on later unread growth', () => {
    const first = reconcilePolledSessions({}, 'uid-1', [{
      sessionId: 'session-1',
      sessionType: 1,
      unread: 2,
      version: 10,
      ts: 100,
      peerNick: '买家A',
      summary: '历史未读',
    }], { notifyOnNewKeys: false })

    expect(first.notifications).toHaveLength(0)

    const second = reconcilePolledSessions(first.snapshot, 'uid-1', [{
      sessionId: 'session-1',
      sessionType: 1,
      unread: 3,
      version: 11,
      ts: 200,
      peerNick: '买家A',
      summary: '新的未读',
    }], { notifyOnNewKeys: true })

    expect(second.notifications).toHaveLength(1)
    expect(second.notifications[0]).toMatchObject({
      uid: 'uid-1',
      sessionId: 'session-1',
      unread: 3,
      version: 11,
      preview: '新的未读',
    })
  })

  it('updates snapshot from live message events to avoid duplicate poll notifications', () => {
    const next = applyLiveEventSnapshot({}, {
      uid: 'uid-2',
      sessionId: 'session-9',
      sessionType: 1,
      unreadCount: 4,
      version: 22,
      ts: 300,
      peerNick: '买家B',
      rawPreview: '实时新消息',
    })

    expect(next['uid-2:session-9:1']).toMatchObject({
      unread: 4,
      version: 22,
      peerNick: '买家B',
      preview: '实时新消息',
    })
  })

  it('prunes removed accounts from the cached snapshot', () => {
    const pruned = pruneSnapshotToUids({
      'uid-1:session-a:1': {
        key: 'uid-1:session-a:1',
        uid: 'uid-1',
        sessionId: 'session-a',
        sessionType: 1,
        unread: 1,
        version: 1,
        ts: 1,
        peerNick: 'A',
        preview: 'A',
      },
      'uid-2:session-b:1': {
        key: 'uid-2:session-b:1',
        uid: 'uid-2',
        sessionId: 'session-b',
        sessionType: 1,
        unread: 1,
        version: 1,
        ts: 1,
        peerNick: 'B',
        preview: 'B',
      },
    }, ['uid-2'])

    expect(Object.keys(pruned)).toEqual(['uid-2:session-b:1'])
  })
})
