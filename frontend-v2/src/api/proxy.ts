import { request } from './client'

// ==================== 代理 IP 管理 ====================

export const proxyStatus = () => request('GET', '/api/proxy/status')
export const proxyList = () => request('GET', '/api/proxy/list')
export const proxyAdd = (url: string, label = '') => request('POST', '/api/proxy/add', { url, label })
export const proxyBatchAdd = (proxies: string[]) => request('POST', '/api/proxy/batch_add', { proxies })
export const proxyRemove = (url: string) => request('POST', '/api/proxy/remove', { url })
export const proxySetEnabled = (enabled: boolean) => request('POST', '/api/proxy/set_enabled', { enabled })
export const proxySetStrategy = (strategy: string) => request('POST', '/api/proxy/set_strategy', { strategy })
export const proxyReset = (url: string) => request('POST', '/api/proxy/reset', { url })
export const proxyCheck = (url: string) => request('POST', '/api/proxy/check', { url })
export const proxyCheckAll = () => request('POST', '/api/proxy/check_all')
