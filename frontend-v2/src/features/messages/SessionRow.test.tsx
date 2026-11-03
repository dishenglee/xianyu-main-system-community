// @ts-nocheck
import { render, screen } from '@testing-library/react'
import { it } from 'vitest'
import assert from 'node:assert'

import { SessionRow } from './SessionRow'

const baseSession = {
  _uid: '90000000001',
  _accountNick: '测试用户甲',
  sessionId: '61826145707',
  sessionType: 1,
  peerNick: 'Chen',
  summary: '180',
  unread: 0,
  ts: 1779323926590,
}

it('SessionRow renders compact item head with title and image', () => {
  const { container } = render(
    <SessionRow
      session={{
        ...baseSession,
        itemId: '1042662837101',
        itemTitle: '折叠露营车',
        itemPic: 'https://img.example.com/camp-cart.jpg',
      }}
      active={false}
      onSelect={() => {}}
    />,
  )

  assert.ok(screen.getByLabelText('折叠露营车'))
  const itemImage = container.querySelector('img[src="https://img.example.com/camp-cart.jpg"]')
  assert.ok(itemImage)
})

it('SessionRow falls back to itemId when itemTitle is absent', () => {
  render(
    <SessionRow
      session={{
        ...baseSession,
        itemId: '1042662837101',
        itemTitle: '',
        itemPic: '',
      }}
      active={false}
      onSelect={() => {}}
    />,
  )

  assert.ok(screen.getByText('1042662837101'))
})

it('SessionRow memo updates when item fields change', () => {
  const noop = () => {}
  const { rerender, container } = render(
    <SessionRow
      session={{
        ...baseSession,
        itemId: 'old-item',
        itemTitle: '旧商品',
        itemPic: 'https://img.example.com/old.jpg',
      }}
      active={false}
      onSelect={noop}
    />,
  )

  rerender(
    <SessionRow
      session={{
        ...baseSession,
        itemId: 'new-item',
        itemTitle: '新商品',
        itemPic: 'https://img.example.com/new.jpg',
      }}
      active={false}
      onSelect={noop}
    />,
  )

  assert.ok(screen.getByLabelText('新商品'))
  assert.equal(screen.queryByLabelText('旧商品'), null)
  assert.ok(container.querySelector('img[src="https://img.example.com/new.jpg"]'))
})

it('SessionRow hides unread badge for notification sessions that disable badges', () => {
  render(
    <SessionRow
      session={{
        ...baseSession,
        sessionType: 3,
        peerNick: '系统消息',
        unread: 2,
      }}
      active={false}
      onSelect={() => {}}
    />,
  )

  assert.equal(screen.queryByText(/^2$/), null)
})

it('SessionRow renders Xianyu emoji shortcode previews as custom images', () => {
  render(
    <SessionRow
      session={{
        ...baseSession,
        summary: '[真棒] 可以',
      }}
      active={false}
      onSelect={() => {}}
    />,
  )

  assert.ok(screen.getByAltText('[真棒]'))
})
