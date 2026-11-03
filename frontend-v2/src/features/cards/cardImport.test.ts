import { describe, expect, it } from 'vitest'

import { buildCardImportPayloads, countCardImportEntries } from './cardImport'

describe('card import payloads', () => {
  it('keeps reusable common card content as one multi-line card', () => {
    const payloads = buildCardImportPayloads({
      rawText: '第一行说明\n第二行兑换码\n第三行售后提示',
      cardPoolTag: ' common ',
      reusable: true,
      createdAt: 12345,
    })

    expect(payloads).toEqual([
      {
        cardKey: 'common-12345-1',
        cardContent: '第一行说明\n第二行兑换码\n第三行售后提示',
        cardPoolTag: 'common',
        reusable: true,
      },
    ])
    expect(countCardImportEntries('第一行\n第二行', true)).toBe(1)
  })

  it('splits one-time card content by non-empty lines', () => {
    const payloads = buildCardImportPayloads({
      rawText: 'A-001\n\n B-002 \nC-003',
      cardPoolTag: '',
      reusable: false,
      createdAt: 12345,
    })

    expect(payloads).toEqual([
      { cardKey: 'CARD-12345-1', cardContent: 'A-001', cardPoolTag: '', reusable: false },
      { cardKey: 'CARD-12345-2', cardContent: 'B-002', cardPoolTag: '', reusable: false },
      { cardKey: 'CARD-12345-3', cardContent: 'C-003', cardPoolTag: '', reusable: false },
    ])
    expect(countCardImportEntries('A-001\n\nB-002\n', false)).toBe(2)
  })
})
