import { isTauriRuntime } from './tauriRuntime'
import { renderBootStatus, waitForTauriRuntime } from './tauriStartup'

async function bootTauriApp(root: HTMLElement) {
  const [
    { StrictMode, createElement },
    { createRoot },
    ,
    { default: App },
    { attachVitePreloadRecovery },
    { installGlobalClientLogging },
    { useAuthStore },
  ] = await Promise.all([
    import('react'),
    import('react-dom/client'),
    import('./styles/main.css'),
    import('./App.tsx'),
    import('./bootstrap/vitePreloadRecovery'),
    import('./bootstrap/clientLogging'),
    import('./store/authStore'),
  ])

  attachVitePreloadRecovery()
  installGlobalClientLogging()

  useAuthStore.setState({ _hydrated: true })
  createRoot(root).render(createElement(StrictMode, null, createElement(App)))
}

async function startApp(root: HTMLElement) {
  if (isTauriRuntime()) {
    await bootTauriApp(root)
    return
  }

  renderBootStatus(
    root,
    '正在连接桌面运行时',
    '首次启动时会等待桌面桥接和本地服务完成初始化。',
  )

  if (!(await waitForTauriRuntime())) {
    renderBootStatus(
      root,
      '桌面运行时未就绪',
      '没有检测到 Tauri 桌面桥接，请关闭后重新打开应用。',
    )
    return
  }

  await bootTauriApp(root)
}

const root = document.getElementById('root')

if (root) {
  void startApp(root).catch((error) => {
    console.error('[bootstrap] failed to start desktop app:', error)
    renderBootStatus(
      root,
      '桌面应用启动失败',
      error instanceof Error ? error.message : String(error || '未知错误'),
    )
  })
}
