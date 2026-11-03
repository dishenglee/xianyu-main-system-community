// @ts-nocheck
import { describe, expect, it } from 'vitest'

import {
  inferSimpleItemFromCategory,
  inferSimpleItemFromContent,
  pickPreferredPublishAccount,
  resolveSimpleItemFlag,
} from './publishUtils'

describe('publishUtils simple-item inference', () => {
  it('treats service-like categories as virtual/service items', () => {
    expect(inferSimpleItemFromCategory({ catName: '软件开发' })).toBe(true)
    expect(inferSimpleItemFromCategory({ catName: '其他服务' })).toBe(true)
    expect(inferSimpleItemFromCategory({ catName: '系统安装升级' })).toBe(true)
  })

  it('treats physical categories as normal goods', () => {
    expect(inferSimpleItemFromCategory({ catName: '手机' })).toBe(false)
    expect(inferSimpleItemFromCategory({ catName: '电视机' })).toBe(false)
    expect(inferSimpleItemFromCategory({ catName: '机械键盘' })).toBe(false)
  })

  it('treats digital-delivery descriptions as virtual/service items', () => {
    expect(inferSimpleItemFromContent('软件安装包+激活码 秒发')).toBe(true)
    expect(inferSimpleItemFromContent('正版会员代充', '提供兑换码和授权码')).toBe(true)
    expect(inferSimpleItemFromContent('小米手持风扇 40h 长续航')).toBe(false)
  })

  it('prefers explicit upstream flag over category inference', () => {
    expect(resolveSimpleItemFlag(true, { catName: '手机' })).toBe(true)
    expect(resolveSimpleItemFlag(false, { catName: '软件开发' })).toBe(false)
  })

  it('falls back to title and description when category is not service-like', () => {
    expect(resolveSimpleItemFlag(null, { catName: '游戏装备' }, {
      title: 'Windows 软件安装包',
      desc: '含序列号和激活码，远程安装',
    })).toBe(true)
    expect(resolveSimpleItemFlag(null, { catName: '手机' }, {
      title: '小米 17 Max 5G 手机',
      desc: '国补现货，白色 16G+512G',
    })).toBe(false)
  })
})

describe('pickPreferredPublishAccount', () => {
  it('does not select delete-only accounts for publishing', () => {
    expect(pickPreferredPublishAccount([
      { uid: 'delete-only', status: 'active', deleteOnly: true, last_session_status: 'ok', last_session_ok_at: 999 },
      { uid: 'usable', status: 'active', last_session_status: 'ok', last_session_ok_at: 1 },
    ])?.uid).toBe('usable')

    expect(pickPreferredPublishAccount([
      { uid: 'delete-only', status: 'active', delete_only: true, last_session_status: 'ok' },
    ])).toBeNull()
  })

  it('keeps accounts whose delete-only flags are serialized false strings', () => {
    expect(pickPreferredPublishAccount([
      { uid: 'usable', status: 'active', deleteOnly: 'false', delete_only: '0', last_session_status: 'ok' },
    ])?.uid).toBe('usable')
  })
})
