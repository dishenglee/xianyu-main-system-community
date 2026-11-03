// @ts-nocheck
import React from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSelectedSessionResolvers } from './useSelectedSessionResolvers'

const apiMocks = vi.hoisted(() => ({
  itemDetail: vi.fn(),
  pcMessageHeadinfo: vi.fn(),
  pcSessionInfo: vi.fn(),
}))

vi.mock('../../../api/index', () => ({
  itemDetail: apiMocks.itemDetail,
  pcMessageHeadinfo: apiMocks.pcMessageHeadinfo,
  pcSessionInfo: apiMocks.pcSessionInfo,
}))

describe('useSelectedSessionResolvers', () => {
  beforeEach(() => {
    apiMocks.itemDetail.mockReset()
    apiMocks.pcMessageHeadinfo.mockReset()
    apiMocks.pcSessionInfo.mockReset()
  })

  it('hydrates missing item fields before fetching detail metadata for a selected chat session', async () => {
    const setItemPriceByItemId = vi.fn()
    const setHeadinfoBySessionId = vi.fn()

    apiMocks.pcSessionInfo.mockResolvedValue({
      sessionId: 'sid-1',
      sessionType: 1,
      itemId: 'item-9',
      itemTitle: '补水后的商品',
      itemPic: 'https://img.example.com/item-9.png',
      peerUid: 'peer-7',
      peerNick: '买家A',
      peerRole: 'buyer',
      selfRole: 'seller',
    })
    apiMocks.pcMessageHeadinfo.mockResolvedValue({
      headInfo: {
        title: '99',
        promotionPrice: '88',
        semanticsDesc: '补水后的商品',
        subTitle: '同城',
        tips: '可小刀',
        picUrl: 'https://img.example.com/headinfo.png',
      },
    })
    apiMocks.itemDetail.mockResolvedValue({ price: '99' })

    const { result } = renderHook(() => {
      const [selectedSession, setSelectedSession] = React.useState({
        _uid: 'uid-1',
        sessionId: 'sid-1',
        sessionType: 1,
        itemId: '',
        itemTitle: '',
        itemPic: '',
        peerNick: '未知',
      })
      const selectedItem = selectedSession.itemId
        ? { itemId: selectedSession.itemId }
        : null

      useSelectedSessionResolvers({
        selectedSession,
        selectedItem,
        itemPriceByItemId: {},
        setItemPriceByItemId,
        headinfoBySessionId: {},
        setHeadinfoBySessionId,
        updateSelectedSession: (patch) => {
          setSelectedSession((prev) => ({ ...prev, ...patch }))
        },
      })

      return selectedSession
    })

    await waitFor(() => {
      expect(apiMocks.pcSessionInfo).toHaveBeenCalledWith('uid-1', 'sid-1', 1)
    })

    await waitFor(() => {
      expect(result.current.itemId).toBe('item-9')
      expect(result.current.itemTitle).toBe('补水后的商品')
      expect(result.current.itemPic).toBe('https://img.example.com/item-9.png')
    })

    await waitFor(() => {
      expect(apiMocks.pcMessageHeadinfo).toHaveBeenCalledWith('uid-1', 'sid-1', 'item-9', 'peer-7')
    })

    await waitFor(() => {
      expect(apiMocks.itemDetail).toHaveBeenCalledWith('uid-1', 'item-9')
    })
  })

  it('retries session hydration while the selected session still lacks item identity', async () => {
    const setItemPriceByItemId = vi.fn()
    const setHeadinfoBySessionId = vi.fn()
    const completeSession = {
      sessionId: 'sid-retry',
      sessionType: 1,
      itemId: 'item-retry',
      itemTitle: '第二次补齐的商品',
      itemPic: 'https://img.example.com/item-retry.png',
      peerUid: 'peer-retry',
      peerNick: '买家B',
      peerRole: 'buyer',
      selfRole: 'seller',
    }

    apiMocks.pcSessionInfo
      .mockResolvedValueOnce({
        sessionId: 'sid-retry',
        sessionType: 1,
        itemId: '',
        itemTitle: '',
        itemPic: '',
        peerUid: 'peer-retry',
        peerNick: '买家B',
        peerRole: '',
        selfRole: '',
      })
      .mockResolvedValue(completeSession)
    apiMocks.pcMessageHeadinfo.mockResolvedValue({ headInfo: null })
    apiMocks.itemDetail.mockResolvedValue({ price: '66' })

    const { result } = renderHook(
      ({ marker }) => {
        const [selectedSession, setSelectedSession] = React.useState({
          _uid: 'uid-retry',
          sessionId: 'sid-retry',
          sessionType: 1,
          itemId: '',
          itemTitle: '',
          itemPic: '',
          peerUid: '',
          peerNick: '未知',
          peerRole: '',
          selfRole: '',
          marker,
        })
        const selectedItem = selectedSession.itemId
          ? { itemId: selectedSession.itemId }
          : null

        useSelectedSessionResolvers({
          selectedSession,
          selectedItem,
          itemPriceByItemId: {},
          setItemPriceByItemId,
          headinfoBySessionId: {},
          setHeadinfoBySessionId,
          updateSelectedSession: (patch) => {
            setSelectedSession((prev) => ({ ...prev, ...patch }))
          },
        })

        return selectedSession
      },
      {
        initialProps: { marker: 1 },
      },
    )

    await waitFor(() => {
      expect(apiMocks.pcSessionInfo).toHaveBeenCalledTimes(1)
    })

    await waitFor(() => {
      expect(apiMocks.pcSessionInfo.mock.calls.length).toBeGreaterThanOrEqual(2)
    })

    await waitFor(() => {
      expect(result.current.itemId).toBe(completeSession.itemId)
      expect(result.current.itemTitle).toBe(completeSession.itemTitle)
      expect(result.current.itemPic).toBe(completeSession.itemPic)
    })
  })
})
