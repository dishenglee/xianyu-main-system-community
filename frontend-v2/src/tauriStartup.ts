import { isTauriRuntime } from './tauriRuntime'
import { sanitizeServiceText } from './utils/errorText'

export const TAURI_BOOT_TIMEOUT_MS = 2000
export const TAURI_BOOT_POLL_MS = 50

export function renderBootStatus(root: HTMLElement, title: string, detail: string) {
  root.replaceChildren()

  const shell = document.createElement('div')
  shell.style.minHeight = '100vh'
  shell.style.display = 'flex'
  shell.style.alignItems = 'center'
  shell.style.justifyContent = 'center'
  shell.style.padding = '24px'
  shell.style.background = '#f5f5f5'

  const panel = document.createElement('div')
  panel.style.width = '100%'
  panel.style.maxWidth = '520px'
  panel.style.padding = '24px'
  panel.style.borderRadius = '20px'
  panel.style.background = '#ffffff'
  panel.style.boxShadow = '0 12px 40px rgba(15, 23, 42, 0.08)'

  const heading = document.createElement('h1')
  heading.textContent = title
  heading.style.margin = '0'
  heading.style.fontSize = '24px'
  heading.style.fontWeight = '600'
  heading.style.color = '#111827'

  const paragraph = document.createElement('p')
  paragraph.textContent = sanitizeServiceText(detail, '应用启动失败，请完全关闭后重新打开')
  paragraph.style.margin = '12px 0 0'
  paragraph.style.fontSize = '14px'
  paragraph.style.lineHeight = '1.7'
  paragraph.style.color = '#4b5563'

  panel.append(heading, paragraph)
  shell.append(panel)
  root.append(shell)
}

export async function waitForTauriRuntime(options: {
  detect?: () => boolean
  timeoutMs?: number
  pollMs?: number
} = {}): Promise<boolean> {
  const detect = options.detect ?? isTauriRuntime
  const timeoutMs = options.timeoutMs ?? TAURI_BOOT_TIMEOUT_MS
  const pollMs = options.pollMs ?? TAURI_BOOT_POLL_MS
  const startedAt = Date.now()

  while (Date.now() - startedAt < timeoutMs) {
    if (detect()) return true
    await new Promise((resolve) => window.setTimeout(resolve, pollMs))
  }

  return detect()
}
