import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ItemPickerModal } from './ItemPickerModal'
import { getItems, searchSendItems } from '../../api/index'

vi.mock('../../api/index', () => ({
  getItems: vi.fn(),
  searchSendItems: vi.fn(),
}))

const getItemsMock = vi.mocked(getItems)
const searchSendItemsMock = vi.mocked(searchSendItems)

describe('ItemPickerModal', () => {
  afterEach(() => {
    vi.clearAllMocks()
  })

  it('loads own items from local cache without calling IM item search', async () => {
    getItemsMock.mockResolvedValue({
      items: [
        {
          itemId: 'item-1',
          title: '本地商品',
          price: '88',
          picUrl: '',
        },
      ],
    } as never)

    render(
      <ItemPickerModal
        open
        uid="uid-1"
        sessionId="session-1"
        onClose={() => {}}
        onPick={() => {}}
      />,
    )

    await waitFor(() => expect(getItemsMock).toHaveBeenCalledWith('uid-1', 1, 30, 'ON_SALE', {
      force: false,
      requestOptions: { timeoutMs: 6000, timeoutMessage: '商品加载超时，请重试' },
    }))
    expect(searchSendItemsMock).not.toHaveBeenCalled()
    expect(await screen.findByText('本地商品')).toBeTruthy()
  })

  it('syncs own items once when local cache has never been populated', async () => {
    getItemsMock
      .mockResolvedValueOnce({ data: { itemList: [] }, _last_synced_at: 0 } as never)
      .mockResolvedValueOnce({
        data: {
          itemList: [
            {
              item_id: 'item-2',
              title: '同步商品',
              price: '66',
              pic_url: '',
            },
          ],
        },
        _last_synced_at: 1779550000,
      } as never)

    render(
      <ItemPickerModal
        open
        uid="uid-1"
        sessionId="session-1"
        onClose={() => {}}
        onPick={() => {}}
      />,
    )

    await waitFor(() => expect(getItemsMock).toHaveBeenCalledTimes(2))
    expect(getItemsMock).toHaveBeenNthCalledWith(2, 'uid-1', 1, 30, 'ON_SALE', {
      force: true,
      requestOptions: { timeoutMs: 8000, timeoutMessage: '商品同步超时，请重试' },
    })
    expect(await screen.findByText('同步商品')).toBeTruthy()
  })

  it('calls IM item search only after switching to peer items', async () => {
    getItemsMock.mockResolvedValue({ items: [] } as never)
    searchSendItemsMock.mockResolvedValue({
      items: [
        {
          itemId: 'peer-item-1',
          title: 'TA的商品',
          mainPic: '',
          price: '99',
        },
      ],
    } as never)

    render(
      <ItemPickerModal
        open
        uid="uid-1"
        sessionId="session-1"
        onClose={() => {}}
        onPick={() => {}}
      />,
    )

    await waitFor(() => expect(getItemsMock).toHaveBeenCalled())
    expect(searchSendItemsMock).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'TA的宝贝' }))

    await waitFor(() => {
      expect(searchSendItemsMock).toHaveBeenCalledWith(
        'uid-1',
        'session-1',
        'other',
        '',
        1,
        20,
        { timeoutMs: 8000, timeoutMessage: '商品搜索超时，请重试' },
      )
    })
    expect(await screen.findByText('TA的商品')).toBeTruthy()
  })
})
