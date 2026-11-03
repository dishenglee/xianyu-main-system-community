import { useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent, type Dispatch, type ReactNode, type SetStateAction } from 'react'
import {
  AlertTriangle,
  CheckCircle2,
  Cloud,
  CloudDownload,
  CloudUpload,
  Database,
  Download,
  FileArchive,
  HardDrive,
  KeyRound,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Upload,
} from 'lucide-react'
import {
  buildAccountDataExportFileName,
  chooseAccountDataExportPath,
  deleteAccountDataCloudSnapshot,
  downloadAccountDataCloudSnapshot,
  exportAccountDataPackage,
  getAccountDataPackagePreview,
  inspectAccountDataPackage,
  listAccountDataCloudSnapshots,
  restoreAccountDataCloudSnapshot,
  restoreAccountDataPackage,
  uploadAccountDataCloudSnapshot,
  type AccountDataCloudUploadProgress,
  type AccountDataCloudDownloadedPackage,
  type AccountDataCloudSnapshotPresentation,
  type AccountDataExportResult,
  type AccountDataImportInspection,
  type AccountDataSectionPresentation,
  type AccountDataTransferPreview,
} from './api'
import { getAccountDataImportAccept } from './filePicker'
import { HubTabs, HUB_PRESETS } from '../../components/HubTabs'
import { useNotify } from '../../components/Notify'
import { useAppDialog } from '../../components/AppDialog'
import { useDevice } from '../../hooks/useDevice'
import { emitActionFailure, emitActionLog, emitActionStarted, emitActionSuccess } from '../../bootstrap/actionLogging'

type SelectionMap = Record<string, boolean>
type TransferProgress = { percent: number; label: string } | null
type ProgressSetter = Dispatch<SetStateAction<TransferProgress>>

const EXPORT_STAGES = [
  { percent: 12, label: '正在准备数据快照' },
  { percent: 42, label: '正在压缩数据包' },
  { percent: 72, label: '正在写入保存文件' },
  { percent: 90, label: '正在完成导出' },
]

const IMPORT_RESTORE_STAGES = [
  { percent: 14, label: '正在准备导入任务' },
  { percent: 52, label: '正在写入待恢复区' },
  { percent: 86, label: '正在准备重启客户端' },
]

const CLOUD_DOWNLOAD_STAGES = [
  { percent: 12, label: '正在下载云端备份' },
  { percent: 48, label: '正在解密数据包' },
  { percent: 78, label: '正在校验数据归属' },
]

const CLOUD_RESTORE_STAGES = [
  { percent: 14, label: '正在准备云端导入任务' },
  { percent: 56, label: '正在写入待恢复区' },
  { percent: 88, label: '正在准备重启客户端' },
]

function formatBytes(bytes = 0) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unitIndex = 0
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024
    unitIndex += 1
  }
  return `${value >= 10 || unitIndex === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unitIndex]}`
}

function formatDateTime(value?: string) {
  if (!value) return '—'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function cloudSnapshotSections(
  keys: string[],
  sections: AccountDataSectionPresentation[],
) {
  const labelMap = new Map(sections.map((section) => [section.key, section.label]))
  return keys.map((key) => labelMap.get(key) || key).join('、')
}

function sectionSelection(sections: AccountDataSectionPresentation[] = []) {
  return sections.reduce<SelectionMap>((acc, section) => {
    acc[section.key] = section.present && (section.required || section.selectedByDefault)
    return acc
  }, {})
}

function selectedKeys(sections: AccountDataSectionPresentation[], selection: SelectionMap) {
  return sections
    .filter((section) => section.present && (section.required || selection[section.key]))
    .map((section) => section.key)
}

function totalSelectedSize(sections: AccountDataSectionPresentation[], selection: SelectionMap) {
  return sections
    .filter((section) => section.present && (section.required || selection[section.key]))
    .reduce((sum, section) => sum + Number(section.sizeBytes || 0), 0)
}

function sectionLogSummary(sections: AccountDataSectionPresentation[], selection: SelectionMap) {
  const keys = selectedKeys(sections, selection)
  return {
    sectionCount: keys.length,
    sections: keys,
    sizeBytes: totalSelectedSize(sections, selection),
  }
}

function cloudUploadProgressLabel(progress: AccountDataCloudUploadProgress) {
  if (
    progress.phase === 'upload'
    && Number(progress.uploadedBytes || 0) > 0
    && Number(progress.totalBytes || 0) > 0
  ) {
    const chunkText = progress.chunkIndex && progress.chunkCount
      ? `（${progress.chunkIndex}/${progress.chunkCount}）`
      : ''
    return `${progress.label}${chunkText} ${formatBytes(progress.uploadedBytes || 0)}/${formatBytes(progress.totalBytes || 0)}`
  }
  if (progress.encryptedSizeBytes && progress.phase === 'upload') {
    return `${progress.label}，实际上传 ${formatBytes(progress.encryptedSizeBytes)}`
  }
  if (progress.packageSizeBytes && progress.phase === 'encrypt') {
    return `${progress.label}`
  }
  return progress.label
}

function fileToBase64(file: File, onProgress?: (percent: number) => void): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onprogress = (event) => {
      if (event.lengthComputable && event.total > 0) {
        onProgress?.(Math.min(38, 8 + Math.round((event.loaded / event.total) * 30)))
      }
    }
    reader.onload = () => resolve(String(reader.result || ''))
    reader.onerror = () => reject(reader.error || new Error('读取文件失败'))
    reader.readAsDataURL(file)
  })
}

