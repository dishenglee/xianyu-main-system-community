import { useEffect, useMemo, useState } from 'react'
import { Search } from 'lucide-react'
import { Modal } from '../../../../components/Modal'
import { getSuperPolishCampaignPrecheck, searchSuperPolishItems, unwrapMtopData } from '../../../../api/superPolish'
import { SuperPolishBadge, SuperPolishError, SuperPolishInput } from '../../components/SuperPolishUi'
import { asArray, asRecord } from '../../model'
import {
  formatCentMoney,
  formatItemStats,
  MAX_CREATE_ITEMS,
  normalizeItemTabs,
  normalizeSelectableItems,
  type SuperPolishItemTab,
} from '../create/helpers'
import type { SuperPolishCampaignDetailItem } from './types'

const FALLBACK_TABS: SuperPolishItemTab[] = [
  { key: 'all', label: '全部', count: 0, tag: '', description: '' },
  { key: 'platReco', label: '平台推荐', count: 0, tag: 'new', description: '' },
]

function toDetailItem(value: ReturnType<typeof normalizeSelectableItems>[number]): SuperPolishCampaignDetailItem {
  return {
    itemId: value.itemId,
    catId: value.catId,
    title: value.title,
    price: value.price,
    imageUrl: value.imageUrl,
    publishTime: value.publishTime,
    saleCount: value.saleCount,
    supportAd: value.supportAd,
    supportAreaTarget: value.supportAreaTarget,
    areaTarget: value.supportAreaTarget,
    tags: value.tags,
    notSupportReasons: value.notSupportReasons,
    raw: value.raw,
  }
}

