// @ts-nocheck
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { apiMocks, notifyMock } = vi.hoisted(() => ({
  apiMocks: {
    itemEditDetail: vi.fn(),
    itemProperties: vi.fn(),
    itemEdit: vi.fn(),
    itemOpen: vi.fn(),
  },
  notifyMock: vi.fn(),
}))

vi.mock('../../api/index', () => apiMocks)
vi.mock('../../components/Notify', () => ({
  useSafeNotify: () => notifyMock,
}))

import { EditItemModal } from './EditItemModal'

describe('EditItemModal', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    apiMocks.itemEditDetail.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        itemTextDTO: { title: '测试商品', desc: '测试描述' },
        itemPriceDTO: { priceInCent: '8800', origPriceInCent: '9900', noBargain: false },
        itemCatDTO: { tbCatId: '50018264', channelCatId: '201460608', leafId: '50018264' },
        imageInfoDOList: [{ url: 'https://img.alicdn.com/test.jpg' }],
        quantity: '1',
      },
    })
    apiMocks.itemProperties.mockResolvedValue({ data: { cards: [] } })
    apiMocks.itemEdit.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: {} })
    apiMocks.itemOpen.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: {} })
  })

  it('shows a success toast after item edit is saved', async () => {
    const onSaved = vi.fn().mockResolvedValue(undefined)

    render(
      <EditItemModal
        uid="90000000002"
        item={{ itemId: 'ITEM001', title: '测试商品', price: '88.00' }}
        onClose={vi.fn()}
        onSaved={onSaved}
      />,
    )

    await screen.findByDisplayValue('测试商品')
    fireEvent.click(screen.getByRole('button', { name: '保存' }))

    await waitFor(() => {
      expect(apiMocks.itemEdit).toHaveBeenCalledWith(
        '90000000002',
        'ITEM001',
        expect.objectContaining({
          title: '测试商品',
          price: '88',
        }),
      )
    })
    expect(notifyMock).toHaveBeenCalledWith('商品保存成功', 'success')
    expect(onSaved).toHaveBeenCalled()
  })
})
