// @ts-nocheck
import { useState, useEffect, useRef } from 'react'
import { ChevronLeft, ChevronRight, Plus, X, Loader2, Link2, MapPin, Sparkles, RefreshCw } from 'lucide-react'
import { BottomSheet } from '../../../components/Modal'
import {
  itemPublish, itemImportLink, uploadItemImage, itemAiDescription, itemKgraph,
  isSuccess, retMsg, request, locationByGps, applyPostPublishExtras,
  publishCheck, publishBadwords, refreshAccount,
} from '../../../api/index'
import { colorForAccount } from '../itemUtils'
import {
  categoryLabel,
  derivePublishTitle,
  extractAiDesc,
  extractAiTitle,
  extractBadwordsMatches,
  isBadwordsSafe,
  isPublishAllowed,
  pickKgraphCardList,
  pickKgraphCategory,
  publishCheckMessage,
  resolveSimpleItemFlag,
} from '../publishUtils'
import { filterBusinessAccounts } from '../../../utils/accountAccess'
import { emitActionFailure, emitActionLog, emitActionStarted, emitActionSuccess } from '../../../bootstrap/actionLogging'

interface PublishWizardSheetProps {
  open: boolean
  accounts: any[]
  onClose: () => void
  onPublished: () => void
  notify: (message: string, type?: 'success' | 'error' | 'info', duration?: number) => void
  onBatchPublish?: (params: any) => Promise<void>
}

interface PublishForm {
  uid: string
  title: string
  desc: string
  price: string
  origPrice: string
  quantity: number
  simpleItem: boolean
  noBargain: boolean
  shippingType: 'free' | 'freight' | 'fixed' | 'self'
  postPrice: string
  imageUrls: string[]
  province: string
  city: string
  area: string
  divisionId: string
  latitude: string
  longitude: string
}

async function ensurePublishable(uid: string) {
  let checkResp: any = await publishCheck(uid).catch(() => null)
  if (checkResp && isPublishAllowed(checkResp)) return { ok: true, checkResp }

  const sessionReason = String(
    checkResp?.data?.message
    || checkResp?.data?.detail
    || checkResp?.detail
    || checkResp?.ret?.[0]
    || '',
  ).toUpperCase()

  if (
    sessionReason.includes('SESSION_EXPIRED')
    || sessionReason.includes('NOT_LOGIN')
    || sessionReason.includes('TOKEN')
  ) {
    await refreshAccount(uid).catch(() => null)
    checkResp = await publishCheck(uid).catch(() => null)
  }

  return { ok: !!(checkResp && isPublishAllowed(checkResp)), checkResp }
}

function publishFormSummary(form: PublishForm) {
  return {
    uid: form.uid,
    descLength: String(form.desc || '').trim().length,
    imageCount: form.imageUrls.length,
    hasPrice: !!String(form.price || '').trim(),
    hasLocation: !![form.province, form.city, form.area].filter(Boolean).join(' '),
    shippingType: form.shippingType,
  }
}

function logPublishValidation(action: string, message: string, form: PublishForm, context: Record<string, unknown> = {}) {
  emitActionLog({
    module: 'items-publish',
    action,
    outcome: 'warn',
    message,
    retryable: false,
    promoteToIncident: false,
    context: {
      ...publishFormSummary(form),
      ...context,
    },
  })
}

function emptyForm(uid = ''): PublishForm {
  return {
    uid, title: '', desc: '', price: '', origPrice: '',
    quantity: 1, simpleItem: false, noBargain: false,
    shippingType: 'free', postPrice: '',
    imageUrls: [], province: '', city: '', area: '',
    divisionId: '', latitude: '', longitude: '',
  }
}

