// @ts-nocheck
// ---------- 发布 modal ----------
// 参考老项目 frontend/src/pages/Items.vue 的发布弹窗实现。
// 流程：账号→描述/价格→图片（并发上传拿 CDN url）→POI 地点→发货方式→提交。
// itemImportLink 可一键从闲鱼链接/口令预填标题/描述/价格/图片。
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  RefreshCw, Sparkles, UploadCloud, Plus, MapPin, X,
} from 'lucide-react'
import {
  itemKgraph, itemAiDescription, itemImportLink, uploadItemImage,
  publishBadwords, request, locationByGps, retMsg,
} from '../../api/index'
import {
  pickKgraphCategory, pickKgraphCardList, categoryLabel,
  deriveTitleFromDescription, extractAiDesc, extractAiTitle, isBadwordsSafe, extractBadwordsMatches,
  pickPreferredPublishAccount, resolveSimpleItemFlag,
} from './publishUtils'
import { filterBusinessAccounts } from '../../utils/accountAccess'
import { emitActionFailure, emitActionLog, emitActionStarted, emitActionSuccess } from '../../bootstrap/actionLogging'
export { publishCheckMessage, isPublishAllowed } from './publishUtils'

function Field({ label, children }) {
  return (
    <div>
      <div className="text-[11.5px] mb-1" style={{ color: 'var(--text-muted)' }}>{label}</div>
      {children}
    </div>
  )
}
const DEFAULT_SHIPPING = 'free' as const
const MAX_IMAGES = 9

function buildPublishInputSignature(desc: string, imageUrls: string[]) {
  return JSON.stringify({
    desc: String(desc || '').trim(),
    imageUrls: [...(imageUrls || [])].filter(Boolean),
  })
}

