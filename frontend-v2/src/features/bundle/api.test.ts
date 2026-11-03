import { beforeEach, describe, expect, it, vi } from 'vitest'

const { emitClientLogMock, invokeMock, listenMock, unlistenMock } = vi.hoisted(() => ({
  emitClientLogMock: vi.fn(),
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  unlistenMock: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: listenMock,
}))

vi.mock('../../api/client', () => ({
  emitClientLog: emitClientLogMock,
}))

describe('account data cloud sync api wrappers', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    listenMock.mockResolvedValue(unlistenMock)
  })

  it('passes the user-selected export path to the desktop command', async () => {
    invokeMock.mockResolvedValueOnce({ path: '/tmp/accounts.ygjdata', sizeBytes: 1 })

    const { exportAccountDataPackage } = await import('./api')

    await exportAccountDataPackage(['accounts'], '/tmp/accounts.ygjdata')

    expect(invokeMock).toHaveBeenCalledWith('export_account_data_package', {
      sections: ['accounts'],
      outputPath: '/tmp/accounts.ygjdata',
    })
  })

  it('invokes upload/list/download/restore/delete cloud commands with stable arguments', async () => {
    invokeMock.mockResolvedValueOnce([])
    invokeMock.mockResolvedValueOnce({ snapshot: { id: 'snapshot-1' }, message: 'ok' })
    invokeMock.mockResolvedValueOnce({ importToken: 'import-token-1', sections: [] })
    invokeMock.mockResolvedValueOnce({ restartRequired: true, message: 'ok' })
    invokeMock.mockResolvedValueOnce(undefined)

    const {
      deleteAccountDataCloudSnapshot,
      downloadAccountDataCloudSnapshot,
      listAccountDataCloudSnapshots,
      restoreAccountDataCloudSnapshot,
      uploadAccountDataCloudSnapshot,
    } = await import('./api')

    await listAccountDataCloudSnapshots()
    await uploadAccountDataCloudSnapshot(['accounts', 'market'], 'sync-pass')
    await downloadAccountDataCloudSnapshot('snapshot-1', 'sync-pass')
    await restoreAccountDataCloudSnapshot('import-token-1', ['accounts'])
    await deleteAccountDataCloudSnapshot('snapshot-1')

    expect(invokeMock).toHaveBeenNthCalledWith(1, 'list_account_data_cloud_snapshots', undefined)
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'upload_account_data_cloud_snapshot', {
      sections: ['accounts', 'market'],
      password: 'sync-pass',
    })
    expect(invokeMock).toHaveBeenNthCalledWith(3, 'download_account_data_cloud_snapshot', {
      snapshotId: 'snapshot-1',
      password: 'sync-pass',
    })
    expect(invokeMock).toHaveBeenNthCalledWith(4, 'restore_account_data_cloud_snapshot', {
      importToken: 'import-token-1',
      sections: ['accounts'],
    })
    expect(invokeMock).toHaveBeenNthCalledWith(5, 'delete_account_data_cloud_snapshot', {
      snapshotId: 'snapshot-1',
    })
  })

  it('records account data transfer telemetry without leaking sync password or archive content', async () => {
    invokeMock.mockResolvedValueOnce({
      snapshot: {
        id: 'snapshot-1',
        ownerUsername: '90000000003',
        packageSizeBytes: 43 * 1024 * 1024,
        encryptedSizeBytes: 44 * 1024 * 1024,
        sections: ['accounts', 'market'],
      },
      message: 'ok',
    })
    invokeMock.mockResolvedValueOnce({
      importToken: 'import-token-1',
      packageOwnerUsername: '90000000003',
      currentUsername: '90000000003',
      canImport: true,
      blockingReason: null,
      sections: [],
      exportedAt: '2026-07-06T00:00:00Z',
      appVersion: '0.1.4',
    })

    const { inspectAccountDataPackage, uploadAccountDataCloudSnapshot } = await import('./api')

    await uploadAccountDataCloudSnapshot(['accounts', 'market'], 'sync-pass')
    await inspectAccountDataPackage('raw-archive-base64-content')

    const logPayload = JSON.stringify(emitClientLogMock.mock.calls.map(([event]) => event))
    expect(logPayload).toContain('upload_account_data_cloud_snapshot_started')
    expect(logPayload).toContain('upload_account_data_cloud_snapshot_success')
    expect(logPayload).toContain('inspect_account_data_package_started')
    expect(logPayload).not.toContain('sync-pass')
    expect(logPayload).not.toContain('raw-archive-base64-content')
  })

  it('records failed cloud sync actions as error telemetry', async () => {
    invokeMock.mockRejectedValueOnce(new Error('HTTP 413'))

    const { uploadAccountDataCloudSnapshot } = await import('./api')

    await expect(uploadAccountDataCloudSnapshot(['accounts'], 'sync-pass')).rejects.toThrow('HTTP 413')

    expect(emitClientLogMock).toHaveBeenCalledWith(expect.objectContaining({
      level: 'error',
      action: 'upload_account_data_cloud_snapshot_failed',
      errorCode: 'upload_account_data_cloud_snapshot_failed',
      detail: 'HTTP 413',
      promoteToIncident: true,
    }))
  })

  it('subscribes to real cloud upload progress events when requested', async () => {
    let progressHandler: ((event: { payload: unknown }) => void) | null = null
    listenMock.mockImplementationOnce(async (_eventName, handler) => {
      progressHandler = handler
      return unlistenMock
    })
    invokeMock.mockImplementationOnce(async () => {
      progressHandler?.({
        payload: {
          percent: 64,
          label: '正在上传云端 3/8',
          phase: 'upload',
          packageSizeBytes: 43,
          encryptedSizeBytes: 44,
          uploadedBytes: 22,
          totalBytes: 44,
          chunkIndex: 3,
          chunkCount: 8,
        },
      })
      return { snapshot: { id: 'snapshot-1' }, message: 'ok' }
    })

    const { ACCOUNT_DATA_CLOUD_UPLOAD_PROGRESS_EVENT, uploadAccountDataCloudSnapshot } = await import('./api')
    const onProgress = vi.fn()

    await uploadAccountDataCloudSnapshot(['accounts'], 'sync-pass', onProgress)

    expect(listenMock).toHaveBeenCalledWith(ACCOUNT_DATA_CLOUD_UPLOAD_PROGRESS_EVENT, expect.any(Function))
    expect(onProgress).toHaveBeenCalledWith(expect.objectContaining({
      percent: 64,
      uploadedBytes: 22,
      totalBytes: 44,
    }))
    expect(unlistenMock).toHaveBeenCalled()
  })
})
