import { isTauriRuntime } from '../../tauriRuntime'

export interface AppVersionInfo {
  current_version: string
  update_configured: boolean
}

export interface AppUpdateCheckResult {
  configured: boolean
  policy_configured: boolean
  current_version: string
  available: boolean
  version: string | null
  body: string | null
  date: string | null
  minimum_version: string | null
  blocked: boolean
  block_reason: string | null
  download_url: string | null
  download_page_url: string | null
}

export interface AppUpdateInstallResult {
  mode: 'tauri_updater' | 'downloaded_installer' | 'android_package_installer'
  restart_required: boolean
  message: string
  path: string | null
}

async function invokeSafe<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, args)
}

export async function getAppVersionInfo(): Promise<AppVersionInfo | null> {
  if (!isTauriRuntime()) return null
  return invokeSafe<AppVersionInfo>('get_app_version_info')
}

export async function checkAppUpdate(): Promise<AppUpdateCheckResult> {
  if (!isTauriRuntime()) {
    return {
      configured: false,
      policy_configured: false,
      current_version: 'local',
      available: false,
      version: null,
      body: null,
      date: null,
      minimum_version: null,
      blocked: false,
      block_reason: null,
      download_url: null,
      download_page_url: null,
    }
  }
  return invokeSafe<AppUpdateCheckResult>('check_app_update')
}

export async function installAppUpdate(params: { targetVersion?: string | null } = {}) {
  if (!isTauriRuntime()) return null
  return invokeSafe<AppUpdateInstallResult>('install_app_update', {
    targetVersion: params.targetVersion ?? null,
  })
}

export async function requestAppRestart(): Promise<void> {
  if (!isTauriRuntime()) return
  return invokeSafe<void>('request_app_restart')
}