function publishImportContext(form: any, sourceText: string, selectedUids: string[], imageUrls: string[] = []) {
  return {
    uid: form.uid,
    accountCount: selectedUids.length,
    inputLength: sourceText.length,
    urlCount: (sourceText.match(/https?:\/\//g) || []).length,
    descLength: String(form.desc || '').trim().length,
    imageCount: imageUrls.length,
  }
}

export function PublishItemModal({ accounts, onClose, onBatchPublish, notify = () => {} }) {
  const sortedAccounts = [...(accounts || [])].sort(
    (a, b) => (Number(b.security_score) || 0) - (Number(a.security_score) || 0),
  )
  const availableAccounts = filterBusinessAccounts(sortedAccounts)
  const [form, setForm] = useState(() => {
    const a = pickPreferredPublishAccount(availableAccounts)
    const loc = a?.location || {}
    return {
      uid: a?.uid || '',
      title: '',
      desc: '',
      price: '',
      origPrice: '',
      quantity: 1,
      simpleItem: false,
      noBargain: false,
      shippingType: DEFAULT_SHIPPING as 'free' | 'freight' | 'fixed' | 'self',
      postPrice: '',
      freightTemplateId: '',
      province: String(loc.province || ''),
      city: String(loc.city || ''),
      area: String(loc.area || ''),
      divisionId: String(loc.divisionId || ''),
      latitude: String(loc.latitude || ''),
      longitude: String(loc.longitude || ''),
    }
  })
  const [images, setImages] = useState<any[]>([])
  const [saving, setSaving] = useState(false)
  const [err, setErr] = useState('')
  const [category, setCategory] = useState<any>(null)
  const [categoryLoading, setCategoryLoading] = useState(false)
  const [categoryError, setCategoryError] = useState('')
  const [descLoading, setDescLoading] = useState(false)
  const [pubPropCards, setPubPropCards] = useState<any[]>([])
  const [pubPropSelections, setPubPropSelections] = useState<Record<string, string>>({})
  const [selectedUids, setSelectedUids] = useState<string[]>(() => form.uid ? [form.uid] : [])
  const [enableCoinDeduction, setEnableCoinDeduction] = useState(true)
  const [coinDeductionPercent, setCoinDeductionPercent] = useState('5')
  const [enableGroupon, setEnableGroupon] = useState(true)
  const [grouponAmount, setGrouponAmount] = useState('')

  function toggleUid(uid: string) {
    setSelectedUids((prev) => {
      if (prev.includes(uid)) {
        if (prev.length <= 1) return prev
        const next = prev.filter((u) => u !== uid)
        if (uid === form.uid) patch({ uid: next[0] })
        return next
      }
      return [...prev, uid]
    })
  }

  function toggleAllUids() {
    const allSelected = availableAccounts.length > 0
      && availableAccounts.every((a: any) => selectedUids.includes(a.uid))
    if (allSelected) {
      setSelectedUids(form.uid ? [form.uid] : [])
    } else {
      setSelectedUids(availableAccounts.map((a: any) => a.uid))
    }
  }

  const [importText, setImportText] = useState('')
  const [importing, setImporting] = useState(false)
  // 位置 autocomplete state：用户在输入框打字 → 防抖 → 调高德 inputtips → 下拉。
  // 跟 LocationPicker modal 内部完全相同的 backend 接口，只是 UI 是 inline 下拉
  // 而不是弹窗——发布商品要的就是「打字就搜，选一下就填」的行内体验。
  const [poiQuery, setPoiQuery] = useState('')
  const [poiSuggestions, setPoiSuggestions] = useState<any[]>([])
  const [poiBusy, setPoiBusy] = useState(false)
  const [poiOpen, setPoiOpen] = useState(false)
  const poiReqRef = useRef(0)
  const [locationLabel, setLocationLabel] = useState('')
  const latestTitleRef = useRef('')
  const lastGeneratedTitleRef = useRef('')
  const latestDescRef = useRef('')
  const lastGeneratedDescRef = useRef('')
  const descLoadingRef = useRef(false)
  const categoryRequestSeqRef = useRef(0)
  const aiRequestSeqRef = useRef(0)
  const simpleItemExplicitRef = useRef<boolean | null>(null)

  function patch(p: Partial<typeof form>) { setForm((f) => ({ ...f, ...p })) }
  function updateTitle(nextTitle: string, source: 'ai' | 'manual' = 'manual') {
    latestTitleRef.current = String(nextTitle || '')
    patch({ title: nextTitle })
    const trimmed = latestTitleRef.current.trim()
    if (!trimmed) {
      lastGeneratedTitleRef.current = ''
      return
    }
    if (source === 'ai') {
      lastGeneratedTitleRef.current = trimmed
    } else if (trimmed !== lastGeneratedTitleRef.current) {
      lastGeneratedTitleRef.current = ''
    }
  }
  function updateDesc(nextDesc: string, source: 'ai' | 'manual' = 'manual') {
    latestDescRef.current = String(nextDesc || '')
    patch({ desc: nextDesc })
    const trimmed = latestDescRef.current.trim()
    if (!trimmed) {
      lastGeneratedDescRef.current = ''
      return
    }
    if (source === 'ai') {
      lastGeneratedDescRef.current = trimmed
    } else if (trimmed !== lastGeneratedDescRef.current) {
      lastGeneratedDescRef.current = ''
    }
  }
  function uploadedImageUrls(list = images) {
    return list.filter((i) => i.url && !i.error).map((i) => i.url)
  }
  function buildPublishTitle({
    title = form.title,
    desc = form.desc,
    categoryOverride = category,
  }: {
    title?: string
    desc?: string
    categoryOverride?: any
  } = {}) {
    return String(title || '').trim()
      || deriveTitleFromDescription(String(desc || ''))
      || categoryLabel(categoryOverride)
      || ''
  }
  const canRefreshCategory = !!form.uid && (!!String(form.desc || '').trim() || uploadedImageUrls().length > 0)
  const canGenerateDescription = !!form.uid && (!!String(form.desc || '').trim() || uploadedImageUrls().length > 0)
  const resolvedPublishTitle = buildPublishTitle()

  async function refreshCategory({
    nextDesc = form.desc,
    nextImages = uploadedImageUrls(),
  } = {}) {
    if (!form.uid) return
    if (!String(nextDesc || '').trim() && !nextImages.length) return
    const requestSeq = ++categoryRequestSeqRef.current
    const normalizedDesc = String(nextDesc || '')
    const normalizedImages = [...(nextImages || [])].filter(Boolean)
    setCategoryLoading(true)
    setCategoryError('')
    try {
      const r: any = await itemKgraph(form.uid, {
        title: '',
        desc: normalizedDesc,
        imageUrls: normalizedImages,
      })
      if (requestSeq !== categoryRequestSeqRef.current) {
        return null
      }
      const cat = pickKgraphCategory(r)
      const nextCategory = cat && categoryLabel(cat) ? cat : null
      if (cat && categoryLabel(cat)) {
        setCategory(cat)
      } else {
        setCategory(null)
        setCategoryError('未识别到分类')
      }
      const cards = pickKgraphCardList(r)
      setPubPropCards(cards)
      const initSel: Record<string, string> = {}
      for (const card of cards) {
        const clicked = card.valuesList.find((v: any) => v.isClicked === '1')
        if (clicked) {
          const vid = String(clicked.valueId || clicked.channelCatId || clicked.catId || '')
          if (vid) initSel[card.propertyId] = vid
        }
      }
      setPubPropSelections(initSel)
      if (simpleItemExplicitRef.current === null) {
        patch({
          simpleItem: resolveSimpleItemFlag(null, nextCategory, {
            title: latestTitleRef.current,
            desc: normalizedDesc,
          }),
        })
      }
      return {
        category: nextCategory,
        cards,
        propSelections: initSel,
      }
    } catch (e: any) {
      if (requestSeq !== categoryRequestSeqRef.current) {
        return null
      }
      setCategory(null)
      setCategoryError(e?.message ? `分类识别失败：${e.message}` : '分类识别失败')
      throw e
    } finally {
      if (requestSeq === categoryRequestSeqRef.current) {
        setCategoryLoading(false)
      }
    }
  }

  async function generateDescription({
    descOverride = form.desc,
    imageUrlsOverride = uploadedImageUrls(),
    categoryOverride = category,
  } = {}) {
    if (!form.uid || descLoadingRef.current) return false
    const normalizedDesc = String(descOverride || '').trim()
    const normalizedImages = imageUrlsOverride.filter(Boolean)
    const inputSignature = buildPublishInputSignature(normalizedDesc, normalizedImages)
    if (!normalizedDesc && !normalizedImages.length) {
      setErr('请先输入商品描述或上传商品图片，再生成 AI 描述')
      return false
    }
    const requestSeq = ++aiRequestSeqRef.current
    descLoadingRef.current = true
    setDescLoading(true)
    setErr('')
    try {
      let resolvedCategory = null
      const refreshResult: any = await refreshCategory({
        nextDesc: normalizedDesc,
        nextImages: normalizedImages,
      }).catch(() => null)
      resolvedCategory = refreshResult?.category ?? categoryOverride ?? null
      const latestSignature = buildPublishInputSignature(
        latestDescRef.current.trim(),
        uploadedImageUrls(),
      )
      if (requestSeq !== aiRequestSeqRef.current || latestSignature !== inputSignature) {
        setErr('描述或图片已变更，请重新点击 AI 写描述')
        return false
      }
      const r: any = await itemAiDescription(form.uid, {
        title: '',
        desc: normalizedDesc,
        category: resolvedCategory,
        imageUrls: normalizedImages,
        cardList: refreshResult?.cards ?? pubPropCards,
        propSelections: refreshResult?.propSelections ?? pubPropSelections,
      })
      const latestSignatureAfterRequest = buildPublishInputSignature(
        latestDescRef.current.trim(),
        uploadedImageUrls(),
      )
      if (requestSeq !== aiRequestSeqRef.current || latestSignatureAfterRequest !== inputSignature) {
        setErr('描述或图片已变更，请重新点击 AI 写描述')
        return false
      }
      const desc = extractAiDesc(r)
      const aiTitle = extractAiTitle(r)
      const nextTitle = aiTitle
        || deriveTitleFromDescription(desc)
        || deriveTitleFromDescription(normalizedDesc)
        || buildPublishTitle({ desc: normalizedDesc, categoryOverride: resolvedCategory })
      const currentTitle = latestTitleRef.current.trim()
      const canOverwriteCurrentTitle = !currentTitle
        || currentTitle === lastGeneratedTitleRef.current
        || !!aiTitle
      if (nextTitle && canOverwriteCurrentTitle) {
        updateTitle(nextTitle, 'ai')
      }
      if (desc) {
        updateDesc(desc, 'ai')
        return true
      }
      setErr('AI 描述未返回可用内容')
      return false
    } catch (e: any) {
      setErr(String(e?.message || e || '网络错误'))
      return false
    } finally {
      descLoadingRef.current = false
      setDescLoading(false)
    }
  }

  useEffect(() => {
    latestTitleRef.current = ''
    lastGeneratedTitleRef.current = ''
    latestDescRef.current = ''
    lastGeneratedDescRef.current = ''
    const a = accounts?.find((x: any) => x.uid === form.uid)
    if (!a) return
    setCategory(null); setCategoryError(''); setPubPropCards([]); setPubPropSelections({})
    setPoiQuery('')
    setLocationLabel('')
    const loc = a.location || {}
    patch({
      province: String(loc.province || ''), city: String(loc.city || ''),
      area: String(loc.area || ''), divisionId: String(loc.divisionId || ''),
      latitude: String(loc.latitude || ''), longitude: String(loc.longitude || ''),
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [form.uid])

  useEffect(() => {
    latestTitleRef.current = String(form.title || '')
  }, [form.title])

  useEffect(() => {
    latestDescRef.current = String(form.desc || '')
  }, [form.desc])

  useEffect(() => {
    if (simpleItemExplicitRef.current !== null) return
    const inferred = resolveSimpleItemFlag(null, category, {
      title: latestTitleRef.current,
      desc: latestDescRef.current,
    })
    setForm((current) => (current.simpleItem === inferred ? current : {
      ...current,
      simpleItem: inferred,
    }))
  }, [category, form.title, form.desc])

  // POI inline autocomplete —— 用户在位置输入框打字，300ms 防抖调高德 inputtips。
  useEffect(() => {
    const kw = poiQuery.trim()
    if (!kw) {
      setPoiSuggestions([])
      setPoiBusy(false)
      return
    }
    const myReq = ++poiReqRef.current
    setPoiBusy(true)
    const timer = window.setTimeout(async () => {
      try {
        const resp: any = await request('POST', '/api/messages/location/search', {
          keyword: kw,
          origin_latitude: Number(form.latitude || 0) || 0,
          origin_longitude: Number(form.longitude || 0) || 0,
        })
        if (poiReqRef.current !== myReq) return
        setPoiSuggestions(Array.isArray(resp?.tips) ? resp.tips.slice(0, 8) : [])
      } catch {
        if (poiReqRef.current !== myReq) return
        setPoiSuggestions([])
      } finally {
        if (poiReqRef.current === myReq) setPoiBusy(false)
      }
    }, 300)
    return () => window.clearTimeout(timer)
  }, [poiQuery, form.latitude, form.longitude])

  async function applyPoiSuggestion(p: any) {
    const latitude = String(p.latitude || form.latitude)
    const longitude = String(p.longitude || form.longitude)
    let province = String(form.province || '')
    let city = String(form.city || '')
    let area = String(p.district || form.area || '')
    let divisionId = String(p.adcode || form.divisionId || '')
    try {
      const enriched: any = await locationByGps(form.uid, Number(latitude), Number(longitude))
      province = String(enriched?.prov || province)
      city = String(enriched?.city || city)
      area = String(enriched?.area || area)
      divisionId = String(enriched?.divisionId || divisionId)
    } catch {
      // 搜索结果本身带的 adcode/district 也能工作；反查失败不阻断用户选择。
    }
    patch({
      province,
      city,
      area,
      divisionId,
      latitude,
      longitude,
    })
    setLocationLabel(String(p.name || ''))
    setPoiQuery(String(p.name || ''))
    setPoiSuggestions([])
    setPoiOpen(false)
  }

  async function doImportLink() {
    if (!importText.trim() || !form.uid) return
    setImporting(true)
    setErr('')
    const sourceText = importText.trim()
    emitActionStarted({
      module: 'items-publish',
      action: 'import_item_link',
      message: '开始通过链接导入商品信息',
      context: publishImportContext(form, sourceText, selectedUids, uploadedImageUrls()),
    })
    try {
      const r: any = await itemImportLink(form.uid, sourceText)
      if (r?.detail) {
        const message = String(r.detail || '导入失败')
        setErr(message)
        notify(message, 'error')
        emitActionLog({
          module: 'items-publish',
          action: 'import_item_link',
          outcome: 'warn',
          message: '链接导入商品信息未成功',
          detail: message,
          retryable: true,
          promoteToIncident: false,
          context: publishImportContext(form, sourceText, selectedUids, uploadedImageUrls()),
        })
        return
      }
      const d = r?.data ?? r
      const importedTitle = String(d?.title || '').trim()
      const importedDesc = String(d?.desc || d?.description || '').trim()
      const rawImageUrls = Array.isArray(d?.imageUrls) ? d.imageUrls.filter(Boolean) : []
      const hasImportedContent = !!(importedTitle || importedDesc || rawImageUrls.length)
      if (!hasImportedContent) {
        const message = retMsg(r, '导入失败')
        setErr(message)
        notify(message, 'error')
        emitActionLog({
          module: 'items-publish',
          action: 'import_item_link',
          outcome: 'warn',
          message: '链接导入商品信息未成功',
          detail: message,
          retryable: true,
          promoteToIncident: false,
          context: publishImportContext(form, sourceText, selectedUids, uploadedImageUrls()),
        })
        return
      }
      const nextTitle = String(d?.title || form.title)
      const nextDesc = String(d?.desc || d?.description || form.desc)
      const nextImageUrls = rawImageUrls
      simpleItemExplicitRef.current = d?.simpleItem !== undefined ? !!d.simpleItem : null
      // 链接导入：标题/描述/价格/图片 + **原帖位置**（如果有）。`itemImportLink`
      // 后端会把上游 detail 里的 itemAddrDTO 字段映射出来——拿到就直接覆盖
      // 当前账号的默认发布地，符合「拿别人链接发布就带别人位置」的预期。
      patch({
        price: String(d?.price || form.price),
        origPrice: String(d?.origPrice || form.origPrice),
        simpleItem: resolveSimpleItemFlag(d?.simpleItem, category, {
          title: nextTitle,
          desc: nextDesc,
        }),
        noBargain: d?.noBargain !== undefined ? !!d.noBargain : form.noBargain,
        ...(d?.province ? { province: String(d.province) } : {}),
        ...(d?.city ? { city: String(d.city) } : {}),
        ...(d?.area ? { area: String(d.area) } : {}),
        ...(d?.divisionId ? { divisionId: String(d.divisionId) } : {}),
        ...(d?.latitude ? { latitude: String(d.latitude) } : {}),
        ...(d?.longitude ? { longitude: String(d.longitude) } : {}),
      })
      updateTitle(nextTitle, 'manual')
      updateDesc(nextDesc, 'manual')
      setLocationLabel('')
      setPoiQuery('')
      if (nextImageUrls.length) {
        setImages(nextImageUrls.map((url: string) => ({
          file: null, previewUrl: url, url, uploading: false, error: '',
        })))
      }
      if (String(nextDesc || '').trim() || nextImageUrls.length) {
        refreshCategory({
          nextDesc,
          nextImages: nextImageUrls,
        }).catch(() => null)
      }
      setImportText('')
      notify('已导入商品信息', 'success')
      emitActionSuccess({
        module: 'items-publish',
        action: 'import_item_link',
        message: '链接导入商品信息成功',
        context: {
          ...publishImportContext(form, sourceText, selectedUids, nextImageUrls),
          hasTitle: !!d?.title,
          importedDescLength: String(nextDesc || '').trim().length,
        },
      })
    } catch (e: any) {
      const message = String(e?.message || e || '网络错误')
      setErr(message)
      notify(message, 'error')
      emitActionFailure({
        module: 'items-publish',
        action: 'import_item_link',
        message: '链接导入商品信息失败',
        error: e,
        context: publishImportContext(form, sourceText, selectedUids, uploadedImageUrls()),
      })
    } finally {
      setImporting(false)
    }
  }

  async function addFiles(files: File[]) {
    const remaining = MAX_IMAGES - images.length
    const toAdd = files.slice(0, remaining).filter((f) => f.type.startsWith('image/'))
    if (!toAdd.length) return
    const existingImages = images
    // 占位 + 并发上传
    const base = existingImages.length
    const placeholders = toAdd.map((f) => ({
      file: f, previewUrl: URL.createObjectURL(f), url: '', uploading: true, error: '',
    }))
    setImages((prev) => [...prev, ...placeholders])
    const uploadResults = await Promise.all(toAdd.map(async (f, i) => {
      const idx = base + i
      try {
        const r: any = await uploadItemImage(form.uid, f)
        const url = r?.success && r?.url ? String(r.url) : ''
        setImages((prev) => prev.map((it, j) => j === idx ? {
          ...it,
          uploading: false,
          url,
          error: url ? '' : (r?.detail || '上传失败'),
        } : it))
        return {
          previewUrl: placeholders[i].previewUrl,
          url,
          uploading: false,
          error: url ? '' : (r?.detail || '上传失败'),
        }
      } catch (e: any) {
        setImages((prev) => prev.map((it, j) => j === idx ? {
          ...it, uploading: false, error: e?.message || '网络错误',
        } : it))
        return {
          previewUrl: placeholders[i].previewUrl,
          url: '',
          uploading: false,
          error: e?.message || '网络错误',
        }
      }
    }))
    const finalImages = [
      ...existingImages,
      ...uploadResults.map((result, i) => ({
        file: toAdd[i],
        ...result,
      })),
    ]
    refreshCategory({
      nextDesc: latestDescRef.current.trim(),
      nextImages: uploadedImageUrls(finalImages),
    }).catch(() => null)
  }

  function removeImage(index: number) {
    const next = images.filter((_, j) => j !== index)
    setImages(next)
    if (!uploadedImageUrls(next).length && !form.desc.trim()) {
      setCategory(null)
      setCategoryError('')
    }
  }
  function onFilePick(e: any) {
    const files = Array.from(e.target.files || []) as File[]
    e.target.value = ''; addFiles(files)
  }
  // POI 交互全部在前面用 inline autocomplete + auto-GPS 实现（applyPoiSuggestion
  // / autoGpsTriedRef），原来的 onPoiInput / runPoi / applyPoi 全删了。

  async function submit() {
    const publishTitle = buildPublishTitle()
    if (!publishTitle || !form.price || !selectedUids.length) return
    if (images.some((i) => i.uploading)) {
      setErr('图片上传中，请稍候再发布')
      return
    }
    setSaving(true)
    setErr('')
    try {
      const badwordsResp: any = await publishBadwords(form.uid, publishTitle).catch(() => null)
      if (badwordsResp && !isBadwordsSafe(badwordsResp)) {
        const matched = extractBadwordsMatches(badwordsResp)
        setErr(matched.length ? `标题包含疑似敏感词：${matched.join('、')}` : publishCheckMessage(badwordsResp, '标题命中敏感词，请调整后重试'))
        return
      }
      const imageUrls = images.filter((i) => i.url && !i.error).map((i) => i.url)
      onBatchPublish({
        selectedUids, form: { ...form, title: publishTitle }, imageUrls,
        cardList: pubPropCards,
        propSelections: pubPropSelections,
        coinEnabled: enableCoinDeduction, coinDeductionPercent,
        grouponEnabled: enableGroupon, grouponAmount,
      })
    } catch (e: any) {
      setErr(String(e?.message || e || '网络错误'))
    } finally {
      setSaving(false)
    }
  }

  const shippingOpts: Array<{ v: 'free'|'freight'|'fixed'|'self'; l: string }> = [
    { v: 'free',    l: '包邮' },
    { v: 'freight', l: '按距离计费' },
    { v: 'fixed',   l: '固定运费' },
    { v: 'self',    l: '无需邮寄' },
  ]
  const locationSummary = locationLabel.trim() || [form.province, form.city, form.area].filter(Boolean).join('')

  return createPortal((
    <div
      className="liquid-scrim fixed inset-0 z-[120] flex items-end justify-center p-0 md:items-center md:p-4"
      onClick={onClose}
    >
      <div
        className="liquid-panel w-full max-w-[620px] overflow-hidden flex flex-col rounded-t-[28px] md:rounded-[26px]"
        style={{ maxHeight: 'calc(100dvh - env(safe-area-inset-top) - 8px)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b shrink-0" style={{ borderColor: 'var(--control-border)' }}>
        <div className="flex items-center gap-2">
          <Plus className="w-4 h-4" style={{ color: 'var(--primary)' }} />
          <span className="text-[14px] font-semibold" style={{ color: 'var(--text-main)' }}>发布商品</span>
            <span className="text-[11px]" style={{ color: 'var(--text-muted)' }}>· 基于描述 / 图片识别分类，AI 描述需手动触发</span>
          </div>
          <button onClick={onClose} className="mobile-icon-button p-1 rounded-full cursor-pointer">
            <X className="w-4 h-4" style={{ color: 'var(--text-muted)' }} />
          </button>
        </div>

        <div className="px-4 py-3 space-y-3 overflow-auto">
          {/* 链接/口令导入 */}
          <div className="flex gap-2">
            <input
              value={importText}
              onChange={(e) => setImportText(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); doImportLink() } }}
              placeholder="粘贴闲鱼商品链接或口令，自动填充信息"
              className="input flex-1 text-[13px]"
            />
            <button
              onClick={doImportLink}
              disabled={!importText.trim() || importing || !form.uid}
              className="btn-ghost btn-sm cursor-pointer disabled:opacity-40"
            >
              {importing ? <RefreshCw className="w-3.5 h-3.5 animate-spin" /> : '导入'}
            </button>
          </div>

          {/* 账号（多选） */}
          <Field label={(
            <div className="flex items-center justify-between">
              <span>发布账号 * · 已选 {selectedUids.length}/{availableAccounts.length || sortedAccounts.length}</span>
              <button
                type="button"
                onClick={toggleAllUids}
                className="text-[11px] font-medium cursor-pointer"
                style={{ color: 'var(--primary)' }}
              >
                {availableAccounts.length > 0 && availableAccounts.every((a: any) => selectedUids.includes(a.uid)) ? '取消全选' : '全选'}
              </button>
            </div>
          )}>
            <div
              className="liquid-list-panel rounded-[14px] overflow-y-auto p-2"
              style={{ maxHeight: 140 }}
            >
              <div className="grid grid-cols-2 md:grid-cols-3 gap-1.5">
                {sortedAccounts.map((a: any) => {
                  const checked = selectedUids.includes(a.uid)
                  const disabled = !availableAccounts.some((entry: any) => entry.uid === a.uid)
                  const isPrimary = a.uid === form.uid
                  const fp = Number(a.security_score) || 0
                  return (
                    <label
                      key={a.uid}
                      className="flex items-center gap-1.5 px-2 py-[5px] rounded-[6px] cursor-pointer transition-colors"
                      style={{
                        background: disabled ? 'var(--surface-soft)' : checked ? 'var(--control-active)' : 'var(--surface-control)',
                        border: `1px solid ${checked ? 'color-mix(in srgb, var(--primary) 22%, transparent)' : 'var(--control-border)'}`,
                        opacity: disabled ? 0.45 : 1,
                      }}
                    >
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() => !disabled && toggleUid(a.uid)}
                        disabled={disabled}
                        className="accent-[var(--primary)] shrink-0"
                      />
                      <span className="text-[12px] truncate flex-1 min-w-0" style={{ color: checked ? 'var(--primary)' : 'var(--text-main)' }}>
                        {a.nick || a.uid}
                      </span>
                      {isPrimary ? (
                        <span className="text-[10px] px-1.5 py-[1px] rounded-[4px] shrink-0" style={{ background: 'var(--primary)', color: 'var(--liquid-highlight)' }}>主</span>
                      ) : checked ? (
                        <button
                          type="button"
                          onClick={(e) => { e.preventDefault(); e.stopPropagation(); patch({ uid: a.uid }) }}
                          className="text-[10px] px-1.5 py-[1px] rounded-[4px] shrink-0 cursor-pointer"
                          style={{ background: 'var(--surface-soft)', color: 'var(--text-muted)', border: '1px solid var(--control-border)' }}
                        >
                          设为主
                        </button>
                      ) : null}
                      {fp > 0 ? (
                        <span
                          className="shrink-0 text-[10px] font-mono tabular-nums"
                          style={{ color: fp >= 700 ? 'var(--success)' : fp >= 500 ? 'var(--warning)' : 'var(--text-muted)' }}
                        >
                          {fp}
                        </span>
                      ) : null}
                    </label>
                  )
                })}
              </div>
            </div>
            <div className="text-[10.5px] mt-1" style={{ color: 'var(--text-muted)' }}>
              主账号用于分类识别和AI描述，地理位置各账号独立
            </div>
          </Field>

          {/* 描述 */}
          <Field label={(
            <div className="flex items-center justify-between gap-2">
              <span>描述 *</span>
              <button
                type="button"
                onClick={generateDescription}
                disabled={descLoading || !canGenerateDescription}
                className="inline-flex items-center gap-1 rounded-[6px] px-2 py-[3px] text-[11px] font-medium cursor-pointer disabled:opacity-40"
                style={{ background: 'var(--surface-soft)', color: 'var(--primary)', border: '1px solid color-mix(in srgb, var(--primary) 18%, transparent)' }}
              >
                {descLoading ? <RefreshCw className="w-3 h-3 animate-spin" /> : <Sparkles className="w-3 h-3" />}
                AI写描述
              </button>
            </div>
          )}>
            <textarea
              value={form.desc}
              maxLength={1000}
              onChange={(e) => updateDesc(e.target.value, 'manual')}
              placeholder="详细描述商品…"
              className="input text-[13px] resize-none"
              style={{ minHeight: 100 }}
            />
            {err ? (
              <div
                className="mt-2 text-[12px] px-2.5 py-1.5 rounded-[12px]"
                style={{ background: 'var(--status-danger-bg)', color: 'var(--status-danger-text)' }}
              >
                {err}
              </div>
            ) : null}
          </Field>

          {/* 价格 */}
          <div className="grid grid-cols-3 gap-2.5">
            <Field label="售价 ¥ *">
              <input
                value={form.price}
                onChange={(e) => patch({ price: e.target.value })}
                inputMode="decimal"
                placeholder="0.00"
                className="input text-[13px] font-mono"
              />
            </Field>
            <Field label="原价 ¥">
              <input
                value={form.origPrice}
                onChange={(e) => patch({ origPrice: e.target.value })}
                inputMode="decimal"
                placeholder="可选"
                className="input text-[13px] font-mono"
              />
            </Field>
            <Field label="库存">
              <input
                value={String(form.quantity)}
                onChange={(e) => patch({ quantity: parseInt(e.target.value || '1', 10) || 1 })}
                inputMode="numeric"
                placeholder="1"
                className="input text-[13px] font-mono"
              />
            </Field>
          </div>

          {/* 图片 */}
          <Field label={`图片 · 最多 ${MAX_IMAGES} 张`}>
            {images.length ? (
              <div className="flex flex-wrap gap-2">
                {images.map((img, i) => (
                  <div key={i} className="flex flex-col items-center">
                    <div
                      className="relative w-20 h-20 rounded-[8px] overflow-hidden border group"
                      style={{ borderColor: img.error ? 'rgba(255,59,48,0.4)' : 'var(--control-border)' }}
                    >
                      <img src={img.previewUrl || img.url} className="w-full h-full object-cover" alt="" />
                      {img.uploading ? (
                        <div className="absolute inset-0 flex items-center justify-center" style={{ background: 'rgba(17,24,39,0.42)', backdropFilter: 'blur(4px)' }}>
                          <RefreshCw className="w-4 h-4 animate-spin text-white" />
                        </div>
                      ) : null}
                      <button
                        onClick={() => removeImage(i)}
                        className="absolute top-1 right-1 w-5 h-5 rounded-full text-white text-[10px] flex items-center justify-center cursor-pointer opacity-0 group-hover:opacity-100 transition-opacity"
                        style={{ background: 'rgba(17,24,39,0.74)' }}
                      >
                        ✕
                      </button>
                    </div>
                    {img.error ? (
                      <input
                        value={img.url || ''}
                        onChange={(e) => setImages((p) => p.map((it, j) => j === i ? { ...it, url: e.target.value, error: e.target.value ? '' : it.error } : it))}
                        placeholder="粘贴 URL"
                        className="input mt-1 w-20 rounded px-1 py-[2px] text-[10px] font-mono"
                        style={{ borderColor: 'rgba(255,59,48,0.4)' }}
                      />
                    ) : null}
                  </div>
                ))}
                {images.length < MAX_IMAGES ? (
                  <label
                    className="w-20 h-20 rounded-[8px] flex items-center justify-center cursor-pointer"
                    style={{ border: '2px dashed var(--control-border)', background: 'rgba(255,255,255,0.45)' }}
                  >
                    <Plus className="w-5 h-5" style={{ color: 'var(--text-muted)' }} />
                    <input type="file" accept="image/*" multiple className="hidden" onChange={onFilePick} />
                  </label>
                ) : null}
              </div>
            ) : (
              <label
                className="flex flex-col items-center justify-center gap-1.5 py-6 rounded-[10px] cursor-pointer"
                style={{ border: '2px dashed var(--control-border)', background: 'rgba(255,255,255,0.45)' }}
              >
                <UploadCloud className="w-6 h-6" style={{ color: 'var(--text-muted)' }} />
                <span className="text-[12px]" style={{ color: 'var(--text-muted)' }}>点击选择图片（最多 {MAX_IMAGES} 张）</span>
                <input type="file" accept="image/*" multiple className="hidden" onChange={onFilePick} />
              </label>
            )}
          </Field>

          <div
            className="rounded-[10px] px-3 py-2.5 flex items-center justify-between gap-3"
            style={{ background: 'var(--surface-soft)', border: '1px solid var(--control-border)' }}
          >
            <div className="min-w-0">
              <div className="text-[11.5px] font-medium" style={{ color: 'var(--text-muted)' }}>自动分类</div>
              <div className="text-[13px] font-semibold truncate mt-0.5" style={{ color: category ? 'var(--text-main)' : 'var(--text-secondary)' }}>
                {categoryLoading ? '识别中…' : category ? categoryLabel(category) : (categoryError || '填写描述或上传图片后识别')}
              </div>
              {category && (category.channelCatId || category.tbCatId) ? (
                <div className="text-[10.5px] font-mono truncate mt-0.5" style={{ color: 'var(--text-muted)' }}>
                  {category.channelCatId || category.tbCatId}
                </div>
              ) : null}
            </div>
            <button
              type="button"
              onClick={() => refreshCategory()}
              disabled={categoryLoading || !canRefreshCategory}
              className="btn-ghost btn-sm cursor-pointer disabled:opacity-40"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${categoryLoading ? 'animate-spin' : ''}`} />
              重新识别
            </button>
          </div>

          {pubPropCards.length > 0 && (
            <div
              className="liquid-muted-card rounded-[16px] px-3 py-2.5"
            >
              <div className="text-[11.5px] font-medium mb-2" style={{ color: 'var(--text-muted)' }}>商品属性</div>
              <div className="flex flex-col gap-2.5">
                {pubPropCards.filter((c: any) => c.propertyId !== '-10000').map((card: any) => (
                  <div key={card.propertyId} className="flex items-center gap-2">
                    <span className="text-[12.5px] shrink-0 w-[68px]" style={{ color: 'var(--text-muted)' }}>
                      {card.propertyName}
                    </span>
                    <div className="flex flex-wrap gap-1.5 flex-1 min-w-0">
                      {card.valuesList.slice(0, 12).map((v: any) => {
                        const vid = String(v.valueId || v.channelCatId || v.catId || '')
                        const sel = String(pubPropSelections[card.propertyId] || '').trim()
                        const selected = sel.length > 0 && vid.length > 0 && sel === vid
                        return (
                          <button
                            key={vid || v.catName || v.text}
                            type="button"
                            onClick={() => {
                              setPubPropSelections((prev) => {
                                const next = { ...prev }
                                if (next[card.propertyId] === vid) {
                                  delete next[card.propertyId]
                                } else {
                                  next[card.propertyId] = vid
                                }
                                return next
                              })
                            }}
                            className="rounded-full px-2.5 py-[3px] text-[11.5px] cursor-pointer transition-colors"
                            style={{
                              background: selected ? 'var(--control-active)' : 'var(--surface-soft)',
                              color: selected ? 'var(--primary)' : 'var(--text-main)',
                              border: selected ? '1px solid color-mix(in srgb, var(--primary) 22%, transparent)' : '1px solid var(--control-border)',
                            }}
                          >
                            {v.catName || v.text || v.valueName || vid}
                          </button>
                        )
                      })}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* 位置信息 —— 打开 modal 时自动定位、自动填省市区。下面的输入框
              允许打字搜索并覆盖本次发布使用的位置。账号默认发布地仅作初始值，
              用户这次手动选的新地点应当立即成为当前生效位置。 */}
          <Field label="位置信息">
            <div className="text-[12px] mb-1" style={{ color: 'var(--text-muted)' }}>
              {locationSummary ? (
                <>
                  <span style={{ color: 'var(--text-main)' }}>{locationSummary}</span>
                  {form.divisionId ? <span className="font-mono ml-1.5">· {form.divisionId}</span> : null}
                  {form.latitude ? <span className="font-mono ml-1.5">· {form.latitude},{form.longitude}</span> : null}
                </>
              ) : (
                <span>未设置发布地，可直接搜索地点：</span>
              )}
            </div>
            <div className="relative">
              <input
                value={poiQuery}
                onChange={(e) => { setPoiQuery(e.target.value); setPoiOpen(true) }}
                onFocus={() => setPoiOpen(true)}
                onBlur={() => window.setTimeout(() => setPoiOpen(false), 150)}
                placeholder="搜索地点（如「武汉洪山」「北京国贸」）"
                className="input text-[13px]"
              />
              {poiBusy ? (
                <RefreshCw className="absolute right-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 animate-spin" style={{ color: 'var(--text-muted)' }} />
              ) : null}
              {poiOpen && poiSuggestions.length > 0 ? (
                <div className="liquid-list-panel absolute left-0 right-0 top-full mt-1 rounded-[14px] overflow-hidden max-h-56 overflow-y-auto z-10">
                  {poiSuggestions.map((p: any, i: number) => (
                    <button
                      key={`${p.name}-${i}`}
                      type="button"
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => applyPoiSuggestion(p)}
                      className="liquid-list-row w-full text-left px-2.5 py-1.5 cursor-pointer"
                      style={{ borderBottom: '1px solid var(--control-border)' }}
                    >
                      <div className="text-[12.5px] font-medium truncate" style={{ color: 'var(--text-main)' }}>{p.name}</div>
                      {p.address ? <div className="text-[11px] truncate" style={{ color: 'var(--text-muted)' }}>{p.address}</div> : null}
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
          </Field>

          {/* 选项 */}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
            <div className="inline-flex items-center gap-1.5 text-[12.5px]" style={{ color: 'var(--text-main)' }}>
              <span style={{ color: 'var(--text-muted)' }}>商品类型</span>
              <span
                className="rounded-full px-2 py-[3px] text-[11.5px] font-medium"
                style={{
                  background: form.simpleItem ? 'var(--primary-light)' : 'var(--surface-soft)',
                  color: form.simpleItem ? 'var(--primary)' : 'var(--text-main)',
                  border: `1px solid ${form.simpleItem ? 'color-mix(in srgb, var(--primary) 18%, transparent)' : 'var(--control-border)'}`,
                }}
              >
                {form.simpleItem ? '虚拟/服务类' : '普通实物'}
              </span>
            </div>
            <label className="flex items-center gap-1.5 text-[12.5px] cursor-pointer" style={{ color: 'var(--text-main)' }}>
              <input type="checkbox" checked={form.noBargain} onChange={(e) => patch({ noBargain: e.target.checked })} />
              禁止议价
            </label>
            <label className="flex items-center gap-1.5 text-[12.5px] cursor-pointer" style={{ color: 'var(--primary)' }}>
              <input type="checkbox" checked={enableCoinDeduction} onChange={(e) => setEnableCoinDeduction(e.target.checked)} className="accent-[var(--primary)]" />
              闲鱼币抵扣
            </label>
            {enableCoinDeduction && (
              <select
                value={coinDeductionPercent}
                onChange={(e) => setCoinDeductionPercent(e.target.value)}
                className="rounded-[6px] px-1.5 py-0.5 text-[12px]"
                style={{ border: '1px solid var(--primary)', color: 'var(--primary)', background: 'var(--primary-light)' }}
              >
                <option value="3">3%</option>
                <option value="5">5%</option>
                <option value="8">8%</option>
                <option value="10">10%</option>
                <option value="15">15%</option>
                <option value="20">20%</option>
              </select>
            )}
            <label className="flex items-center gap-1.5 text-[12.5px] cursor-pointer" style={{ color: 'var(--primary)' }}>
              <input type="checkbox" checked={enableGroupon} onChange={(e) => setEnableGroupon(e.target.checked)} className="accent-[var(--primary)]" />
              2人小刀
            </label>
            {enableGroupon && (
              <div className="flex items-center gap-1">
                <span className="text-[12px]" style={{ color: 'var(--primary)' }}>¥</span>
                <input
                  value={grouponAmount}
                  onChange={(e) => setGrouponAmount(e.target.value.replace(/[^\d.]/g, ''))}
                  placeholder="自动"
                  className="w-[52px] rounded-[6px] px-1.5 py-0.5 text-[12px] text-center"
                  style={{ border: '1px solid var(--primary)', color: 'var(--primary)', background: 'var(--primary-light)' }}
                />
              </div>
            )}
          </div>

          {/* 发货 */}
          <div>
            <div className="text-[11.5px] mb-1" style={{ color: 'var(--text-muted)' }}>发货方式</div>
            <div className="grid grid-cols-4 gap-2">
              {shippingOpts.map((opt) => (
                <button
                  key={opt.v}
                  onClick={() => patch({ shippingType: opt.v })}
                  className="py-[6px] px-2 rounded-[8px] text-[12.5px] font-medium cursor-pointer transition-colors"
                  style={{
                    background: form.shippingType === opt.v ? 'var(--primary)' : 'var(--surface-control)',
                    color: form.shippingType === opt.v ? 'var(--liquid-highlight)' : 'var(--text-main)',
                    border: `1px solid ${form.shippingType === opt.v ? 'var(--primary)' : 'var(--control-border)'}`,
                  }}
                >
                  {opt.l}
                </button>
              ))}
            </div>
            {form.shippingType === 'fixed' ? (
              <div className="mt-2 flex items-center gap-2">
                <span className="text-[12.5px]" style={{ color: 'var(--text-muted)' }}>邮费 ¥</span>
                <input
                  value={form.postPrice}
                  onChange={(e) => patch({ postPrice: e.target.value })}
                  inputMode="decimal"
                  placeholder="0.00"
                  className="input flex-1 text-[13px] font-mono"
                />
              </div>
            ) : null}
          </div>

        </div>

        <div
          className="flex items-center justify-end gap-2 px-4 pt-3 border-t shrink-0"
          style={{
            borderColor: 'var(--control-border)',
            background: 'rgba(255,255,255,0.72)',
            paddingBottom: 'calc(14px + env(safe-area-inset-bottom))',
          }}
        >
          <button onClick={onClose} className="btn-ghost btn-sm cursor-pointer">取消</button>
          <button
            onClick={submit}
            disabled={saving || !resolvedPublishTitle || !form.price || !selectedUids.length}
            className="btn-primary btn-sm cursor-pointer disabled:opacity-40"
          >
            {saving
              ? (<><RefreshCw className="w-3.5 h-3.5 animate-spin" /> 检查中…</>)
              : selectedUids.length > 1
                ? `发布到 ${selectedUids.length} 个账号`
                : '发布'
            }
          </button>
        </div>
      </div>
    </div>
  ), document.body)
}
