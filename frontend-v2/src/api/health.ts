import { request } from './client'

// ==================== 健康检查 ====================

export const healthCheck = () => request('GET', '/health')
