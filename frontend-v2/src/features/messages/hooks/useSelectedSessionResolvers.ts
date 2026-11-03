// @ts-nocheck
// Lazy resolvers for the currently-selected session:
//   * sessionInfo — fills missing peer/item identity for restored sessions
//     before any item/headinfo fetch can proceed.
//   * itemPrice — only on the session record as `itemId`; price comes from
//     /api/item/detail. Cached by itemId so repeated selects don't re-fetch.
//   * headInfo — promotion price / picUrl / desc / 价格 title for sessionType=1.
// Both only fire on the desktop where the conversation panel renders. Mobile's
// MobileChat self-resolves these in its own component because it lives at a
// separate route.
import { useEffect, useRef } from 'react'

import { itemDetail, pcMessageHeadinfo, pcSessionInfo } from '../../../api/index'
import { sessionType } from '../messagesConstants'
import { buildSessionIdentityPatch, needsSessionHydration } from '../sessionHydration'

interface ResolverOptions {
  selectedSession: any
  selectedItem: any
  itemPriceByItemId: Record<string, string>
  setItemPriceByItemId: (
    updater: (prev: Record<string, string>) => Record<string, string>,
  ) => void
  headinfoBySessionId: Record<string, any>
  setHeadinfoBySessionId: (
    updater: (prev: Record<string, any>) => Record<string, any>,
  ) => void
  updateSelectedSession?: (patch: any) => void
}

export function useSelectedSessionResolvers({
  selectedSession,
  selectedItem,
  itemPriceByItemId,
  setItemPriceByItemId,
  headinfoBySessionId,
  setHeadinfoBySessionId,
  updateSelectedSession,
}: ResolverOptions) {
  const hydrationAttemptCountRef = useRef(new Map<string, number>())

  useEffect(() => {
    const uid = selectedSession?._uid
    const sid = String(selectedSession?.sessionId ?? '').trim()
    const st = sessionType(selectedSession)
    if (!uid || !sid) return
    if (!needsSessionHydration(selectedSession)) return

    const hydrationKey = `${uid}:${sid}:${st}`
    const attemptCount = hydrationAttemptCountRef.current.get(hydrationKey) || 0
    if (attemptCount >= 3) return
    hydrationAttemptCountRef.current.set(hydrationKey, attemptCount + 1)

    pcSessionInfo(uid, sid, st)
      .then((remoteSession: any) => {
        const patch = buildSessionIdentityPatch(remoteSession, selectedSession)
        if (!patch.sessionId) return
        updateSelectedSession?.(patch)
        if (!needsSessionHydration({ ...selectedSession, ...patch })) {
          hydrationAttemptCountRef.current.delete(hydrationKey)
        }
      })
      .catch(() => {})
  }, [
    selectedSession?._uid,
    selectedSession?.sessionId,
    selectedSession?.itemId,
    selectedSession?.itemTitle,
    selectedSession?.itemPic,
    selectedSession?.peerNick,
    selectedSession?.peerUid,
    selectedSession?.peerRole,
    selectedSession?.selfRole,
    updateSelectedSession,
  ])

  // itemDetail — price lazy fetch
  useEffect(() => {
    const uid = selectedSession?._uid
    const itemId = selectedItem?.itemId
    if (!uid || !itemId) return
    if (itemPriceByItemId[itemId] !== undefined) return
    // Reserve the slot to prevent a double-fetch from re-renders.
    setItemPriceByItemId((prev) => ({ ...prev, [itemId]: '' }))
    itemDetail(uid, itemId)
      .then((detail: any) => {
        const raw =
          detail?.price ??
          detail?.item?.price ??
          detail?.itemDetail?.price ??
          detail?.data?.price ??
          ''
        const normalized = typeof raw === 'number' ? String(raw) : String(raw || '').trim()
        setItemPriceByItemId((prev) => ({ ...prev, [itemId]: normalized }))
      })
      .catch(() => {})
  }, [selectedSession?._uid, selectedItem?.itemId])

  // headInfo — promotion price / pic / desc; only sessionType=1
  useEffect(() => {
    const uid = selectedSession?._uid
    const itemId = String(
      selectedSession?.itemId ?? selectedSession?.item?.id ?? selectedItem?.itemId ?? '',
    ).trim()
    if (!uid || !itemId) return
    if (sessionType(selectedSession) !== 1) return
    const sid = selectedSession.sessionId
    if (headinfoBySessionId[sid] !== undefined) return
    setHeadinfoBySessionId((prev) => ({ ...prev, [sid]: null }))
    const peerUid = String(selectedSession.peerUid ?? selectedSession.peer_uid ?? '').trim()
    pcMessageHeadinfo(uid, sid, itemId, peerUid)
      .then((res: any) => {
        if (res?.hideHeader) return
        const hi = res?.headInfo
        if (!hi) return
        setHeadinfoBySessionId((prev) => ({
          ...prev,
          [sid]: {
            price: String(hi.title ?? '').trim(),
            promotionPrice: String(hi.promotionPrice ?? '').trim(),
            desc: String(hi.semanticsDesc ?? '').trim(),
            subTitle: String(hi.subTitle ?? '').trim(),
            tips: String(hi.tips ?? '').trim(),
            picUrl: String(hi.picUrl ?? '').trim(),
          },
        }))
      })
      .catch(() => {})
  }, [
    selectedSession?._uid,
    selectedSession?.sessionId,
    selectedSession?.itemId,
    selectedItem?.itemId,
  ])
}
