import { describe, expect, it } from 'vitest'

import {
  buildKeywordPayload,
  findExistingKeywordByTemplate,
  keywordTemplateKey,
  resolveKeywordTargetUids,
  selectSavedKeywordTemplates,
} from './keywordTargets'

describe('keyword targets', () => {
  it('uses the multi-selected account list as keyword write targets', () => {
    expect(resolveKeywordTargetUids(['seller-a', 'seller-b'])).toEqual(['seller-a', 'seller-b'])
    expect(resolveKeywordTargetUids(['seller-a', 'seller-a', '', ' seller-b '])).toEqual(['seller-a', 'seller-b'])
  })

  it('selects only enabled saved keyword templates by id', () => {
    const templates = [
      { id: 1, keyword: '发货', reply: '马上发', type: 'text', enabled: true },
      { id: 2, keyword: '图片', reply: 'https://example.com/a.png', type: 'image', enabled: true },
      { id: 3, keyword: '关闭', reply: '不会应用', type: 'text', enabled: false },
      { id: 4, keyword: '', reply: '空关键词', type: 'text', enabled: true },
    ]

    expect(selectSavedKeywordTemplates(templates, [1, 2, 3, 4]).map((template) => template.id)).toEqual([1, 2])
  })

  it('builds idempotent keyword payloads and matches existing account rules by business key', () => {
    const template = { keyword: ' 发货 ', reply: '马上发', type: 'text', itemId: ' item-1 ', enabled: true }
    const existing = [
      { id: 10, keyword: '发货', reply: '旧回复', type: 'text', itemId: 'item-1', enabled: true },
      { id: 11, keyword: '发货', reply: '其他商品', type: 'text', itemId: 'item-2', enabled: true },
    ]

    expect(buildKeywordPayload(template)).toEqual({
      keyword: '发货',
      reply: '马上发',
      type: 'text',
      itemId: 'item-1',
      enabled: true,
    })
    expect(keywordTemplateKey(template)).toBe(keywordTemplateKey(existing[0]))
    expect(findExistingKeywordByTemplate(existing, template)?.id).toBe(10)
  })
})
