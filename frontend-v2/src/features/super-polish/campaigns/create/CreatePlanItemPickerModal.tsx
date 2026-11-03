import { useEffect, useMemo, useState } from 'react'
import { Search } from 'lucide-react'
import { Modal } from '../../../../components/Modal'
import { searchSuperPolishItems, unwrapMtopData } from '../../../../api/superPolish'
import { SafeRemoteImage } from '../../../messages/SafeRemoteImage'
import { SuperPolishBadge, SuperPolishError, SuperPolishInput } from '../../components/SuperPolishUi'
import { asArray, asRecord } from '../../model'
import {
  formatCentMoney,
  formatItemStats,
  MAX_CREATE_ITEMS,
  normalizeItemTabs,
  normalizeSelectableItems,
  type SuperPolishItemTab,
  type SuperPolishSelectableItem,
} from './helpers'

const FALLBACK_TABS: SuperPolishItemTab[] = [
  { key: 'all', label: '全部', count: 0, tag: '', description: '' },
  { key: 'platReco', label: '平台推荐', count: 0, tag: 'new', description: '' },
]

export function CreatePlanItemPickerModal({
  uid,
  open,
  selectedIds,
  selectedCount,
  onClose,
  onToggleItem,
  onAddItems,
  onClearSelected,
  onRememberItems,
}: {
  uid: string
  open: boolean
  selectedIds: string[]
  selectedCount: number
  onClose: () => void
  onToggleItem: (item: SuperPolishSelectableItem) => void
  onAddItems: (items: SuperPolishSelectableItem[]) => void
  onClearSelected: () => void
  onRememberItems: (items: SuperPolishSelectableItem[]) => void
}) {
  const [searchInput, setSearchInput] = useState('')
  const [tabs, setTabs] = useState<SuperPolishItemTab[]>(FALLBACK_TABS)
  const [items, setItems] = useState<SuperPolishSelectableItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [tabsHydrated, setTabsHydrated] = useState(false)
  const [requestMeta, setRequestMeta] = useState({
    tab: 'platReco',
    searchKey: '',
    needTabs: true,
    requestId: 0,
  })

  const activeTab = requestMeta.tab

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
        const normalizedItems = normalizeSelectableItems(result.itemList ?? payload.result ?? [])
        const rawTabs = asArray(result.tabList)
        setItems(normalizedItems)
        setError('')
        if (rawTabs.length > 0) {
          setTabs(normalizeItemTabs(result))
          setTabsHydrated(true)
        }
        onRememberItems(normalizedItems)
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
  }, [onRememberItems, open, requestMeta, uid])

  function triggerQuery(nextTab = activeTab) {
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

  const selectableItems = useMemo(
    () => items.filter((item) => item.supportAd),
    [items],
  )

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="新增宝贝"
      width={960}
      footer={(
        <>
          <button type="button" onClick={onClose} className="btn-secondary btn-sm">
            确定
          </button>
        </>
      )}
    >
      <div className="space-y-4">
        <SuperPolishError message={error} />

        <div className="flex flex-wrap items-center gap-2">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              type="button"
              onClick={() => triggerQuery(tab.key)}
              className="rounded-[14px] border px-3 py-2 text-[12px] font-medium transition"
              style={{
                borderColor: activeTab === tab.key ? 'var(--primary)' : 'var(--control-border)',
                background: activeTab === tab.key ? 'var(--primary-light)' : 'var(--surface-soft)',
                color: activeTab === tab.key ? 'var(--text-main)' : 'var(--text-secondary)',
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
          <button
            type="button"
            onClick={() => triggerQuery()}
            className="btn-secondary btn-sm"
          >
            <Search className="h-3.5 w-3.5" />
            搜索
          </button>
        </div>

        <div
          className="flex flex-wrap items-center justify-between gap-2 rounded-[16px] border px-3 py-2.5 md:gap-3 md:rounded-[18px] md:px-4 md:py-3"
          style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
        >
          <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
            <SuperPolishBadge tone="info">已添加: {selectedCount}/{MAX_CREATE_ITEMS}</SuperPolishBadge>
            <span className="min-w-0 text-[11px] md:text-[12px]" style={{ color: 'var(--text-secondary)' }}>
              当前 tab 共 {items.length} 个宝贝，可直接批量加入当前计划池。
            </span>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <button type="button" onClick={() => onAddItems(selectableItems)} className="btn-secondary btn-sm" disabled={selectableItems.length === 0}>
              全部添加
            </button>
            <button type="button" onClick={onClearSelected} className="btn-ghost btn-sm" disabled={selectedCount === 0}>
              清空
            </button>
          </div>
        </div>

        <div className="space-y-3">
          {loading ? (
            <div
              className="rounded-[18px] border px-4 py-5 text-[13px]"
              style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
            >
              正在加载宝贝...
            </div>
          ) : items.length === 0 ? (
            <div
              className="rounded-[18px] border px-4 py-5 text-[13px]"
              style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}
            >
              当前没有可展示的宝贝。
            </div>
          ) : (
            <div
              className="overflow-hidden rounded-[18px] border"
              style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
            >
              <div
                className="hidden grid-cols-[minmax(0,1.7fr)_110px_180px_120px_90px] gap-3 border-b px-4 py-3 text-[12px] font-medium md:grid"
                style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)', background: 'var(--surface-soft)' }}
              >
                <div>宝贝</div>
                <div>售价</div>
                <div>数据</div>
                <div>定向</div>
                <div className="text-right">操作</div>
              </div>
              {items.map((item) => {
                const selected = selectedIds.includes(item.itemId)
                const disabled = !item.supportAd
                return (
                  <div
                    key={item.itemId}
                    className="border-b px-3 py-3 last:border-b-0 md:px-4 md:py-4"
                    style={{
                      borderColor: 'var(--control-border)',
                      background: selected ? 'var(--primary-light)' : 'transparent',
                      opacity: disabled ? 0.72 : 1,
                    }}
                  >
                    <div className="grid gap-2 md:grid-cols-[minmax(0,1.7fr)_110px_180px_120px_90px] md:items-center md:gap-3">
                      <div className="flex min-w-0 items-start gap-2.5 md:gap-3">
                        <SafeRemoteImage
                          src={item.imageUrl}
                          alt={item.title}
                          className="h-12 w-12 rounded-[12px] object-cover md:h-14 md:w-14"
                          fallback={(
                            <div
                              className="flex h-12 w-12 items-center justify-center rounded-[12px] text-[11px] md:h-14 md:w-14"
                              style={{ background: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
                            >
                              宝贝
                            </div>
                          )}
                        />

                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <div className="text-[13px] font-semibold leading-[18px] md:leading-5" style={{ color: 'var(--text-main)' }}>
                              {item.title}
                            </div>
                            {selected ? <SuperPolishBadge tone="success">已添加</SuperPolishBadge> : null}
                          </div>
                          <div className="mt-0.5 text-[11px] md:mt-1 md:text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                            商品 ID {item.itemId}
                          </div>
                          {item.notSupportReasons.length > 0 ? (
                            <div className="mt-1 text-[11px] md:text-[12px]" style={{ color: 'var(--status-warning-text)' }}>
                              {item.notSupportReasons.join(' / ')}
                            </div>
                          ) : null}
                        </div>
                      </div>

                      <div className="flex min-w-0 items-center gap-1.5 md:contents">
                        <div className="shrink-0 text-[12px] font-semibold md:font-normal" style={{ color: 'var(--text-main)' }}>
                          {formatCentMoney(item.price)}
                        </div>
                        <div className="min-w-0 flex-1 truncate text-[11px] md:text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                          {formatItemStats(item)}
                        </div>
                        <div className="shrink-0">
                          {item.supportAreaTarget ? <SuperPolishBadge tone="info">城市定向</SuperPolishBadge> : <SuperPolishBadge tone="neutral">全部地域</SuperPolishBadge>}
                        </div>
                        <div className="shrink-0 md:flex md:items-center md:justify-end">
                          <button
                            type="button"
                            onClick={() => onToggleItem(item)}
                            disabled={disabled}
                            className={selected ? 'btn-ghost btn-sm whitespace-nowrap px-2 md:px-3' : 'btn-secondary btn-sm whitespace-nowrap px-2 md:px-3'}
                          >
                            {selected ? '取消添加' : '添加'}
                          </button>
                        </div>
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
