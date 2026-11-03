import { describe, expect, it } from 'vitest'

import { normalizeBatchPublishTitle } from './BatchListModal'

describe('normalizeBatchPublishTitle', () => {
  it('trims batch publish titles to xianyu 30-char limit', () => {
    expect(
      normalizeBatchPublishTitle('这是一个明显超过三十个汉字限制的闲鱼商品标题需要在批量上架前自动截断'),
    ).toBe('这是一个明显超过三十个汉字限制的闲鱼商品标题需要在批量上架前')
  })

  it('keeps short titles unchanged', () => {
    expect(normalizeBatchPublishTitle('Mac mini M2 8G')).toBe('Mac mini M2 8G')
  })
})