export function CampaignDetailItemEditorModal({
  uid,
  open,
  selectedItems,
  onClose,
  onConfirm,
  onConfirmRemove,
}: {
  uid: string
  open: boolean
  selectedItems: SuperPolishCampaignDetailItem[]
  onClose: () => void
  onConfirm: (items: SuperPolishCampaignDetailItem[]) => void
  onConfirmRemove: (item: SuperPolishCampaignDetailItem) => Promise<boolean>
}) {
  const [draftItems, setDraftItems] = useState<SuperPolishCampaignDetailItem[]>(selectedItems)
  const [searchInput, setSearchInput] = useState('')
  const [tabs, setTabs] = useState<SuperPolishItemTab[]>(FALLBACK_TABS)
  const [items, setItems] = useState<SuperPolishCampaignDetailItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [tabsHydrated, setTabsHydrated] = useState(false)
  const [precheckStatus, setPrecheckStatus] = useState<'loading' | 'success' | 'error'>('loading')
  const [requestMeta, setRequestMeta] = useState({
    tab: 'platReco',
    searchKey: '',
    needTabs: true,
    requestId: 0,
  })

  const selectedIds = useMemo(
    () => draftItems.map((item) => item.itemId),
    [draftItems],
  )

  useEffect(() => {
    if (!open || !uid) return

    let active = true
    getSuperPolishCampaignPrecheck(uid)
      .then(() => {
        if (!active) return
        setPrecheckStatus('success')
      })
      .catch(() => {
        if (!active) return
        setPrecheckStatus('error')
      })

    return () => {
      active = false
    }
  }, [open, uid])

  useEffect(() => {
    if (!open || !uid) return

    let active = true
    searchSuperPolishItems(uid, {
      pageNum: 1,
      pageSize: 10,
      tab: requestMeta.tab,
      needTabs: requestMeta.needTabs,
      searchKey: requestMeta.searchKey,
    })
      .then((response) => {
        if (!active) return
        const payload = asRecord(unwrapMtopData(response))
        const result = asRecord(payload.result)
        const normalizedItems = normalizeSelectableItems(result.itemList ?? payload.result ?? []).map(toDetailItem)
        const rawTabs = asArray(result.tabList)
        setItems(normalizedItems)
        setError('')
        if (rawTabs.length > 0) {
          setTabs(normalizeItemTabs(result))
          setTabsHydrated(true)
        }
      })
      .catch((reason: unknown) => {
        if (!active) return
        setItems([])
        setError(reason instanceof Error ? reason.message : '加载宝贝失败')
      })
      .finally(() => {
        if (!active) return
        setLoading(false)
      })

    return () => {
      active = false
    }
  }, [open, requestMeta, uid])

  function triggerQuery(nextTab = requestMeta.tab) {
    setError('')
    setItems([])
    setLoading(true)
    setRequestMeta((current) => ({
      tab: nextTab,
      searchKey: searchInput,
      needTabs: !tabsHydrated,
      requestId: current.requestId + 1,
    }))
  }

  function handleAdd(item: SuperPolishCampaignDetailItem) {
    setDraftItems((current) => {
      if (current.some((entry) => entry.itemId === item.itemId)) return current
      if (current.length >= MAX_CREATE_ITEMS) return current
      return [...current, item]
    })
  }

  async function handleRemove(item: SuperPolishCampaignDetailItem) {
    const confirmed = await onConfirmRemove(item)
    if (!confirmed) return
    setDraftItems((current) => current.filter((entry) => entry.itemId !== item.itemId))
  }

  const precheckNotice = precheckStatus === 'loading'
    ? '正在校验建计划能力，请稍候继续选择宝贝。'
    : precheckStatus === 'error'
      ? '建计划能力校验失败，仍可继续查看候选宝贝。'
      : '计划能力已校验，可继续从推荐宝贝或搜索结果里补充当前计划。'

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="编辑宝贝"
      width={1040}
      footer={(
        <>
          <button type="button" onClick={onClose} className="btn-ghost btn-sm">
            取消
          </button>
          <button type="button" onClick={() => onConfirm(draftItems)} className="btn-primary btn-sm">
            确定
          </button>
        </>
      )}
    >
      <div className="space-y-4">
        <SuperPolishError message={error} />

        <div
          className="rounded-[18px] border px-4 py-3 text-[12px]"
          style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
        >
          {precheckNotice}
        </div>

        <div
          className="rounded-[18px] border px-4 py-4"
          style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
        >
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>
                当前已选宝贝
              </div>
              <SuperPolishBadge tone="info">{draftItems.length}/{MAX_CREATE_ITEMS}</SuperPolishBadge>
            </div>
            <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              这里对应当前计划最终会提交的宝贝池。
            </div>
          </div>

          <div className="mt-4 space-y-3">
            {draftItems.length === 0 ? (
              <div
                className="rounded-[16px] border px-4 py-4 text-[12px]"
                style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
              >
                当前还没有选中宝贝。
              </div>
            ) : draftItems.map((item) => (
              <div
                key={item.itemId}
                className="flex flex-wrap items-center justify-between gap-3 rounded-[16px] border px-4 py-3"
                style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
              >
                <div className="min-w-0">
                  <div className="text-[13px] font-semibold leading-5" style={{ color: 'var(--text-main)' }}>
                    {item.title}
                  </div>
                  <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                    商品 ID {item.itemId} · {formatCentMoney(item.price)}
                  </div>
                </div>
                <button type="button" onClick={() => void handleRemove(item)} className="btn-ghost btn-sm">
                  移除
                </button>
              </div>
            ))}
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              type="button"
              onClick={() => triggerQuery(tab.key)}
              className="rounded-[14px] border px-3 py-2 text-[12px] font-medium transition"
              style={{
                borderColor: requestMeta.tab === tab.key ? 'var(--primary)' : 'var(--control-border)',
                background: requestMeta.tab === tab.key ? 'var(--primary-light)' : 'var(--surface-soft)',
                color: requestMeta.tab === tab.key ? 'var(--text-main)' : 'var(--text-secondary)',
              }}
            >
              <span>{tab.label}</span>
              {tab.tag ? (
                <span className="ml-1 text-[10px]" style={{ color: 'var(--primary)' }}>
                  {tab.tag}
                </span>
              ) : null}
              <span className="ml-1 text-[11px] opacity-70">({tab.count})</span>
            </button>
          ))}
        </div>

        <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
          <SuperPolishInput
            value={searchInput}
            onChange={(event) => setSearchInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                triggerQuery()
              }
            }}
            placeholder="搜索宝贝标题"
          />
          <button type="button" onClick={() => triggerQuery()} className="btn-secondary btn-sm">
            <Search className="h-3.5 w-3.5" />
            搜索
          </button>
        </div>

        <div className="space-y-3">
          {loading ? (
            <div
              className="rounded-[18px] border px-4 py-5 text-[13px]"
              style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
            >
              正在加载候选宝贝...
            </div>
          ) : items.length === 0 ? (
            <div
              className="rounded-[18px] border px-4 py-5 text-[13px]"
              style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
            >
              当前没有可展示的候选宝贝。
            </div>
          ) : (
            <div
              className="overflow-hidden rounded-[18px] border"
              style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
            >
              <div
                className="hidden grid-cols-[minmax(0,1.8fr)_110px_180px_120px_100px] gap-3 border-b px-4 py-3 text-[12px] font-medium md:grid"
                style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)', background: 'var(--surface-soft)' }}
              >
                <div>候选宝贝</div>
                <div>售价</div>
                <div>数据</div>
                <div>定向</div>
                <div className="text-right">操作</div>
              </div>
              {items.map((item) => {
                const selected = selectedIds.includes(item.itemId)
                const disabled = !item.supportAd || item.notSupportReasons.length > 0
                return (
                  <div
                    key={item.itemId}
                    className="border-b px-4 py-4 last:border-b-0"
                    style={{
                      borderColor: 'var(--control-border)',
                      background: selected ? 'var(--primary-light)' : 'transparent',
                      opacity: disabled && !selected ? 0.76 : 1,
                    }}
                  >
                    <div className="grid gap-3 md:grid-cols-[minmax(0,1.8fr)_110px_180px_120px_100px] md:items-center">
                      <div className="flex min-w-0 items-start gap-3">
                        {item.imageUrl ? (
                          <img src={item.imageUrl} alt={item.title} className="h-14 w-14 rounded-[12px] object-cover" />
                        ) : (
                          <div
                            className="flex h-14 w-14 items-center justify-center rounded-[12px] text-[11px]"
                            style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
                          >
                            宝贝
                          </div>
                        )}

                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <div className="text-[13px] font-semibold leading-5" style={{ color: 'var(--text-main)' }}>
                              {item.title}
                            </div>
                            {selected ? <SuperPolishBadge tone="success">已添加</SuperPolishBadge> : null}
                          </div>
                          <div className="mt-1 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                            商品 ID {item.itemId}
                          </div>
                          <div className="mt-2 flex flex-wrap items-center gap-2">
                            {item.tags.map((tag) => (
                              <SuperPolishBadge key={`${item.itemId}-${tag}`} tone="neutral">
                                {tag}
                              </SuperPolishBadge>
                            ))}
                            {item.notSupportReasons.map((reason) => (
                              <SuperPolishBadge key={`${item.itemId}-${reason}`} tone="warning">
                                {reason}
                              </SuperPolishBadge>
                            ))}
                          </div>
                        </div>
                      </div>

                      <div className="text-[13px] font-medium" style={{ color: 'var(--text-main)' }}>
                        {formatCentMoney(item.price)}
                      </div>

                      <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                        {formatItemStats({
                          itemId: item.itemId,
                          catId: item.catId,
                          title: item.title,
                          price: item.price,
                          imageUrl: item.imageUrl,
                          publishTime: item.publishTime,
                          browseCount: Number(item.raw.broseCnt) || 0,
                          exposureCount: Number(item.raw.exposureCnt) || 0,
                          saleCount: item.saleCount,
                          supportAd: item.supportAd,
                          supportAreaTarget: item.supportAreaTarget,
                          checked: false,
                          tags: item.tags,
                          notSupportReasons: item.notSupportReasons,
                          raw: item.raw,
                        })}
                      </div>

                      <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                        {item.supportAreaTarget ? '可城市定向' : '全部地域'}
                      </div>

                      <div className="flex justify-end">
                        {selected ? (
                          <span className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                            已在当前计划
                          </span>
                        ) : item.notSupportReasons.length > 0 ? (
                          <span className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                            {item.notSupportReasons[0]}
                          </span>
                        ) : (
                          <button
                            type="button"
                            onClick={() => handleAdd(item)}
                            className="btn-secondary btn-sm"
                            disabled={draftItems.length >= MAX_CREATE_ITEMS}
                          >
                            添加
                          </button>
                        )}
                      </div>
                    </div>
                  </div>
                )
              })}
            </div>
          )}
        </div>
      </div>
    </Modal>
  )
}