function useTransferProgress(setProgress: ProgressSetter) {
  const timersRef = useRef<number[]>([])

  const clearTimers = useCallback(() => {
    for (const timer of timersRef.current) window.clearTimeout(timer)
    timersRef.current = []
  }, [])

  const start = useCallback((stages: { percent: number; label: string }[]) => {
    clearTimers()
    const [first, ...rest] = stages
    if (first) setProgress(first)
    timersRef.current = rest.map((stage, index) => window.setTimeout(() => {
      setProgress(stage)
    }, 700 + index * 900))
  }, [clearTimers, setProgress])

  const set = useCallback((progress: Exclude<TransferProgress, null>) => {
    setProgress(progress)
  }, [setProgress])

  const finish = useCallback((label: string) => {
    clearTimers()
    setProgress({ percent: 100, label })
    timersRef.current = [window.setTimeout(() => setProgress(null), 1100)]
  }, [clearTimers, setProgress])

  const reset = useCallback(() => {
    clearTimers()
    setProgress(null)
  }, [clearTimers, setProgress])

  useEffect(() => clearTimers, [clearTimers])

  return { start, set, finish, reset }
}

function ProgressLine({ progress }: { progress: TransferProgress }) {
  if (!progress) return null
  const percent = Math.max(0, Math.min(100, progress.percent))
  return (
    <div className="mt-3 rounded-[12px] border px-3 py-2.5" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}>
      <div className="flex items-center justify-between gap-3 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
        <span>{progress.label}</span>
        <span className="font-mono font-semibold" style={{ color: 'var(--text-main)' }}>{percent}%</span>
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded-full" style={{ background: 'var(--surface-sunken)' }}>
        <div
          className="h-full rounded-full transition-all duration-300"
          style={{ width: `${percent}%`, background: 'var(--primary)' }}
        />
      </div>
    </div>
  )
}

function Panel({ title, children, isMobile }: { title: string; children: ReactNode; isMobile: boolean }) {
  return (
    <section className={isMobile ? 'mobile-surface-card px-4 py-4' : 'surface-card rounded-[18px] p-5'}>
      <h2 className={isMobile ? 'mobile-section-title text-[1.08rem]' : 'text-[15px] font-semibold'}>{title}</h2>
      <div className="mt-4">{children}</div>
    </section>
  )
}

function SectionRow({
  section,
  checked,
  disabled,
  onChange,
}: {
  section: AccountDataSectionPresentation
  checked: boolean
  disabled?: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <label
      className="flex min-h-[58px] items-center gap-3 rounded-[12px] border px-3 py-2.5"
      style={{
        borderColor: checked ? 'rgba(10, 102, 255, 0.22)' : 'var(--control-border)',
        background: checked ? 'var(--control-active)' : 'var(--surface-control)',
      }}
    >
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        className="h-4 w-4 shrink-0 accent-[var(--primary)]"
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>{section.label}</span>
          {section.required ? (
            <span className="rounded-full px-2 py-0.5 text-[10px] font-medium" style={{ background: 'var(--status-info-bg)', color: 'var(--status-info-text)' }}>必选</span>
          ) : null}
          {!section.present ? (
            <span className="rounded-full px-2 py-0.5 text-[10px] font-medium" style={{ background: 'var(--status-warning-bg)', color: 'var(--status-warning-text)' }}>暂无数据</span>
          ) : null}
        </div>
        <div className="mt-1 truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>{section.description}</div>
      </div>
      <div className="shrink-0 text-right font-mono text-[12px] font-semibold" style={{ color: 'var(--text-secondary)' }}>
        {formatBytes(section.sizeBytes)}
      </div>
    </label>
  )
}

function ResultNotice({ result }: { result: AccountDataExportResult | null }) {
  if (!result) return null
  return (
    <div className="liquid-notice liquid-notice-success mt-3 rounded-[12px] px-3 py-2 text-[12px]">
      已导出 {formatBytes(result.sizeBytes)} 到：{result.path}
    </div>
  )
}

