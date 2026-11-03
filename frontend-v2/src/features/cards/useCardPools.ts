import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { bulkCreateCards, deleteCard, getCards, updateCard } from '../../api'
import { getAuthToken } from '../../api/client'
import type { Card } from '../../api/types'
import { buildCardImportPayloads } from './cardImport'
import { buildCardGroupDeletePlan, buildCardGroupUpdatePlan, groupCardPools } from './cardPoolGroups'
import type { CardPoolGroup } from './cardPoolGroups'

export type LoadedCard = Card & { cardKey?: string }

export function useCardPools() {
  const [cards, setCards] = useState<LoadedCard[]>([])
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const requestRef = useRef(0)
  const pools = useMemo(() => groupCardPools(cards), [cards])

  const load = useCallback(async () => {
    const requestId = ++requestRef.current
    const token = getAuthToken()
    setLoading(true)
    try {
      const list = await getCards('__global__')
      if (requestId === requestRef.current && token === getAuthToken()) setCards(Array.isArray(list) ? list : [])
    } finally {
      if (requestId === requestRef.current) setLoading(false)
    }
  }, [])

  useEffect(() => {
    const timer = window.setTimeout(() => { void load() }, 0)
    return () => window.clearTimeout(timer)
  }, [load])

  const importCards = useCallback(async (draft: { tag: string; rawText: string; reusable: boolean }) => {
    const tag = draft.tag.trim()
    if (!tag) throw new Error('请输入卡池标签')
    const existing = pools.find((pool) => pool.tag === tag)
    if (existing && (existing.type === 'mixed' || (existing.type === 'reusable') !== draft.reusable)) {
      throw new Error('同一标签不能混合一次性卡密和固定卡密')
    }
    const payloads = buildCardImportPayloads({ rawText: draft.rawText, cardPoolTag: tag, reusable: draft.reusable })
    if (!payloads.length) throw new Error('至少输入一张卡密')
    setSaving(true)
    try {
      for (let index = 0; index < payloads.length; index += 20) {
        await bulkCreateCards('__global__', payloads.slice(index, index + 20))
      }
      await load()
      return payloads.length
    } finally {
      setSaving(false)
    }
  }, [load, pools])

  const updatePool = useCallback(async (pool: CardPoolGroup<LoadedCard>, draft: { tag: string; contentsById: Record<number, string> }) => {
    const duplicate = pools.find((candidate) => candidate.key !== pool.key && candidate.tag === draft.tag.trim())
    if (duplicate) throw new Error('该标签名称已存在')
    const plan = buildCardGroupUpdatePlan(pool, draft)
    setSaving(true)
    try {
      const results = await Promise.allSettled(plan.map((item) => updateCard(item.id, item.payload)))
      const failed = results.filter((result) => result.status === 'rejected').length
      await load()
      return { updated: results.length - failed, failed }
    } finally {
      setSaving(false)
    }
  }, [load, pools])

  const deletePool = useCallback(async (pool: CardPoolGroup<LoadedCard>) => {
    const ids = buildCardGroupDeletePlan(pool)
    setSaving(true)
    try {
      const results = await Promise.allSettled(ids.map((id) => deleteCard(id)))
      const failed = results.filter((result) => result.status === 'rejected').length
      await load()
      return { deleted: results.length - failed, failed }
    } finally {
      setSaving(false)
    }
  }, [load])

  return { pools, loading, saving, load, importCards, updatePool, deletePool }
}
