import { useEffect, useMemo, useState } from 'react'
import { Modal } from '../../../../components/Modal'
import { locationDivisionChildren, locationDivisions } from '../../../../api/market'
import { SuperPolishBadge } from '../../components/SuperPolishUi'
import { AreaTargetSupportNotice } from '../components/AreaTargetSupportNotice'
import type { AreaTargetSupportSummary } from '../areaTargetSupport'
import {
  ALL_HOURS,
  normalizeDivisionTree,
  selectionKey,
  toDivisionSelection,
  type DivisionNode,
  type DivisionSelection,
} from './helpers'

type AdvancedTab = 'division' | 'schedule'

function formatProvinceLabel(name: string) {
  return name.replace(/省$/u, '').replace(/市$/u, '')
}

export function CreatePlanAdvancedSettingsDialog({
  uid,
  open,
  initialTab = 'division',
  areaTargetAvailable,
  areaTargetSupport,
  creativeOptAvailable,
  initialDivisions,
  initialHours,
  initialCreativeOptEnabled,
  onClose,
  onSave,
}: {
  uid: string
  open: boolean
  initialTab?: AdvancedTab
  areaTargetAvailable: boolean
  areaTargetSupport: AreaTargetSupportSummary
  creativeOptAvailable: boolean
  initialDivisions: DivisionSelection[]
  initialHours: number[]
  initialCreativeOptEnabled: boolean
  onClose: () => void
  onSave: (value: {
    divisions: DivisionSelection[]
    hours: number[]
    creativeOptEnabled: boolean
  }) => void
}) {
  const [activeTab, setActiveTab] = useState<AdvancedTab>(initialTab)
  const [draftDivisions, setDraftDivisions] = useState<DivisionSelection[]>(initialDivisions)
  const [draftHours, setDraftHours] = useState<number[]>(initialHours)
  const [provinces, setProvinces] = useState<DivisionNode[]>([])
  const [activeProvinceId, setActiveProvinceId] = useState('')
  const [cities, setCities] = useState<DivisionNode[]>([])
  const [provinceTotals, setProvinceTotals] = useState<Record<string, number>>({})
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!open || !uid) return
    let active = true
    locationDivisions(uid)
      .then((result) => {
        if (!active) return
        const nextProvinces = normalizeDivisionTree(result, 'divisionArea')
        const preferredProvince = initialDivisions[0]?.province
        const matchedProvince = nextProvinces.find((province) => province.province === preferredProvince)
        const nextProvinceId = matchedProvince?.id || nextProvinces[0]?.id || ''
        setProvinces(nextProvinces)
        setActiveProvinceId(nextProvinceId)
        if (!nextProvinceId || !areaTargetAvailable) {
          setCities([])
          return
        }
        return locationDivisionChildren(uid, Number(nextProvinceId))
          .then((children) => {
            if (!active) return
            const nextCities = normalizeDivisionTree(children, 'items')
            setCities(nextCities)
            setProvinceTotals((current) => ({ ...current, [nextProvinceId]: nextCities.length }))
          })
      })
      .finally(() => {
        if (!active) return
        setLoading(false)
      })
    return () => {
      active = false
    }
  }, [areaTargetAvailable, initialDivisions, open, uid])

  const selectedDivisionKeys = useMemo(
    () => new Set(draftDivisions.map((division) => selectionKey(division))),
    [draftDivisions],
  )

  function toggleDivision(node: DivisionNode) {
    const selection = toDivisionSelection(node)
    if (!selection) return
    const key = selectionKey(selection)
    setDraftDivisions((current) => (
      current.some((entry) => selectionKey(entry) === key)
        ? current.filter((entry) => selectionKey(entry) !== key)
        : [...current, selection]
    ))
  }

  function toggleHour(hour: number) {
    setDraftHours((current) => (
      current.includes(hour)
        ? current.filter((value) => value !== hour)
        : [...current, hour].sort((left, right) => left - right)
    ))
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="高级设置"
      width={920}
      footer={(
        <>
          <button type="button" onClick={onClose} className="btn-secondary btn-sm">
            取消
          </button>
          <button
            type="button"
            onClick={() => onSave({
              divisions: areaTargetAvailable ? draftDivisions : [],
              hours: draftHours.length > 0 ? draftHours : ALL_HOURS,
              creativeOptEnabled: creativeOptAvailable ? initialCreativeOptEnabled : false,
            })}
            className="btn-primary btn-sm"
          >
            确定
          </button>
        </>
      )}
    >
      <div className="space-y-4">
        <div className="text-[12px] leading-5" style={{ color: 'var(--text-secondary)' }}>
          高级设置会影响计划拿量效果，如非必须建议使用系统默认设置。
        </div>

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => setActiveTab('division')}
            className="rounded-[14px] border px-3 py-2 text-[12px] font-medium"
            style={{
              borderColor: activeTab === 'division' ? 'var(--primary)' : 'var(--control-border)',
              background: activeTab === 'division' ? 'var(--primary-light)' : 'var(--surface-soft)',
              color: activeTab === 'division' ? 'var(--text-main)' : 'var(--text-secondary)',
            }}
          >
            投放地域
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('schedule')}
            className="rounded-[14px] border px-3 py-2 text-[12px] font-medium"
            style={{
              borderColor: activeTab === 'schedule' ? 'var(--primary)' : 'var(--control-border)',
              background: activeTab === 'schedule' ? 'var(--primary-light)' : 'var(--surface-soft)',
              color: activeTab === 'schedule' ? 'var(--text-main)' : 'var(--text-secondary)',
            }}
          >
            投放时段
          </button>
        </div>

        {activeTab === 'division' ? (
          <div className="space-y-4">
            {!areaTargetAvailable ? (
              <AreaTargetSupportNotice
                summary={areaTargetSupport}
                subject="当前所选宝贝"
              />
            ) : (
              <div className="space-y-4">
                {!areaTargetSupport.fullySupported ? (
                  <AreaTargetSupportNotice
                    summary={areaTargetSupport}
                    subject="当前所选宝贝"
                  />
                ) : null}

                <div className="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
                  <div
                    className="rounded-[18px] border p-3"
                    style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
                  >
                    <div className="mb-2 flex items-center justify-between gap-2">
                      <div className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>
                        投放地域
                      </div>
                      <button type="button" onClick={() => setDraftDivisions([])} className="btn-ghost btn-sm" disabled={draftDivisions.length === 0}>
                        全国
                      </button>
                    </div>
                    <div className="space-y-2">
                      {provinces.map((province) => (
                        <button
                          key={province.id}
                          type="button"
                          aria-label={formatProvinceLabel(province.province || province.name)}
                          onClick={() => {
                            if (!uid) return
                            setActiveProvinceId(province.id)
                            if (!areaTargetAvailable) {
                              setCities([])
                              return
                            }
                            setLoading(true)
                            void locationDivisionChildren(uid, Number(province.id))
                              .then((result) => {
                                const nextCities = normalizeDivisionTree(result, 'items')
                                setCities(nextCities)
                                setProvinceTotals((current) => ({ ...current, [province.id]: nextCities.length }))
                              })
                              .finally(() => {
                                setLoading(false)
                              })
                          }}
                          className="w-full rounded-[12px] border px-3 py-2 text-left text-[12px]"
                          style={{
                            borderColor: activeProvinceId === province.id ? 'var(--primary)' : 'var(--control-border)',
                            background: activeProvinceId === province.id ? 'var(--primary-light)' : 'var(--surface-card)',
                            color: 'var(--text-main)',
                          }}
                        >
                          {formatProvinceLabel(province.province || province.name)}
                          {provinceTotals[province.id]
                            ? `(${draftDivisions.filter((division) => division.province === province.province).length}/${provinceTotals[province.id]})`
                            : ''}
                        </button>
                      ))}
                    </div>
                  </div>

                  <div
                    className="rounded-[18px] border p-4"
                    style={{ borderColor: 'var(--control-border)', background: 'var(--surface-card)' }}
                  >
                    <div className="mb-3 flex flex-wrap items-center gap-2">
                      <SuperPolishBadge tone="info">已选 {draftDivisions.length} 城</SuperPolishBadge>
                      <button type="button" onClick={() => setDraftDivisions([])} className="btn-ghost btn-sm" disabled={draftDivisions.length === 0}>
                        清空
                      </button>
                    </div>
                    {loading ? (
                      <div className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                        正在加载城市...
                      </div>
                    ) : (
                      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                        {cities.map((city) => {
                          const selection = toDivisionSelection(city)
                          const checked = selection ? selectedDivisionKeys.has(selectionKey(selection)) : false
                          return (
                            <label
                              key={city.id}
                              className="flex items-center gap-2 rounded-[12px] border px-3 py-2 text-[12px]"
                              style={{
                                borderColor: checked ? 'var(--primary)' : 'var(--control-border)',
                                background: checked ? 'var(--primary-light)' : 'var(--surface-soft)',
                                color: 'var(--text-main)',
                              }}
                            >
                              <input
                                type="checkbox"
                                aria-label={city.city || city.name}
                                checked={checked}
                                onChange={() => toggleDivision(city)}
                              />
                              {city.city || city.name}
                            </label>
                          )
                        })}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}
          </div>
        ) : (
          <div
            className="rounded-[18px] border p-4"
            style={{ borderColor: 'var(--control-border)', background: 'var(--surface-soft)' }}
          >
            <div className="mb-3 flex flex-wrap items-center gap-2">
              <SuperPolishBadge tone="info">已选 {draftHours.length}/{ALL_HOURS.length} 时段</SuperPolishBadge>
              <button type="button" onClick={() => setDraftHours(ALL_HOURS)} className="btn-ghost btn-sm">
                恢复全部时段
              </button>
            </div>
            <div className="grid gap-2 sm:grid-cols-4 lg:grid-cols-6">
              {ALL_HOURS.map((hour) => {
                const selected = draftHours.includes(hour)
                return (
                  <button
                    key={hour}
                    type="button"
                    onClick={() => toggleHour(hour)}
                    className="rounded-[12px] border px-3 py-2 text-[12px]"
                    style={{
                      borderColor: selected ? 'var(--primary)' : 'var(--control-border)',
                      background: selected ? 'var(--primary-light)' : 'var(--surface-card)',
                      color: selected ? 'var(--text-main)' : 'var(--text-secondary)',
                    }}
                  >
                    {hour}时
                  </button>
                )
              })}
            </div>
          </div>
        )}
      </div>
    </Modal>
  )
}
