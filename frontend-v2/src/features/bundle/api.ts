/**
 * Tauri wrappers for account data transfer.
 *
 * The current product flow uses `.ygjdata` packages for same-user account data
 * backup/restore. The legacy QR bundle commands remain available for older
 * callers but are no longer used by the page.
 */

import { emitClientLog, type ClientLogEvent } from '../../api/client'

export interface BundleEnvelope {
  url: string
  qr_svg: string
  key_b64: string
}

export interface AccountDataSectionPresentation {
  key: string
  label: string
  description: string
  required: boolean
  present: boolean
  sizeBytes: number
  selectedByDefault: boolean
}

export interface AccountDataTransferPreview {
  ownerUsername: string
  sections: AccountDataSectionPresentation[]
  totalSizeBytes: number
}

export interface AccountDataExportResult {
  path: string
  fileName: string
  ownerUsername: string
  sections: AccountDataSectionPresentation[]
  sizeBytes: number
}

export interface AccountDataImportInspection {
  importToken: string
  packageOwnerUsername: string
  currentUsername: string
  canImport: boolean
  blockingReason: string | null
  sections: AccountDataSectionPresentation[]
  exportedAt: string
  appVersion: string
}

export interface AccountDataRestoreResult {
  restartRequired: boolean
  message: string
}

export interface AccountDataCloudSnapshotPresentation {
  id: string
  ownerUsername: string
  appVersion: string
  createdAt: string
  packageSizeBytes: number
  encryptedSizeBytes: number
  sections: string[]
  ciphertextSha256: string
}

export interface AccountDataCloudUploadResult {
  snapshot: AccountDataCloudSnapshotPresentation
  message: string
}

export interface AccountDataCloudUploadProgress {
  percent: number
  label: string
  phase: string
  packageSizeBytes: number | null
  encryptedSizeBytes: number | null
  uploadedBytes: number | null
  totalBytes: number | null
  chunkIndex: number | null
  chunkCount: number | null
}

export const ACCOUNT_DATA_CLOUD_UPLOAD_PROGRESS_EVENT = 'account-data-cloud-upload-progress'

export interface AccountDataCloudDownloadedPackage {
  importToken: string
  packageOwnerUsername: string
  currentUsername: string
  canImport: boolean
  blockingReason: string | null
  sections: AccountDataSectionPresentation[]
  exportedAt: string
  appVersion: string
}

async function invokeSafe<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, args)
}

type AccountDataLogContext = Record<string, unknown>

function currentRoute(): string {
  return typeof window === 'undefined' ? '/bundle' : window.location.pathname || '/bundle'
}

function normalizeSections(sections: string[] | undefined): string[] {
  return Array.from(new Set((sections || []).map((section) => section.trim()).filter(Boolean)))
}

function sectionCount(sections: { key?: string }[] | undefined): number {
  return Array.isArray(sections) ? sections.length : 0
}

function sectionKeys(sections: { key?: string }[] | undefined): string[] {
  return Array.isArray(sections)
    ? sections.map((section) => String(section.key || '').trim()).filter(Boolean)
    : []
}

function errorDetail(error: unknown): string {
  return error instanceof Error ? error.message : String(error || 'unknown error')
}

function emitAccountDataLog(
  level: ClientLogEvent['level'],
  action: string,
  message: string,
  context: AccountDataLogContext = {},
  detail = '',
) {
  emitClientLog({
    level,
    logger: 'frontend.action',
    module: 'account-data-transfer',
    action,
    route: currentRoute(),
    errorCode: action,
    message,
    detail,
    retryable: level === 'error' && action.includes('cloud'),
    promoteToIncident: level === 'error',
    context,
  })
}

async function withAccountDataLog<T>(
  action: string,
  startedMessage: string,
  successMessage: string,
  context: AccountDataLogContext,
  run: () => Promise<T>,
  successContext: (result: T) => AccountDataLogContext = () => ({}),
): Promise<T> {
  emitAccountDataLog('info', `${action}_started`, startedMessage, context)
  try {
    const result = await run()
    emitAccountDataLog('info', `${action}_success`, successMessage, {
      ...context,
      ...successContext(result),
    })
    return result
  } catch (error) {
    emitAccountDataLog('error', `${action}_failed`, `${startedMessage}失败`, context, errorDetail(error))
    throw error
  }
}

export async function getAccountDataPackagePreview(): Promise<AccountDataTransferPreview> {
  return withAccountDataLog(
    'preview_account_data_package',
    '读取账号数据大小',
    '账号数据大小读取成功',
    {},
    () => invokeSafe<AccountDataTransferPreview>('get_account_data_package_preview'),
    (result) => ({
      ownerUsername: result.ownerUsername,
      totalSizeBytes: result.totalSizeBytes,
      sectionCount: sectionCount(result.sections),
      sections: (result.sections || []).map((section) => ({
        key: section.key,
        present: section.present,
        sizeBytes: section.sizeBytes,
      })),
    }),
  )
}

export function buildAccountDataExportFileName(ownerUsername: string): string {
  const owner = ownerUsername.trim().replace(/[^\w.-]+/g, '_').replace(/^_+|_+$/g, '') || 'user'
  const stamp = new Date()
    .toISOString()
    .replace(/[-:]/g, '')
    .replace('T', '-')
    .slice(0, 15)
  return `鱼管家账号数据-${owner}-${stamp}.ygjdata`
}

export async function chooseAccountDataExportPath(defaultFileName: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog')
  return save({
    title: '选择账号数据保存位置',
    defaultPath: defaultFileName,
    filters: [{ name: '鱼管家账号数据包', extensions: ['ygjdata'] }],
  })
}

