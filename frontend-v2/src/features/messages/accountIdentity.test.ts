// @ts-nocheck
import { it } from 'vitest'
import assert from 'node:assert'

import { resolveAccountIdentity } from './accountIdentity'

it('resolveAccountIdentity falls back to session self profile when account profile is incomplete', () => {
  const result = resolveAccountIdentity(
    {
      uid: '2217178046893',
      nick: 'xy191774985939',
      avatar: '',
    },
    [
      {
        selfUid: '2217178046893',
        selfNick: '敲厉害的糯糯学姐',
        selfLogo: 'https://img.example.com/me.jpg',
      },
    ],
  )

  assert.deepEqual(result, {
    nick: '敲厉害的糯糯学姐',
    avatar: 'https://img.example.com/me.jpg',
  })
})

it('resolveAccountIdentity prefers IM self profile over stored account profile', () => {
  const result = resolveAccountIdentity(
    {
      uid: '90000000001',
      nick: '大碗宽面哎',
      avatar: 'https://img.example.com/account.jpg',
    },
    [
      {
        selfUid: '90000000001',
        selfNick: '测试用户甲',
        selfLogo: 'https://img.example.com/me.jpg',
      },
    ],
  )

  assert.deepEqual(result, {
    nick: '测试用户甲',
    avatar: 'https://img.example.com/me.jpg',
  })
})

it('resolveAccountIdentity falls back to stored account profile when no IM self profile exists', () => {
  const result = resolveAccountIdentity(
    {
      uid: '90000000002',
      nick: '测试用户乙',
      avatar: 'https://img.example.com/account.jpg',
    },
    [],
  )

  assert.deepEqual(result, {
    nick: '测试用户乙',
    avatar: 'https://img.example.com/account.jpg',
  })
})