export default function BundleTransferPage() {
  const cloudSyncAvailable: boolean = false
  const { isMobile } = useDevice()
  const notify = useNotify()
  const { showConfirm } = useAppDialog()
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const importFileAccept = useMemo(() => getAccountDataImportAccept(), [])
  const [preview, setPreview] = useState<AccountDataTransferPreview | null>(null)
  const [exportSelection, setExportSelection] = useState<SelectionMap>({})
  const [exporting, setExporting] = useState(false)
  const [exportResult, setExportResult] = useState<AccountDataExportResult | null>(null)
  const [exportProgress, setExportProgress] = useState<TransferProgress>(null)
  const [loadingPreview, setLoadingPreview] = useState(false)
  const [importInspection, setImportInspection] = useState<AccountDataImportInspection | null>(null)
  const [importSelection, setImportSelection] = useState<SelectionMap>({})
  const [importing, setImporting] = useState(false)
  const [importProgress, setImportProgress] = useState<TransferProgress>(null)
  const [selectedFileName, setSelectedFileName] = useState('')
  const [cloudSnapshots, setCloudSnapshots] = useState<AccountDataCloudSnapshotPresentation[]>([])
  const [cloudError, setCloudError] = useState('')
  const [cloudLoading, setCloudLoading] = useState(false)
  const [cloudSelection, setCloudSelection] = useState<SelectionMap>({})
  const [cloudUploadOpen, setCloudUploadOpen] = useState(false)
  const [cloudPassword, setCloudPassword] = useState('')
  const [cloudUploading, setCloudUploading] = useState(false)
  const [cloudUploadProgress, setCloudUploadProgress] = useState<TransferProgress>(null)
  const [cloudSelectedSnapshotId, setCloudSelectedSnapshotId] = useState('')
  const [cloudRestorePassword, setCloudRestorePassword] = useState('')
  const [cloudDownloaded, setCloudDownloaded] = useState<AccountDataCloudDownloadedPackage | null>(null)
  const [cloudRestoreSelection, setCloudRestoreSelection] = useState<SelectionMap>({})
  const [cloudDownloading, setCloudDownloading] = useState(false)
  const [cloudDownloadProgress, setCloudDownloadProgress] = useState<TransferProgress>(null)
  const [cloudRestoring, setCloudRestoring] = useState(false)
  const [cloudRestoreProgress, setCloudRestoreProgress] = useState<TransferProgress>(null)
  const [cloudDeletingId, setCloudDeletingId] = useState('')
  const exportProgressCtl = useTransferProgress(setExportProgress)
  const importProgressCtl = useTransferProgress(setImportProgress)
  const cloudUploadProgressCtl = useTransferProgress(setCloudUploadProgress)
  const cloudDownloadProgressCtl = useTransferProgress(setCloudDownloadProgress)
  const cloudRestoreProgressCtl = useTransferProgress(setCloudRestoreProgress)

  const loadPreview = useCallback(async () => {
    setLoadingPreview(true)
    try {
      const data = await getAccountDataPackagePreview()
      setPreview(data)
      setExportSelection(sectionSelection(data.sections))
      setCloudSelection(sectionSelection(data.sections))
      emitActionSuccess({
        module: 'account-data-transfer',
        action: 'load_transfer_preview',
        message: '账号数据导入导出预览加载成功',
        context: {
          ownerUsername: data.ownerUsername,
          sectionCount: data.sections.length,
          totalSizeBytes: data.sections.reduce((sum, section) => sum + Number(section.sizeBytes || 0), 0),
        },
      })
    } catch (error) {
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'load_transfer_preview',
        message: '账号数据导入导出预览加载失败',
        error,
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
    } finally {
      setLoadingPreview(false)
    }
  }, [notify])

  const loadCloudSnapshots = useCallback(async () => {
    setCloudLoading(true)
    setCloudError('')
    try {
      const snapshots = await listAccountDataCloudSnapshots()
      setCloudSnapshots(snapshots)
      emitActionSuccess({
        module: 'account-data-transfer',
        action: 'list_cloud_snapshots',
        message: '云端备份列表加载成功',
        context: { snapshotCount: snapshots.length },
      })
    } catch (error) {
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'list_cloud_snapshots',
        message: '云端备份列表加载失败',
        error,
      })
      setCloudError(error instanceof Error ? error.message : String(error))
    } finally {
      setCloudLoading(false)
    }
  }, [])

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void loadPreview()
    }, 0)
    return () => window.clearTimeout(timer)
  }, [loadCloudSnapshots, loadPreview])

  const exportSize = useMemo(
    () => totalSelectedSize(preview?.sections || [], exportSelection),
    [preview?.sections, exportSelection],
  )

  const importSize = useMemo(
    () => totalSelectedSize(importInspection?.sections || [], importSelection),
    [importInspection?.sections, importSelection],
  )

  const cloudUploadSize = useMemo(
    () => totalSelectedSize(preview?.sections || [], cloudSelection),
    [cloudSelection, preview?.sections],
  )

  const cloudRestoreSize = useMemo(
    () => totalSelectedSize(cloudDownloaded?.sections || [], cloudRestoreSelection),
    [cloudDownloaded?.sections, cloudRestoreSelection],
  )

  const cloudSelectedSnapshot = useMemo(
    () => cloudSnapshots.find((snapshot) => snapshot.id === cloudSelectedSnapshotId) || null,
    [cloudSelectedSnapshotId, cloudSnapshots],
  )

  const handleExport = useCallback(async () => {
    if (!preview || exporting) return
    const sections = selectedKeys(preview.sections, exportSelection)
    let outputPath: string | null = null
    try {
      outputPath = await chooseAccountDataExportPath(buildAccountDataExportFileName(preview.ownerUsername))
    } catch (error) {
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'choose_export_path',
        message: '选择账号数据导出路径失败',
        error,
        context: sectionLogSummary(preview.sections, exportSelection),
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
      return
    }
    if (!outputPath) {
      emitActionLog({
        module: 'account-data-transfer',
        action: 'export_package',
        outcome: 'cancelled',
        message: '用户取消账号数据导出',
        retryable: false,
        promoteToIncident: false,
        context: sectionLogSummary(preview.sections, exportSelection),
      })
      return
    }

    setExporting(true)
    setExportResult(null)
    exportProgressCtl.start(EXPORT_STAGES)
    emitActionStarted({
      module: 'account-data-transfer',
      action: 'export_package',
      message: '开始导出账号数据',
      context: sectionLogSummary(preview.sections, exportSelection),
    })
    try {
      const result = await exportAccountDataPackage(sections, outputPath)
      setExportResult(result)
      notify(`账号数据已导出：${formatBytes(result.sizeBytes)}`, 'success')
      exportProgressCtl.finish('导出完成')
      emitActionSuccess({
        module: 'account-data-transfer',
        action: 'export_package',
        message: '账号数据导出成功',
        context: {
          ...sectionLogSummary(preview.sections, exportSelection),
          resultSizeBytes: result.sizeBytes,
        },
      })
    } catch (error) {
      exportProgressCtl.reset()
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'export_package',
        message: '账号数据导出失败',
        error,
        context: sectionLogSummary(preview.sections, exportSelection),
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
    } finally {
      setExporting(false)
    }
  }, [exportProgressCtl, exportSelection, exporting, notify, preview])

  const handleFileSelected = useCallback(async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (!file) return
    setSelectedFileName(file.name)
    setImportInspection(null)
    setImportSelection({})
    setImporting(true)
    importProgressCtl.start([{ percent: 8, label: '正在读取数据包' }])
    emitActionStarted({
      module: 'account-data-transfer',
      action: 'inspect_import_package',
      message: '开始校验账号数据包',
      context: {
        fileName: file.name,
        fileSizeBytes: file.size,
      },
    })
    try {
      const archiveBase64 = await fileToBase64(file, (percent) => {
        importProgressCtl.set({ percent, label: '正在读取数据包' })
      })
      importProgressCtl.set({ percent: 46, label: '正在校验数据归属' })
      const inspection = await inspectAccountDataPackage(archiveBase64)
      setImportInspection(inspection)
      setImportSelection(sectionSelection(inspection.sections))
      if (inspection.canImport) {
        notify('数据包校验通过', 'success')
        importProgressCtl.finish('数据包校验完成')
      } else {
        notify(inspection.blockingReason || '数据包不能导入', 'error')
        importProgressCtl.finish('数据包校验未通过')
      }
      emitActionLog({
        module: 'account-data-transfer',
        action: 'inspect_import_package',
        outcome: inspection.canImport ? 'success' : 'warn',
        message: inspection.canImport ? '账号数据包校验通过' : '账号数据包校验未通过',
        detail: inspection.canImport ? '' : inspection.blockingReason || undefined,
        retryable: false,
        promoteToIncident: false,
        context: {
          fileName: file.name,
          fileSizeBytes: file.size,
          canImport: inspection.canImport,
          sectionCount: inspection.sections.length,
        },
      })
    } catch (error) {
      importProgressCtl.reset()
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'inspect_import_package',
        message: '账号数据包校验失败',
        error,
        context: {
          fileName: file.name,
          fileSizeBytes: file.size,
        },
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
    } finally {
      setImporting(false)
    }
  }, [importProgressCtl, notify])

  const handleRestore = useCallback(async () => {
    if (!importInspection || importing || !importInspection.canImport) return
    const sections = selectedKeys(importInspection.sections, importSelection)
    const confirmed = await showConfirm(
      `确认导入 ${sections.length} 类数据？客户端会自动重启，未勾选的数据不会从数据包恢复，可在重启后通过同步重新拉取商品、订单和消息。`,
      { title: '导入账号数据', confirmLabel: '导入并重启', danger: true, width: 480 },
    )
    if (!confirmed) return

    setImporting(true)
    importProgressCtl.start(IMPORT_RESTORE_STAGES)
    emitActionStarted({
      module: 'account-data-transfer',
      action: 'restore_import_package',
      message: '开始导入账号数据包',
      context: sectionLogSummary(importInspection.sections, importSelection),
    })
    try {
      await restoreAccountDataPackage(importInspection.importToken, sections)
      notify('数据已安排导入，客户端即将重启', 'success')
      importProgressCtl.finish('导入任务已创建，正在重启')
      emitActionSuccess({
        module: 'account-data-transfer',
        action: 'restore_import_package',
        message: '账号数据包导入任务已创建',
        context: sectionLogSummary(importInspection.sections, importSelection),
      })
    } catch (error) {
      importProgressCtl.reset()
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'restore_import_package',
        message: '账号数据包导入失败',
        error,
        context: sectionLogSummary(importInspection.sections, importSelection),
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
      setImporting(false)
    }
  }, [importInspection, importProgressCtl, importSelection, importing, notify, showConfirm])

  const handleCloudUpload = useCallback(async () => {
    if (!preview || cloudUploading) return
    const password = cloudPassword.trim()
    if (!password) {
      emitActionLog({
        module: 'account-data-transfer',
        action: 'upload_cloud_snapshot',
        outcome: 'warn',
        message: '云端备份上传缺少同步密码',
        retryable: false,
        promoteToIncident: false,
        context: sectionLogSummary(preview.sections, cloudSelection),
      })
      notify('请输入同步密码', 'error')
      return
    }
    const sections = selectedKeys(preview.sections, cloudSelection)
    setCloudUploading(true)
    cloudUploadProgressCtl.set({ percent: 3, label: '正在准备云端上传' })
    emitActionStarted({
      module: 'account-data-transfer',
      action: 'upload_cloud_snapshot',
      message: '开始上传云端备份',
      context: sectionLogSummary(preview.sections, cloudSelection),
    })
    try {
      const result = await uploadAccountDataCloudSnapshot(sections, password, (progress) => {
        cloudUploadProgressCtl.set({
          percent: progress.percent,
          label: cloudUploadProgressLabel(progress),
        })
      })
      notify(`云端备份已上传：${formatBytes(result.snapshot.encryptedSizeBytes)}`, 'success')
      setCloudPassword('')
      setCloudUploadOpen(false)
      await loadCloudSnapshots()
      cloudUploadProgressCtl.finish('云端备份上传完成')
      emitActionSuccess({
        module: 'account-data-transfer',
        action: 'upload_cloud_snapshot',
        message: '云端备份上传成功',
        context: {
          ...sectionLogSummary(preview.sections, cloudSelection),
          packageSizeBytes: result.snapshot.packageSizeBytes,
          encryptedSizeBytes: result.snapshot.encryptedSizeBytes,
        },
      })
    } catch (error) {
      cloudUploadProgressCtl.reset()
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'upload_cloud_snapshot',
        message: '云端备份上传失败',
        error,
        context: sectionLogSummary(preview.sections, cloudSelection),
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
    } finally {
      setCloudUploading(false)
    }
  }, [cloudPassword, cloudSelection, cloudUploadProgressCtl, cloudUploading, loadCloudSnapshots, notify, preview])

  const handleSelectCloudSnapshot = useCallback((snapshotId: string) => {
    setCloudSelectedSnapshotId(snapshotId)
    setCloudDownloaded(null)
    setCloudRestoreSelection({})
    setCloudRestorePassword('')
    cloudDownloadProgressCtl.reset()
    cloudRestoreProgressCtl.reset()
  }, [cloudDownloadProgressCtl, cloudRestoreProgressCtl])

  const handleCloudDownload = useCallback(async () => {
    if (!cloudSelectedSnapshot || cloudDownloading) return
    const password = cloudRestorePassword.trim()
    if (!password) {
      emitActionLog({
        module: 'account-data-transfer',
        action: 'download_cloud_snapshot',
        outcome: 'warn',
        message: '云端备份下载缺少同步密码',
        retryable: false,
        promoteToIncident: false,
        context: {
          packageSizeBytes: cloudSelectedSnapshot.packageSizeBytes,
          encryptedSizeBytes: cloudSelectedSnapshot.encryptedSizeBytes,
          sectionCount: cloudSelectedSnapshot.sections.length,
        },
      })
      notify('请输入同步密码', 'error')
      return
    }
    setCloudDownloading(true)
    setCloudDownloaded(null)
    setCloudRestoreSelection({})
    cloudDownloadProgressCtl.start(CLOUD_DOWNLOAD_STAGES)
    emitActionStarted({
      module: 'account-data-transfer',
      action: 'download_cloud_snapshot',
      message: '开始下载云端备份',
      context: {
        packageSizeBytes: cloudSelectedSnapshot.packageSizeBytes,
        encryptedSizeBytes: cloudSelectedSnapshot.encryptedSizeBytes,
        sectionCount: cloudSelectedSnapshot.sections.length,
      },
    })
    try {
      const downloaded = await downloadAccountDataCloudSnapshot(cloudSelectedSnapshot.id, password)
      setCloudDownloaded(downloaded)
      setCloudRestoreSelection(sectionSelection(downloaded.sections))
      if (downloaded.canImport) {
        notify('云端备份解密校验通过', 'success')
        cloudDownloadProgressCtl.finish('云端备份校验完成')
      } else {
        notify(downloaded.blockingReason || '云端备份不能导入', 'error')
        cloudDownloadProgressCtl.finish('云端备份校验未通过')
      }
      emitActionLog({
        module: 'account-data-transfer',
        action: 'download_cloud_snapshot',
        outcome: downloaded.canImport ? 'success' : 'warn',
        message: downloaded.canImport ? '云端备份下载校验通过' : '云端备份下载校验未通过',
        detail: downloaded.canImport ? '' : downloaded.blockingReason || undefined,
        retryable: false,
        promoteToIncident: false,
        context: {
          packageSizeBytes: cloudSelectedSnapshot.packageSizeBytes,
          encryptedSizeBytes: cloudSelectedSnapshot.encryptedSizeBytes,
          canImport: downloaded.canImport,
          sectionCount: downloaded.sections.length,
        },
      })
    } catch (error) {
      cloudDownloadProgressCtl.reset()
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'download_cloud_snapshot',
        message: '云端备份下载失败',
        error,
        context: {
          packageSizeBytes: cloudSelectedSnapshot.packageSizeBytes,
          encryptedSizeBytes: cloudSelectedSnapshot.encryptedSizeBytes,
          sectionCount: cloudSelectedSnapshot.sections.length,
        },
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
    } finally {
      setCloudDownloading(false)
    }
  }, [cloudDownloadProgressCtl, cloudDownloading, cloudRestorePassword, cloudSelectedSnapshot, notify])

  const handleCloudRestore = useCallback(async () => {
    if (!cloudDownloaded || cloudRestoring || !cloudDownloaded.canImport) return
    const sections = selectedKeys(cloudDownloaded.sections, cloudRestoreSelection)
    const confirmed = await showConfirm(
      `确认从云端备份导入 ${sections.length} 类数据？客户端会自动重启，未勾选的数据不会从备份恢复。`,
      { title: '恢复云端备份', confirmLabel: '导入并重启', danger: true, width: 480 },
    )
    if (!confirmed) return

    setCloudRestoring(true)
    cloudRestoreProgressCtl.start(CLOUD_RESTORE_STAGES)
    emitActionStarted({
      module: 'account-data-transfer',
      action: 'restore_cloud_snapshot',
      message: '开始恢复云端备份',
      context: sectionLogSummary(cloudDownloaded.sections, cloudRestoreSelection),
    })
    try {
      await restoreAccountDataCloudSnapshot(cloudDownloaded.importToken, sections)
      notify('云端备份已安排导入，客户端即将重启', 'success')
      cloudRestoreProgressCtl.finish('云端导入任务已创建，正在重启')
      emitActionSuccess({
        module: 'account-data-transfer',
        action: 'restore_cloud_snapshot',
        message: '云端备份恢复任务已创建',
        context: sectionLogSummary(cloudDownloaded.sections, cloudRestoreSelection),
      })
    } catch (error) {
      cloudRestoreProgressCtl.reset()
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'restore_cloud_snapshot',
        message: '云端备份恢复失败',
        error,
        context: sectionLogSummary(cloudDownloaded.sections, cloudRestoreSelection),
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
      setCloudRestoring(false)
    }
  }, [cloudDownloaded, cloudRestoreProgressCtl, cloudRestoreSelection, cloudRestoring, notify, showConfirm])

  const handleDeleteCloudSnapshot = useCallback(async (snapshot: AccountDataCloudSnapshotPresentation) => {
    if (cloudDeletingId) return
    const confirmed = await showConfirm(
      `确认删除 ${formatDateTime(snapshot.createdAt)} 的云端备份？`,
      { title: '删除云端备份', confirmLabel: '删除', danger: true, width: 420 },
    )
    if (!confirmed) return
    setCloudDeletingId(snapshot.id)
    emitActionStarted({
      module: 'account-data-transfer',
      action: 'delete_cloud_snapshot',
      message: '开始删除云端备份',
      context: {
        packageSizeBytes: snapshot.packageSizeBytes,
        encryptedSizeBytes: snapshot.encryptedSizeBytes,
        sectionCount: snapshot.sections.length,
      },
    })
    try {
      await deleteAccountDataCloudSnapshot(snapshot.id)
      notify('云端备份已删除', 'success')
      if (cloudSelectedSnapshotId === snapshot.id) {
        setCloudSelectedSnapshotId('')
        setCloudDownloaded(null)
        setCloudRestoreSelection({})
        setCloudRestorePassword('')
      }
      await loadCloudSnapshots()
      emitActionSuccess({
        module: 'account-data-transfer',
        action: 'delete_cloud_snapshot',
        message: '云端备份删除成功',
        context: {
          packageSizeBytes: snapshot.packageSizeBytes,
          encryptedSizeBytes: snapshot.encryptedSizeBytes,
          sectionCount: snapshot.sections.length,
        },
      })
    } catch (error) {
      emitActionFailure({
        module: 'account-data-transfer',
        action: 'delete_cloud_snapshot',
        message: '云端备份删除失败',
        error,
        context: {
          packageSizeBytes: snapshot.packageSizeBytes,
          encryptedSizeBytes: snapshot.encryptedSizeBytes,
          sectionCount: snapshot.sections.length,
        },
      })
      notify(error instanceof Error ? error.message : String(error), 'error')
    } finally {
      setCloudDeletingId('')
    }
  }, [cloudDeletingId, cloudSelectedSnapshotId, loadCloudSnapshots, notify, showConfirm])

  const owner = preview?.ownerUsername || '未登录'
  const sections = preview?.sections || []
  const requiredReady = sections.every((section) => !section.required || section.present)
  const canExport = requiredReady && selectedKeys(sections, exportSelection).length > 0 && !exporting && !loadingPreview
  const canCloudUpload = requiredReady && selectedKeys(sections, cloudSelection).length > 0 && !cloudUploading && !loadingPreview

  return (
    <div className={isMobile ? 'h-full flex flex-col mobile-page-shell' : 'contents'} style={isMobile ? { backgroundColor: 'var(--bg-page)' } : undefined}>
      <HubTabs tabs={HUB_PRESETS.accounts} />
      <div className={isMobile ? 'flex-1 overflow-y-auto mobile-page-body' : 'flex-1 overflow-y-auto p-4 md:p-6'}>
        <div className={isMobile ? 'mobile-page-stack' : 'space-y-4 md:space-y-5'}>
          <header className={isMobile ? 'mobile-surface-card px-4 py-4' : 'surface-card rounded-[18px] px-5 py-4'}>
            <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div>
                <h1 className={isMobile ? 'mobile-section-title' : 'text-lg font-semibold'}>账号数据导入导出</h1>
                <p className={isMobile ? 'mobile-section-note mt-2' : 'mt-1 text-[13px]'} style={{ color: 'var(--text-secondary)' }}>
                  账号数据必选，商品订单、消息记录、通知配置可按需勾选。
                  只导入账号数据后，也可以在对应页面重新同步商品、订单和消息。
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <span className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12px] font-medium" style={{ background: 'var(--status-info-bg)', color: 'var(--status-info-text)' }}>
                  <ShieldCheck className="h-3.5 w-3.5" />
                  {owner}
                </span>
                <button type="button" className="btn-secondary btn-sm" onClick={loadPreview} disabled={loadingPreview}>
                  <RefreshCw className={`h-3.5 w-3.5 ${loadingPreview ? 'animate-spin' : ''}`} />
                  刷新大小
                </button>
              </div>
            </div>
          </header>

          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <Panel title="导出数据" isMobile={isMobile}>
              <div className="space-y-2">
                {sections.map((section) => (
                  <SectionRow
                    key={section.key}
                    section={section}
                    checked={section.present && (section.required || !!exportSelection[section.key])}
                    disabled={section.required || !section.present || exporting}
                    onChange={(checked) => setExportSelection((prev) => ({ ...prev, [section.key]: checked }))}
                  />
                ))}
              </div>
              <div className="mt-4 flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                <div className="flex items-center gap-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                  <HardDrive className="h-4 w-4" />
                  当前选择原始数据约 {formatBytes(exportSize)}
                </div>
                <button type="button" className="btn-primary px-4 py-2.5" disabled={!canExport} onClick={handleExport}>
                  <Download className="h-4 w-4" />
                  {exporting ? '导出中…' : '导出数据包'}
                </button>
              </div>
              <ProgressLine progress={exportProgress} />
              <ResultNotice result={exportResult} />
            </Panel>

            <Panel title="导入数据" isMobile={isMobile}>
              <input
                ref={fileInputRef}
                type="file"
                accept={importFileAccept}
                className="hidden"
                onChange={handleFileSelected}
              />
              <button
                type="button"
                className="btn-secondary flex min-h-[74px] w-full items-center justify-center gap-3 rounded-[14px] border-dashed"
                onClick={() => fileInputRef.current?.click()}
                disabled={importing}
              >
                <FileArchive className="h-5 w-5" />
                <span>{importing ? '读取中…' : selectedFileName || '选择 .ygjdata 数据包'}</span>
              </button>
              <ProgressLine progress={importProgress} />

              {importInspection ? (
                <div className="mt-4 space-y-3">
                  <div
                    className={`liquid-notice ${importInspection.canImport ? 'liquid-notice-success' : 'liquid-notice-danger'} rounded-[12px] px-3 py-2 text-[12px]`}
                  >
                    <div className="flex items-center gap-2">
                      {importInspection.canImport ? <CheckCircle2 className="h-4 w-4" /> : <AlertTriangle className="h-4 w-4" />}
                      <span>
                        {importInspection.canImport
                          ? `数据包归属 ${importInspection.packageOwnerUsername}，可导入`
                          : importInspection.blockingReason || '数据包不能导入'}
                      </span>
                    </div>
                  </div>

                  <div className="space-y-2">
                    {importInspection.sections.map((section) => (
                      <SectionRow
                        key={section.key}
                        section={section}
                        checked={section.required || !!importSelection[section.key]}
                        disabled={section.required || importing || !importInspection.canImport}
                        onChange={(checked) => setImportSelection((prev) => ({ ...prev, [section.key]: checked }))}
                      />
                    ))}
                  </div>

                  <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                    <div className="flex items-center gap-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      <Database className="h-4 w-4" />
                      当前选择原始数据约 {formatBytes(importSize)}
                    </div>
                    <button
                      type="button"
                      className="btn-primary px-4 py-2.5"
                      disabled={importing || !importInspection.canImport}
                      onClick={handleRestore}
                    >
                      <Upload className="h-4 w-4" />
                      导入并重启
                    </button>
                  </div>
                </div>
              ) : (
                <p className="mt-3 text-[12px] leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  数据包会导入所选本机业务数据；导入前建议先导出一份当前数据备份。
                </p>
              )}
            </Panel>
          </div>

          {cloudSyncAvailable && <Panel title="云端同步（社区版未提供）" isMobile={isMobile}>
            <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]">
              <div className="space-y-3">
                <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
                  <div className="rounded-[12px] border px-3 py-2.5" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}>
                    <div className="flex items-center gap-2 text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                      <Cloud className="h-3.5 w-3.5" />
                      手动同步
                    </div>
                    <div className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>不会自动上传</div>
                  </div>
                  <div className="rounded-[12px] border px-3 py-2.5" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}>
                    <div className="flex items-center gap-2 text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                      <ShieldCheck className="h-3.5 w-3.5" />
                      只存密文
                    </div>
                    <div className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>服务端看不到账号数据</div>
                  </div>
                  <div className="rounded-[12px] border px-3 py-2.5" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}>
                    <div className="flex items-center gap-2 text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                      <KeyRound className="h-3.5 w-3.5" />
                      同步密码
                    </div>
                    <div className="mt-1 text-[11px]" style={{ color: 'var(--text-secondary)' }}>丢失后旧备份无法恢复</div>
                  </div>
                </div>

                <div className="space-y-2">
                  {sections.map((section) => (
                    <SectionRow
                      key={section.key}
                      section={section}
                      checked={section.present && (section.required || !!cloudSelection[section.key])}
                      disabled={section.required || !section.present || cloudUploading}
                      onChange={(checked) => setCloudSelection((prev) => ({ ...prev, [section.key]: checked }))}
                    />
                  ))}
                </div>

                <div className="rounded-[14px] border px-3 py-3" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}>
                  <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                    <div className="flex items-center gap-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                      <CloudUpload className="h-4 w-4" />
                      原始数据约 {formatBytes(cloudUploadSize)}，实际上传以压缩加密后为准
                    </div>
                    <button
                      type="button"
                      className="btn-secondary btn-sm"
                      disabled={!canCloudUpload}
                      onClick={() => setCloudUploadOpen((open) => !open)}
                    >
                      <CloudUpload className="h-3.5 w-3.5" />
                      上传云端备份
                    </button>
                  </div>
                  {cloudUploadOpen ? (
                    <div className="mt-3 flex flex-col gap-2 md:flex-row md:items-center">
                      <input
                        type="password"
                        className="input min-w-0 flex-1"
                        value={cloudPassword}
                        onChange={(event) => setCloudPassword(event.target.value)}
                        placeholder="设置或输入同步密码"
                        autoComplete="new-password"
                      />
                      <button
                        type="button"
                        className="btn-primary px-4 py-2.5"
                        disabled={!canCloudUpload || cloudUploading}
                        onClick={handleCloudUpload}
                      >
                        <Upload className="h-4 w-4" />
                        {cloudUploading ? '上传中…' : '确认上传'}
                      </button>
                    </div>
                  ) : null}
                  <ProgressLine progress={cloudUploadProgress} />
                </div>
              </div>

              <div className="space-y-3">
                <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
                  <div className="text-[13px] font-semibold" style={{ color: 'var(--text-main)' }}>云端备份</div>
                  <button type="button" className="btn-secondary btn-sm" onClick={loadCloudSnapshots} disabled={cloudLoading}>
                    <RefreshCw className={`h-3.5 w-3.5 ${cloudLoading ? 'animate-spin' : ''}`} />
                    刷新
                  </button>
                </div>

                {cloudError ? (
                  <div className="liquid-notice liquid-notice-danger rounded-[12px] px-3 py-2 text-[12px]">
                    {cloudError}
                  </div>
                ) : null}

                <div className="space-y-2">
                  {cloudLoading && cloudSnapshots.length === 0 ? (
                    <div className="rounded-[12px] border px-3 py-5 text-center text-[12px]" style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}>
                      正在读取云端备份…
                    </div>
                  ) : cloudSnapshots.length === 0 ? (
                    <div className="rounded-[12px] border px-3 py-5 text-center text-[12px]" style={{ borderColor: 'var(--control-border)', color: 'var(--text-secondary)' }}>
                      暂无云端备份
                    </div>
                  ) : (
                    cloudSnapshots.map((snapshot) => {
                      const active = snapshot.id === cloudSelectedSnapshotId
                      return (
                        <div
                          key={snapshot.id}
                          className="rounded-[12px] border px-3 py-2.5"
                          style={{
                            borderColor: active ? 'rgba(10, 102, 255, 0.24)' : 'var(--control-border)',
                            background: active ? 'var(--control-active)' : 'var(--surface-control)',
                          }}
                        >
                          <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
                            <button
                              type="button"
                              className="min-w-0 flex-1 text-left"
                              onClick={() => handleSelectCloudSnapshot(snapshot.id)}
                            >
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="font-mono text-[11px]" style={{ color: 'var(--text-tertiary)' }}>
                                  {snapshot.id}
                                </span>
                                <span className="text-[12px] font-semibold" style={{ color: 'var(--text-main)' }}>
                                  {formatDateTime(snapshot.createdAt)}
                                </span>
                                <span className="rounded-full px-2 py-0.5 text-[10px] font-medium" style={{ background: 'var(--status-info-bg)', color: 'var(--status-info-text)' }}>
                                  {formatBytes(snapshot.packageSizeBytes)}
                                </span>
                              </div>
                              <div className="mt-1 truncate text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                                {cloudSnapshotSections(snapshot.sections, sections)}
                              </div>
                            </button>
                            <div className="flex shrink-0 items-center gap-2">
                              <button
                                type="button"
                                className="btn-secondary btn-sm"
                                onClick={() => handleSelectCloudSnapshot(snapshot.id)}
                              >
                                <CloudDownload className="h-3.5 w-3.5" />
                                恢复
                              </button>
                              <button
                                type="button"
                                className="btn-ghost btn-sm"
                                disabled={cloudDeletingId === snapshot.id}
                                onClick={() => void handleDeleteCloudSnapshot(snapshot)}
                                aria-label="删除云端备份"
                                title="删除云端备份"
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </button>
                            </div>
                          </div>
                        </div>
                      )
                    })
                  )}
                </div>

                {cloudSelectedSnapshot ? (
                  <div className="rounded-[14px] border px-3 py-3" style={{ borderColor: 'var(--control-border)', background: 'var(--surface-control)' }}>
                    <div className="flex flex-col gap-2 md:flex-row md:items-center">
                      <input
                        type="password"
                        className="input min-w-0 flex-1"
                        value={cloudRestorePassword}
                        onChange={(event) => setCloudRestorePassword(event.target.value)}
                        placeholder="输入同步密码后解密校验"
                        autoComplete="current-password"
                      />
                      <button
                        type="button"
                        className="btn-primary px-4 py-2.5"
                        onClick={handleCloudDownload}
                        disabled={cloudDownloading}
                      >
                        <CloudDownload className="h-4 w-4" />
                        {cloudDownloading ? '解密中…' : '下载校验'}
                      </button>
                    </div>
                    <ProgressLine progress={cloudDownloadProgress} />

                    {cloudDownloaded ? (
                      <div className="mt-3 space-y-3">
                        <div
                          className={`liquid-notice ${cloudDownloaded.canImport ? 'liquid-notice-success' : 'liquid-notice-danger'} rounded-[12px] px-3 py-2 text-[12px]`}
                        >
                          <div className="flex items-center gap-2">
                            {cloudDownloaded.canImport ? <CheckCircle2 className="h-4 w-4" /> : <AlertTriangle className="h-4 w-4" />}
                            <span>
                              {cloudDownloaded.canImport
                                ? `云端备份归属 ${cloudDownloaded.packageOwnerUsername}，可导入`
                                : cloudDownloaded.blockingReason || '云端备份不能导入'}
                            </span>
                          </div>
                        </div>

                        <div className="space-y-2">
                          {cloudDownloaded.sections.map((section) => (
                            <SectionRow
                              key={section.key}
                              section={section}
                              checked={section.required || !!cloudRestoreSelection[section.key]}
                              disabled={section.required || cloudRestoring || !cloudDownloaded.canImport}
                              onChange={(checked) => setCloudRestoreSelection((prev) => ({ ...prev, [section.key]: checked }))}
                            />
                          ))}
                        </div>

                        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                          <div className="flex items-center gap-2 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
                            <Database className="h-4 w-4" />
                            当前选择原始数据约 {formatBytes(cloudRestoreSize)}
                          </div>
                          <button
                            type="button"
                            className="btn-primary px-4 py-2.5"
                            disabled={cloudRestoring || !cloudDownloaded.canImport}
                            onClick={handleCloudRestore}
                          >
                            <Upload className="h-4 w-4" />
                            导入并重启
                          </button>
                        </div>
                        <ProgressLine progress={cloudRestoreProgress} />
                      </div>
                    ) : null}
                  </div>
                ) : null}
              </div>
            </div>
          </Panel>}
        </div>
      </div>
    </div>
  )
}