export async function exportAccountDataPackage(sections: string[], outputPath?: string): Promise<AccountDataExportResult> {
  return withAccountDataLog(
    'export_account_data_package',
    '导出账号数据包',
    '账号数据包导出成功',
    {
      sections: normalizeSections(sections),
      hasOutputPath: Boolean(outputPath?.trim()),
    },
    () => invokeSafe<AccountDataExportResult>('export_account_data_package', { sections, outputPath }),
    (result) => ({
      ownerUsername: result.ownerUsername,
      sizeBytes: result.sizeBytes,
      fileName: result.fileName,
      sectionCount: sectionCount(result.sections),
    }),
  )
}

export async function inspectAccountDataPackage(archiveBase64: string): Promise<AccountDataImportInspection> {
  return withAccountDataLog(
    'inspect_account_data_package',
    '校验账号数据包',
    '账号数据包校验完成',
    {
      archiveBase64Length: archiveBase64.length,
    },
    () => invokeSafe<AccountDataImportInspection>('inspect_account_data_package', { archiveBase64 }),
    (result) => ({
      packageOwnerUsername: result.packageOwnerUsername,
      currentUsername: result.currentUsername,
      canImport: result.canImport,
      blockingReason: result.blockingReason || '',
      sectionCount: sectionCount(result.sections),
      sections: sectionKeys(result.sections),
    }),
  )
}

export async function restoreAccountDataPackage(importToken: string, sections: string[]): Promise<AccountDataRestoreResult> {
  return withAccountDataLog(
    'restore_account_data_package',
    '导入账号数据包',
    '账号数据包导入任务已创建',
    {
      sections: normalizeSections(sections),
      hasImportToken: Boolean(importToken.trim()),
    },
    () => invokeSafe<AccountDataRestoreResult>('restore_account_data_package', { importToken, sections }),
    (result) => ({
      restartRequired: result.restartRequired,
      message: result.message,
    }),
  )
}

export async function listAccountDataCloudSnapshots(): Promise<AccountDataCloudSnapshotPresentation[]> {
  return withAccountDataLog(
    'list_account_data_cloud_snapshots',
    '读取云端备份列表',
    '云端备份列表读取成功',
    {},
    () => invokeSafe<AccountDataCloudSnapshotPresentation[]>('list_account_data_cloud_snapshots'),
    (result) => ({
      snapshotCount: result.length,
    }),
  )
}

export async function uploadAccountDataCloudSnapshot(
  sections: string[],
  password: string,
  onProgress?: (progress: AccountDataCloudUploadProgress) => void,
): Promise<AccountDataCloudUploadResult> {
  return withAccountDataLog(
    'upload_account_data_cloud_snapshot',
    '上传云端备份',
    '云端备份上传成功',
    {
      sections: normalizeSections(sections),
      hasPassword: Boolean(password.trim()),
    },
    async () => {
      let unlisten: (() => void) | undefined
      if (onProgress) {
        const { listen } = await import('@tauri-apps/api/event')
        unlisten = await listen<AccountDataCloudUploadProgress>(
          ACCOUNT_DATA_CLOUD_UPLOAD_PROGRESS_EVENT,
          (event) => onProgress(event.payload),
        )
      }
      try {
        return await invokeSafe<AccountDataCloudUploadResult>('upload_account_data_cloud_snapshot', { sections, password })
      } finally {
        unlisten?.()
      }
    },
    (result) => ({
      snapshotId: result.snapshot.id,
      ownerUsername: result.snapshot.ownerUsername,
      packageSizeBytes: result.snapshot.packageSizeBytes,
      encryptedSizeBytes: result.snapshot.encryptedSizeBytes,
      snapshotSections: result.snapshot.sections,
    }),
  )
}

export async function downloadAccountDataCloudSnapshot(snapshotId: string, password: string): Promise<AccountDataCloudDownloadedPackage> {
  return withAccountDataLog(
    'download_account_data_cloud_snapshot',
    '下载云端备份',
    '云端备份下载并校验完成',
    {
      snapshotId,
      hasPassword: Boolean(password.trim()),
    },
    () => invokeSafe<AccountDataCloudDownloadedPackage>('download_account_data_cloud_snapshot', { snapshotId, password }),
    (result) => ({
      packageOwnerUsername: result.packageOwnerUsername,
      currentUsername: result.currentUsername,
      canImport: result.canImport,
      blockingReason: result.blockingReason || '',
      sectionCount: sectionCount(result.sections),
      sections: sectionKeys(result.sections),
    }),
  )
}

export async function restoreAccountDataCloudSnapshot(importToken: string, sections: string[]): Promise<AccountDataRestoreResult> {
  return withAccountDataLog(
    'restore_account_data_cloud_snapshot',
    '恢复云端备份',
    '云端备份恢复任务已创建',
    {
      sections: normalizeSections(sections),
      hasImportToken: Boolean(importToken.trim()),
    },
    () => invokeSafe<AccountDataRestoreResult>('restore_account_data_cloud_snapshot', { importToken, sections }),
    (result) => ({
      restartRequired: result.restartRequired,
      message: result.message,
    }),
  )
}

export async function deleteAccountDataCloudSnapshot(snapshotId: string): Promise<void> {
  return withAccountDataLog(
    'delete_account_data_cloud_snapshot',
    '删除云端备份',
    '云端备份删除成功',
    { snapshotId },
    () => invokeSafe<void>('delete_account_data_cloud_snapshot', { snapshotId }),
  )
}

export async function exportAccountBundle(plaintext: string): Promise<BundleEnvelope> {
  return invokeSafe<BundleEnvelope>('export_account_bundle', { plaintext })
}

export async function importAccountBundle(url: string): Promise<string> {
  return invokeSafe<string>('import_account_bundle', { url })
}