export function PublishWizardSheet({ open, accounts, onClose, onPublished, notify, onBatchPublish }: PublishWizardSheetProps) {
  const activeAccounts = filterBusinessAccounts(accounts)
  const [step, setStep] = useState(0)
  const [form, setForm] = useState<PublishForm>(() => emptyForm(activeAccounts[0]?.uid || ''))
  const [busy, setBusy] = useState(false)
  const [selectedUids, setSelectedUids] = useState<string[]>(() => activeAccounts.length ? [activeAccounts[0].uid] : [])
  const [importText, setImportText] = useState('')
  const [importing, setImporting] = useState(false)
  const [aiBusy, setAiBusy] = useState(false)
  const [category, setCategory] = useState<any>(null)
  const [categoryLoading, setCategoryLoading] = useState(false)
  const [categoryError, setCategoryError] = useState('')
  const [propCards, setPropCards] = useState<any[]>([])
  const [propSelections, setPropSelections] = useState<Record<string, string>>({})
  const [enableCoinDeduction, setEnableCoinDeduction] = useState(true)
  const [coinDeductionPercent, setCoinDeductionPercent] = useState('5')
  const [enableGroupon, setEnableGroupon] = useState(true)
  const [grouponAmount, setGrouponAmount] = useState('')
  const fileInputRef = useRef<HTMLInputElement>(null)
  const simpleItemExplicitRef = useRef<boolean | null>(null)
  // 位置：inline autocomplete 而不是弹窗。打字 → 防抖 → 高德 inputtips → 下拉。
  const [poiQuery, setPoiQuery] = useState('')
  const [poiSuggestions, setPoiSuggestions] = useState<any[]>([])
  const [poiBusy, setPoiBusy] = useState(false)
  const poiReqRef = useRef(0)
  const [locationLabel, setLocationLabel] = useState('')

  useEffect(() => {
    if (open) {
      setStep(0)
      setForm(emptyForm(activeAccounts[0]?.uid || ''))
      setSelectedUids(activeAccounts.length ? [activeAccounts[0].uid] : [])
      setImportText('')
      setPoiQuery('')
      setPoiSuggestions([])
      setLocationLabel('')
      setCategory(null)
      setCategoryError('')
      setPropCards([])
      setPropSelections({})
      simpleItemExplicitRef.current = null
      setEnableCoinDeduction(true)
      setCoinDeductionPercent('5')
      setEnableGroupon(true)
      setGrouponAmount('')
    }
  }, [open])

  // POI autocomplete 防抖
  useEffect(() => {
    const kw = poiQuery.trim()
    if (!kw) { setPoiSuggestions([]); setPoiBusy(false); return }
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
      // 反查失败时保留搜索接口带回的近似位置字段。
    }
    setForm((f) => ({
      ...f,
      province,
      city,
      area,
      divisionId,
      latitude,
      longitude,
    }))
    setLocationLabel(String(p.name || ''))
    setPoiQuery(String(p.name || ''))
    setPoiSuggestions([])
  }

  // 切换账号时自动填充该账号的常用地区
  useEffect(() => {
    if (!form.uid) return
    const acc = accounts.find((a) => a.uid === form.uid)
    if (acc?.location && !form.province) {
      setLocationLabel('')
      setPoiQuery('')
      setForm((f) => ({
        ...f,
        province: acc.location.province || '',
        city: acc.location.city || '',
        area: acc.location.area || '',
        divisionId: acc.location.divisionId || '',
      }))
    }
  }, [form.uid])

  useEffect(() => {
    if (simpleItemExplicitRef.current !== null) return
    const inferred = resolveSimpleItemFlag(null, category, {
      title: form.title,
      desc: form.desc,
    })
    setForm((current) => (current.simpleItem === inferred ? current : {
      ...current,
      simpleItem: inferred,
    }))
  }, [category, form.title, form.desc])

  function update<K extends keyof PublishForm>(k: K, v: PublishForm[K]) {
    setForm((f) => ({ ...f, [k]: v }))
  }
  const locationSummary = locationLabel.trim() || [form.province, form.city, form.area].filter(Boolean).join(' ')

  async function refreshCategory({
    nextDesc = form.desc,
    nextImages = form.imageUrls,
  }: {
    nextDesc?: string
    nextImages?: string[]
  } = {}) {
    if (!form.uid) return null
    const normalizedDesc = String(nextDesc || '').trim()
    const normalizedImages = [...(nextImages || [])].filter(Boolean)
    if (!normalizedDesc && normalizedImages.length === 0) return null
    setCategoryLoading(true)
    setCategoryError('')
    emitActionStarted({
      module: 'items-publish',
      action: 'refresh_publish_category',
      message: '开始识别发布商品分类',
      context: {
        uid: form.uid,
        descLength: normalizedDesc.length,
        imageCount: normalizedImages.length,
      },
    })
    try {
      const resp: any = await itemKgraph(form.uid, {
        title: '',
        desc: normalizedDesc,
        imageUrls: normalizedImages,
      })
      const nextCategory = pickKgraphCategory(resp)
      const cards = pickKgraphCardList(resp)
      const nextSelections: Record<string, string> = {}
      for (const card of cards) {
        const clicked = (card?.valuesList || []).find((value: any) => value?.isClicked === '1')
        if (!clicked) continue
        const valueId = String(clicked.valueId || clicked.channelCatId || clicked.catId || '').trim()
        if (valueId) nextSelections[String(card.propertyId || '')] = valueId
      }
      if (nextCategory && categoryLabel(nextCategory)) {
        setCategory(nextCategory)
        if (simpleItemExplicitRef.current === null) {
          setForm((current) => ({
            ...current,
            simpleItem: resolveSimpleItemFlag(null, nextCategory, {
              title: current.title,
              desc: normalizedDesc,
            }),
          }))
        }
      } else {
        setCategory(null)
        setCategoryError('未识别到分类')
        if (simpleItemExplicitRef.current === null) {
          setForm((current) => ({
            ...current,
            simpleItem: resolveSimpleItemFlag(null, null, {
              title: current.title,
              desc: normalizedDesc,
            }),
          }))
        }
      }
      setPropCards(cards)
      setPropSelections(nextSelections)
      emitActionSuccess({
        module: 'items-publish',
        action: 'refresh_publish_category',
        message: '发布商品分类识别成功',
        context: {
          uid: form.uid,
          hasCategory: !!(nextCategory && categoryLabel(nextCategory)),
          cardCount: cards.length,
          selectionCount: Object.keys(nextSelections).length,
        },
      })
      return {
        category: nextCategory && categoryLabel(nextCategory) ? nextCategory : null,
        cardList: cards,
        propSelections: nextSelections,
      }
    } catch (error: any) {
      setCategory(null)
      setPropCards([])
      setPropSelections({})
      setCategoryError(error?.message ? `分类识别失败：${error.message}` : '分类识别失败')
      emitActionFailure({
        module: 'items-publish',
        action: 'refresh_publish_category',
        message: '发布商品分类识别失败',
        error,
        context: {
          uid: form.uid,
          descLength: normalizedDesc.length,
          imageCount: normalizedImages.length,
        },
      })
      return null
    } finally {
      setCategoryLoading(false)
    }
  }

  async function handleImport() {
    if (!importText.trim()) return
    if (!selectedUids.length) {
      notify('请先选择账号', 'error')
      return
    }
    setImporting(true)
    const sourceText = importText.trim()
    emitActionStarted({
      module: 'items-publish',
      action: 'import_item_link',
      message: '开始通过链接导入商品信息',
      context: {
        uid: form.uid,
        inputLength: sourceText.length,
        urlCount: (sourceText.match(/https?:\/\//g) || []).length,
      },
    })
    try {
      const r = await itemImportLink(form.uid || selectedUids[0], sourceText)
      // import_link 后端返回的是自定义格式（title/desc/price/imageUrls 在顶层），
      // 没有 MTOP 风格的 ret 字段，所以不能靠 isSuccess 判断。
      const data = r?.data ?? r ?? {}
      const hasImportedContent = !!(data?.title || data?.desc || (Array.isArray(data?.imageUrls) && data.imageUrls.length))
      if (hasImportedContent) {
        const nextDesc = data.desc || data.description || form.desc
        const nextImageUrls = Array.isArray(data.imageUrls) ? data.imageUrls : (Array.isArray(data.images) ? data.images : form.imageUrls)
        simpleItemExplicitRef.current = data.simpleItem !== undefined ? !!data.simpleItem : null
        setForm((f) => ({
          ...f,
          title: data.title || f.title,
          desc: nextDesc,
          price: String(data.price || f.price),
          simpleItem: resolveSimpleItemFlag(data.simpleItem, category, {
            title: data.title || f.title,
            desc: nextDesc,
          }),
          noBargain: data.noBargain !== undefined ? !!data.noBargain : f.noBargain,
          imageUrls: nextImageUrls,
        }))
        setImportText('')
        void refreshCategory({ nextDesc, nextImages: nextImageUrls })
        notify('已导入商品信息', 'success')
        emitActionSuccess({
          module: 'items-publish',
          action: 'import_item_link',
          message: '链接导入商品信息成功',
          context: {
            uid: form.uid,
            inputLength: sourceText.length,
            urlCount: (sourceText.match(/https?:\/\//g) || []).length,
            hasTitle: !!data.title,
            descLength: String(nextDesc || '').length,
            imageCount: nextImageUrls.length,
          },
        })
      } else {
        emitActionLog({
          module: 'items-publish',
          action: 'import_item_link',
          outcome: 'warn',
          message: '链接导入商品信息未成功',
          detail: retMsg(r, '导入失败'),
          retryable: true,
          promoteToIncident: false,
          context: {
            uid: form.uid,
            inputLength: sourceText.length,
            urlCount: (sourceText.match(/https?:\/\//g) || []).length,
          },
        })
        notify(retMsg(r, '导入失败'), 'error')
      }
    } catch (e) {
      emitActionFailure({
        module: 'items-publish',
        action: 'import_item_link',
        message: '链接导入商品信息失败',
        error: e,
        context: {
          uid: form.uid,
          inputLength: sourceText.length,
          urlCount: (sourceText.match(/https?:\/\//g) || []).length,
        },
      })
      notify(String(e?.message || e), 'error')
    } finally {
      setImporting(false)
    }
  }

  async function handleUpload(files: FileList | null) {
    if (!files || !form.uid) return
    const remaining = 9 - form.imageUrls.length
    const toUpload = Array.from(files).slice(0, remaining)
    const nextImageUrls = [...form.imageUrls]
    emitActionStarted({
      module: 'items-publish',
      action: 'upload_publish_images',
      message: '开始上传发布商品图片',
      context: {
        uid: form.uid,
        fileCount: toUpload.length,
        existingImageCount: form.imageUrls.length,
        totalFileSizeBytes: toUpload.reduce((sum, file) => sum + file.size, 0),
      },
    })
    let ok = 0
    let fail = 0
    for (const file of toUpload) {
      try {
        const r = await uploadItemImage(form.uid, file)
        const url = r?.data?.url || r?.data?.imageUrl || r?.url
        if (url) {
          ok += 1
          nextImageUrls.push(url)
          setForm((f) => ({ ...f, imageUrls: [...f.imageUrls, url] }))
        } else {
          fail += 1
          notify(retMsg(r, '上传失败'), 'error')
        }
      } catch (e) {
        fail += 1
        notify(String(e?.message || e), 'error')
      }
    }
    if (nextImageUrls.length > 0) {
      void refreshCategory({ nextDesc: form.desc, nextImages: nextImageUrls })
    }
    emitActionLog({
      module: 'items-publish',
      action: 'upload_publish_images',
      outcome: fail > 0 ? 'warn' : 'success',
      message: fail > 0 ? '发布商品图片部分上传失败' : '发布商品图片上传成功',
      retryable: fail > 0,
      promoteToIncident: false,
      context: {
        uid: form.uid,
        fileCount: toUpload.length,
        successCount: ok,
        failureCount: fail,
        finalImageCount: nextImageUrls.length,
      },
    })
  }

  function removeImage(i: number) {
    const nextImageUrls = form.imageUrls.filter((_, idx) => idx !== i)
    setForm((f) => ({ ...f, imageUrls: nextImageUrls }))
    if (!nextImageUrls.length && !form.desc.trim()) {
      setCategory(null)
      setCategoryError('')
      setPropCards([])
      setPropSelections({})
      return
    }
    void refreshCategory({ nextDesc: form.desc, nextImages: nextImageUrls })
  }

  // POI 选择走全局 LocationPicker（openMobileLocationPicker）。

  async function handleSubmit() {
    if (!selectedUids.length) { notify('请选择账号', 'error'); return }
    if (!form.desc.trim()) { notify('请输入商品描述', 'error'); return }
    if (!form.price || Number(form.price) <= 0) { notify('请输入价格', 'error'); return }
    if (form.imageUrls.length === 0) { notify('请至少上传 1 张图片', 'error'); return }

    // Ensure primary uid is set for pre-checks
    if (!form.uid && selectedUids.length) update('uid', selectedUids[0])

    setBusy(true)
    emitActionStarted({
      module: 'items-publish',
      action: 'publish_item',
      message: '开始发布商品',
      context: publishFormSummary(form),
    })
    try {
      const publishTitle = derivePublishTitle(form.title, form.desc) || '闲置商品'
      const publishable = await ensurePublishable(form.uid)
      if (publishable.checkResp && !publishable.ok) {
        emitActionLog({
          module: 'items-publish',
          action: 'publish_item',
          outcome: 'warn',
          message: '发布商品前置校验未通过',
          detail: publishCheckMessage(publishable.checkResp, '暂不可发布'),
          retryable: true,
          promoteToIncident: false,
          context: publishFormSummary(form),
        })
        notify(publishCheckMessage(publishable.checkResp, '暂不可发布'), 'error')
        return
      }
      const badwordsResp: any = await publishBadwords(form.uid, publishTitle).catch(() => null)
      if (badwordsResp && !isBadwordsSafe(badwordsResp)) {
        const matched = extractBadwordsMatches(badwordsResp)
        emitActionLog({
          module: 'items-publish',
          action: 'publish_item',
          outcome: 'warn',
          message: '发布商品标题命中敏感词校验',
          retryable: false,
          promoteToIncident: false,
          context: {
            ...publishFormSummary(form),
            matchedCount: matched.length,
          },
        })
        notify(
          matched.length
            ? `标题包含疑似敏感词：${matched.join('、')}`
            : publishCheckMessage(badwordsResp, '标题命中敏感词，请调整后重试'),
          'error',
        )
        return
      }
      if (onBatchPublish && selectedUids.length > 0) {
        await onBatchPublish({
          selectedUids,
          form: { ...form, title: publishTitle },
          imageUrls: form.imageUrls,
          cardList: propCards,
          propSelections,
          coinEnabled: enableCoinDeduction,
          coinDeductionPercent,
          grouponEnabled: enableGroupon,
          grouponAmount,
        })
        emitActionSuccess({
          module: 'items-publish',
          action: 'publish_item',
          message: `批量发布到 ${selectedUids.length} 个账号`,
          context: { ...publishFormSummary(form), accountCount: selectedUids.length },
        })
        onClose()
        onPublished()
      } else {
        const r = await itemPublish(form.uid, {
          title: publishTitle,
          desc: form.desc.trim(),
          price: form.price,
          origPrice: form.origPrice || form.price,
          imageUrls: form.imageUrls,
          simpleItem: form.simpleItem,
          quantity: form.quantity || 1,
          noBargain: form.noBargain,
          shippingType: form.shippingType,
          postPrice: form.shippingType === 'fixed' ? form.postPrice : '',
          canFreeShipping: form.shippingType === 'free',
          cardList: propCards,
          propSelections,
          province: form.province,
          city: form.city,
          area: form.area,
          divisionId: form.divisionId,
          latitude: form.latitude,
          longitude: form.longitude,
        })
        if (isSuccess(r)) {
          const extraRun = await applyPostPublishExtras({
            uid: form.uid,
            publishResponse: r,
            price: String(form.price || '100'),
            coinEnabled: enableCoinDeduction,
            preferredCoinPercent: parseInt(coinDeductionPercent, 10) || 5,
            grouponEnabled: enableGroupon,
            grouponAmount,
            grouponDelayMs: 2000,
          })
          const extras = extraRun.results
            .map((entry: any) => entry.ok ? `${entry.label}✓` : entry.label)
            .join(' ')
          notify(extras ? `发布成功 (${extras})` : '发布成功', 'success')
          emitActionSuccess({
            module: 'items-publish',
            action: 'publish_item',
            message: '商品发布成功',
            context: {
              ...publishFormSummary(form),
              extraCount: extraRun.results.length,
              extraSuccessCount: extraRun.results.filter((entry: any) => entry.ok).length,
            },
          })
          onClose()
          onPublished()
        } else {
          emitActionLog({
            module: 'items-publish',
            action: 'publish_item',
            outcome: 'warn',
            message: '商品发布未成功',
            detail: retMsg(r, '发布失败'),
            retryable: true,
            promoteToIncident: false,
            context: publishFormSummary(form),
          })
          notify(retMsg(r, '发布失败'), 'error')
        }
      }
    } catch (e) {
      emitActionFailure({
        module: 'items-publish',
        action: 'publish_item',
        message: '商品发布失败',
        error: e,
        context: publishFormSummary(form),
      })
      notify(String(e?.message || e), 'error')
    } finally {
      setBusy(false)
    }
  }

  async function handleAiDescription() {
    if (!form.uid) {
      logPublishValidation('generate_ai_description', '生成 AI 描述前未选择账号', form)
      notify('请先选择账号', 'error')
      return
    }
    const desc = form.desc.trim()
    if (!desc && form.imageUrls.length === 0) {
      logPublishValidation('generate_ai_description', '生成 AI 描述缺少输入内容', form)
      notify('请先输入商品描述或上传图片', 'error')
      return
    }
    setAiBusy(true)
    emitActionStarted({
      module: 'items-publish',
      action: 'generate_ai_description',
      message: '开始生成 AI 商品描述',
      context: publishFormSummary(form),
    })
    try {
      const refreshed = await refreshCategory({
        nextDesc: desc,
        nextImages: form.imageUrls,
      })
      const r: any = await itemAiDescription(form.uid, {
        title: '',
        desc,
        category: refreshed?.category ?? category ?? null,
        imageUrls: form.imageUrls,
        cardList: refreshed?.cardList ?? propCards,
        propSelections: refreshed?.propSelections ?? propSelections,
      })
      const nextDesc = extractAiDesc(r)
      const nextTitle = extractAiTitle(r)
      if (!nextDesc) {
        emitActionLog({
          module: 'items-publish',
          action: 'generate_ai_description',
          outcome: 'warn',
          message: 'AI 商品描述未返回有效内容',
          detail: retMsg(r, 'AI 描述生成失败'),
          retryable: true,
          promoteToIncident: false,
          context: publishFormSummary(form),
        })
        notify(retMsg(r, 'AI 描述生成失败'), 'error')
        return
      }
      setForm((f) => ({
        ...f,
        desc: nextDesc,
        title: nextTitle || derivePublishTitle(f.title, nextDesc),
      }))
      notify('AI 描述已更新', 'success')
      emitActionSuccess({
        module: 'items-publish',
        action: 'generate_ai_description',
        message: 'AI 商品描述生成成功',
        context: {
          ...publishFormSummary(form),
          generatedDescLength: nextDesc.length,
          hasGeneratedTitle: !!nextTitle,
        },
      })
    } catch (e: any) {
      emitActionFailure({
        module: 'items-publish',
        action: 'generate_ai_description',
        message: 'AI 商品描述生成失败',
        error: e,
        context: publishFormSummary(form),
      })
      notify(String(e?.message || e), 'error')
    } finally {
      setAiBusy(false)
    }
  }

  const stepLabels = ['商品信息', '价格库存', '发布设置']
  const canPrev = step > 0
  const canNext = step < 2

  function nextStep() {
    if (step === 0) {
      if (!form.uid) { notify('请选择账号', 'error'); return }
      if (!form.desc.trim()) { notify('请输入商品描述', 'error'); return }
      if (form.imageUrls.length === 0) { notify('请至少上传 1 张图片', 'error'); return }
    }
    if (step === 1) {
      if (!form.price || Number(form.price) <= 0) { notify('请输入价格', 'error'); return }
    }
    if (step < 2) setStep(step + 1)
  }

  return (
    <BottomSheet
      open={open}
      onClose={onClose}
      title={`发布商品 - ${stepLabels[step]} (${step + 1}/3)`}
      footer={
        <>
          {canPrev && (
            <button className="btn-secondary flex-1" onClick={() => setStep(step - 1)} disabled={busy}>
              <ChevronLeft className="w-4 h-4" /> 上一步
            </button>
          )}
          {canNext ? (
            <button className="btn-primary flex-1" onClick={nextStep} disabled={busy}>
              下一步 <ChevronRight className="w-4 h-4" />
            </button>
          ) : (
            <button className="btn-primary flex-1" onClick={handleSubmit} disabled={busy}>
              {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : '确认发布'}
            </button>
          )}
        </>
      }
    >
      {/* 步骤指示器 */}
      <div className="flex items-center gap-2 mb-4 px-1">
        {stepLabels.map((label, i) => (
          <div key={i} className="flex-1 flex flex-col items-center gap-1">
            <div
              className="w-7 h-7 rounded-full flex items-center justify-center text-[12px] font-semibold transition-colors"
              style={{
                backgroundColor: i <= step ? 'var(--primary)' : 'var(--surface-sunken)',
                color: i <= step ? 'var(--liquid-highlight)' : 'var(--text-muted)',
              }}
            >
              {i + 1}
            </div>
            <span className="text-[10px]" style={{ color: i === step ? 'var(--primary)' : 'var(--text-muted)' }}>
              {label}
            </span>
          </div>
        ))}
      </div>

      {/* Step 1: 基本信息 */}
      {step === 0 && (
        <div className="space-y-4">
          <div>
            <div className="flex items-center justify-between">
              <label className="label">发布账号 · 已选 {selectedUids.length}/{activeAccounts.length}</label>
              <button
                type="button"
                onClick={() => {
                  if (activeAccounts.length && activeAccounts.every((a: any) => selectedUids.includes(a.uid))) {
                    setSelectedUids([])
                  } else {
                    setSelectedUids(activeAccounts.map((a: any) => a.uid))
                  }
                }}
                className="text-[11px] font-medium"
                style={{ color: 'var(--primary)' }}
              >
                {activeAccounts.length > 0 && activeAccounts.every((a: any) => selectedUids.includes(a.uid)) ? '取消全选' : '全选'}
              </button>
            </div>
            <div className="grid grid-cols-2 gap-2">
              {activeAccounts.map((a) => {
                const checked = selectedUids.includes(a.uid)
                return (
                  <button
                    key={a.uid}
                    onClick={() => {
                      setSelectedUids((prev) => prev.includes(a.uid) ? prev.filter((u) => u !== a.uid) : [...prev, a.uid])
                      if (!form.uid) update('uid', a.uid)
                      if (!selectedUids.length) update('uid', a.uid)
                    }}
                    className="flex items-center gap-2 p-2 rounded-xl text-left transition-colors"
                    style={{
                      backgroundColor: checked ? 'var(--primary-light)' : 'var(--surface-soft)',
                      border: `2px solid ${checked ? 'var(--primary)' : 'transparent'}`,
                    }}
                  >
                    <div className="w-2 h-7 rounded-full shrink-0" style={{ backgroundColor: colorForAccount(a.uid) }} />
                    <span className="text-[13px] font-medium truncate flex-1 min-w-0" style={{ color: checked ? 'var(--primary)' : 'var(--text-main)' }}>
                      {a.nick || a.uid}
                    </span>
                    {checked && <span className="text-[10px] shrink-0" style={{ color: 'var(--primary)' }}>✓</span>}
                  </button>
                )
              })}
            </div>
          </div>

          <div className="rounded-xl p-3" style={{ backgroundColor: 'var(--surface-soft)' }}>
            <label className="label-hint flex items-center gap-1.5"><Link2 className="w-3.5 h-3.5" /> 从链接导入（可选）</label>
            <div className="flex gap-2 mt-2">
              <input
                className="input flex-1"
                placeholder="粘贴闲鱼/淘宝商品链接或口令"
                value={importText}
                onChange={(e) => setImportText(e.target.value)}
              />
              <button className="btn-secondary px-3" onClick={handleImport} disabled={importing || !importText.trim()}>
                {importing ? <Loader2 className="w-4 h-4 animate-spin" /> : '导入'}
              </button>
            </div>
          </div>

          <div>
            <div className="flex items-center justify-between gap-3">
              <label className="label">商品描述 <span style={{ color: 'var(--danger)' }}>*</span></label>
              <button
                type="button"
                className="btn-secondary h-8 px-3 shrink-0"
                onClick={handleAiDescription}
                disabled={aiBusy || (!form.desc.trim() && form.imageUrls.length === 0)}
              >
                {aiBusy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Sparkles className="w-3.5 h-3.5" />}
                AI写描述
              </button>
            </div>
            <textarea
              className="input"
              rows={4}
              placeholder="直接输入商品描述，发布时会自动生成标题"
              value={form.desc}
              onChange={(e) => update('desc', e.target.value)}
            />
            <p className="text-[11px] mt-1" style={{ color: 'var(--text-muted)' }}>
              可先写已有描述，再用 AI 基于当前描述和已上传图片继续完善。
            </p>
          </div>

          <div>
            <label className="label">商品图片 <span style={{ color: 'var(--danger)' }}>*</span> ({form.imageUrls.length}/9)</label>
            <div className="grid grid-cols-3 gap-2">
              {form.imageUrls.map((url, i) => (
                <div key={i} className="relative aspect-square rounded-xl overflow-hidden" style={{ backgroundColor: 'var(--surface-soft)' }}>
                  <img src={url} alt="" className="w-full h-full object-cover" />
                  <button
                    onClick={() => removeImage(i)}
                    className="absolute top-1 right-1 w-6 h-6 rounded-full bg-black/60 flex items-center justify-center"
                  >
                    <X className="w-3.5 h-3.5 text-white" />
                  </button>
                </div>
              ))}
              {form.imageUrls.length < 9 && (
                <button
                  onClick={() => fileInputRef.current?.click()}
                  className="aspect-square rounded-xl flex flex-col items-center justify-center gap-1 border-2 border-dashed cursor-pointer"
                  style={{ borderColor: 'var(--border-light)', backgroundColor: 'var(--surface-soft)', color: 'var(--text-secondary)' }}
                >
                  <Plus className="w-5 h-5" />
                  <span className="text-[11px]">添加</span>
                </button>
              )}
            </div>
            <input
              ref={fileInputRef}
              type="file"
              accept="image/*"
              multiple
              hidden
              onChange={(e) => handleUpload(e.target.files)}
            />
          </div>

          <div
            className="rounded-xl p-3"
            style={{ backgroundColor: 'var(--surface-soft)' }}
          >
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>自动分类</div>
                <div className="mt-1 text-[14px] font-semibold truncate" style={{ color: category ? 'var(--text-main)' : 'var(--text-secondary)' }}>
                  {categoryLoading ? '识别中…' : category ? categoryLabel(category) : (categoryError || '上传图片或填写描述后识别')}
                </div>
              </div>
              <button
                type="button"
                onClick={() => { void refreshCategory() }}
                disabled={categoryLoading || (!form.desc.trim() && form.imageUrls.length === 0)}
                className="btn-secondary h-8 px-3 shrink-0 disabled:opacity-40"
              >
                {categoryLoading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
                识别
              </button>
            </div>
          </div>

          {propCards.filter((card: any) => String(card?.propertyId || '') !== '-10000').length > 0 ? (
            <div
              className="rounded-xl p-3 space-y-3"
              style={{ backgroundColor: 'var(--surface-soft)' }}
            >
              <div className="text-[12px] font-medium" style={{ color: 'var(--text-secondary)' }}>商品属性</div>
              {propCards
                .filter((card: any) => String(card?.propertyId || '') !== '-10000')
                .map((card: any) => (
                  <div key={card.propertyId}>
                    <div className="text-[12px] mb-1.5" style={{ color: 'var(--text-secondary)' }}>
                      {card.propertyName}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      {(card.valuesList || []).slice(0, 12).map((value: any) => {
                        const valueId = String(value.valueId || value.channelCatId || value.catId || '').trim()
                        const selected = valueId && String(propSelections[card.propertyId] || '').trim() === valueId
                        return (
                          <button
                            key={valueId || value.valueName || value.catName}
                            type="button"
                            onClick={() => {
                              setPropSelections((current) => {
                                const next = { ...current }
                                if (next[card.propertyId] === valueId) {
                                  delete next[card.propertyId]
                                } else if (valueId) {
                                  next[card.propertyId] = valueId
                                }
                                return next
                              })
                            }}
                            className="rounded-full px-3 py-1.5 text-[12px] transition-colors"
                            style={{
                              backgroundColor: selected ? 'var(--primary-light)' : 'var(--surface-card-strong)',
                              border: `1px solid ${selected ? 'var(--primary)' : 'var(--control-border)'}`,
                              color: selected ? 'var(--primary)' : 'var(--text-main)',
                            }}
                          >
                            {value.valueName || value.catName || value.text || valueId}
                          </button>
                        )
                      })}
                    </div>
                  </div>
                ))}
            </div>
          ) : null}
        </div>
      )}

      {/* Step 2: 价格库存 */}
      {step === 1 && (
        <div className="space-y-4">
          <div>
            <label className="label">售价（元）<span style={{ color: 'var(--danger)' }}>*</span></label>
            <input
              type="number"
              className="input text-[20px] font-semibold"
              placeholder="0.00"
              value={form.price}
              onChange={(e) => update('price', e.target.value)}
              min="0"
              step="0.01"
              inputMode="decimal"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="label">原价（可选）</label>
              <input
                type="number"
                className="input"
                placeholder="原价"
                value={form.origPrice}
                onChange={(e) => update('origPrice', e.target.value)}
                min="0"
                step="0.01"
                inputMode="decimal"
              />
            </div>
            <div>
              <label className="label">数量</label>
              <input
                type="number"
                className="input"
                value={form.quantity}
                onChange={(e) => update('quantity', Math.max(1, Number(e.target.value) || 1))}
                min="1"
                inputMode="numeric"
              />
            </div>
          </div>

          <button
            onClick={() => update('noBargain', !form.noBargain)}
            className="w-full flex items-center justify-between p-3 rounded-xl"
            style={{ backgroundColor: form.noBargain ? 'var(--primary-light)' : 'var(--surface-soft)' }}
          >
            <span className="text-[14px]" style={{ color: 'var(--text-main)' }}>不接受还价</span>
            <div
              className="w-11 h-6 rounded-full relative transition-colors"
              style={{ backgroundColor: form.noBargain ? 'var(--primary)' : 'var(--status-neutral-bg)' }}
            >
              <div
                className="absolute top-0.5 w-5 h-5 rounded-full transition-transform"
                style={{
                  backgroundColor: 'var(--liquid-highlight)',
                  boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.64), 0 2px 6px rgba(25,35,55,0.12)',
                  transform: form.noBargain ? 'translateX(22px)' : 'translateX(2px)',
                }}
              />
            </div>
          </button>

          <div className="rounded-xl p-3 space-y-3" style={{ backgroundColor: 'var(--surface-soft)' }}>
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-[14px] font-medium" style={{ color: 'var(--text-main)' }}>闲鱼币抵扣</div>
                <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>发布成功后自动按可用比例开启</div>
              </div>
              <label className="inline-flex items-center gap-2 text-[13px]" style={{ color: 'var(--text-main)' }}>
                <input
                  type="checkbox"
                  checked={enableCoinDeduction}
                  onChange={(e) => setEnableCoinDeduction(e.target.checked)}
                />
                开启
              </label>
            </div>
            {enableCoinDeduction ? (
              <select
                value={coinDeductionPercent}
                onChange={(e) => setCoinDeductionPercent(e.target.value)}
                className="input"
              >
                <option value="3">3%</option>
                <option value="5">5%</option>
                <option value="8">8%</option>
                <option value="10">10%</option>
                <option value="15">15%</option>
                <option value="20">20%</option>
              </select>
            ) : null}

            <div className="h-px" style={{ backgroundColor: 'var(--border-light)' }} />

            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-[14px] font-medium" style={{ color: 'var(--text-main)' }}>2人小刀</div>
                <div className="text-[11px]" style={{ color: 'var(--text-muted)' }}>留空自动选择当前可开的最低档</div>
              </div>
              <label className="inline-flex items-center gap-2 text-[13px]" style={{ color: 'var(--text-main)' }}>
                <input
                  type="checkbox"
                  checked={enableGroupon}
                  onChange={(e) => setEnableGroupon(e.target.checked)}
                />
                开启
              </label>
            </div>
            {enableGroupon ? (
              <div className="flex items-center gap-2">
                <span className="text-[14px] font-medium" style={{ color: 'var(--text-secondary)' }}>¥</span>
                <input
                  type="text"
                  className="input flex-1"
                  value={grouponAmount}
                  onChange={(e) => setGrouponAmount(e.target.value.replace(/[^\d.]/g, ''))}
                  placeholder="自动"
                  inputMode="decimal"
                />
              </div>
            ) : null}
          </div>
        </div>
      )}

      {/* Step 3: 发布设置 */}
      {step === 2 && (
        <div className="space-y-4">
          <div>
            <label className="label flex items-center gap-1.5"><MapPin className="w-3.5 h-3.5" /> 商品位置</label>
            <div className="text-[12px] mb-1.5" style={{ color: 'var(--text-secondary)' }}>
              {locationSummary || '未设置发布地，可直接搜索地点'}
              {form.divisionId ? <span className="font-mono ml-1.5">· {form.divisionId}</span> : null}
            </div>
            <div className="relative">
              <input
                type="text"
                value={poiQuery}
                onChange={(e) => setPoiQuery(e.target.value)}
                placeholder="搜索地点（如「武汉洪山」）"
                className="w-full text-[13px] px-3 py-2 rounded-xl"
                style={{ backgroundColor: 'var(--surface-soft)', color: 'var(--text-main)', border: 'none', outline: 'none' }}
              />
              {poiBusy ? (
                <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 animate-spin" style={{ color: 'var(--text-muted)' }} />
              ) : null}
              {poiSuggestions.length > 0 ? (
                <div className="liquid-list-panel absolute left-0 right-0 top-full mt-1 rounded-xl overflow-hidden max-h-56 overflow-y-auto z-10">
                  {poiSuggestions.map((p: any, i: number) => (
                    <button
                      key={`${p.name}-${i}`}
                      type="button"
                      onClick={() => applyPoiSuggestion(p)}
                      className="liquid-list-row w-full text-left px-3 py-2 active:bg-[var(--surface-soft)]"
                      style={{ borderBottom: i === poiSuggestions.length - 1 ? 'none' : '1px solid var(--control-border)' }}
                    >
                      <p className="text-[13px] font-medium truncate" style={{ color: 'var(--text-main)' }}>{p.name}</p>
                      {p.address ? <p className="text-[11px] truncate" style={{ color: 'var(--text-muted)' }}>{p.address}</p> : null}
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
          </div>

          <div>
            <label className="label">发货方式</label>
            <div className="grid grid-cols-2 gap-2">
              {[
                { value: 'free', label: '包邮', hint: '卖家承担运费' },
                { value: 'freight', label: '按距离计费', hint: '使用运费模板' },
                { value: 'fixed', label: '固定运费', hint: '填写固定邮费' },
                { value: 'self', label: '无需邮寄', hint: '自提/虚拟服务' },
              ].map((option) => (
                <button
                  key={option.value}
                  onClick={() => update('shippingType', option.value)}
                  className="p-3 rounded-xl text-center cursor-pointer transition-colors"
                  style={{
                    backgroundColor: form.shippingType === option.value ? 'var(--primary-light)' : 'var(--surface-soft)',
                    border: `2px solid ${form.shippingType === option.value ? 'var(--primary)' : 'transparent'}`,
                  }}
                >
                  <div className="text-[14px] font-medium" style={{ color: 'var(--text-main)' }}>{option.label}</div>
                  <div className="text-[11px] mt-0.5" style={{ color: 'var(--text-muted)' }}>{option.hint}</div>
                </button>
              ))}
            </div>
            {form.shippingType === 'fixed' && (
              <input
                type="number"
                className="input mt-2"
                placeholder="邮费（元）"
                value={form.postPrice}
                onChange={(e) => update('postPrice', e.target.value)}
                min="0"
                step="0.01"
                inputMode="decimal"
              />
            )}
          </div>
        </div>
      )}
    </BottomSheet>
  )
}
