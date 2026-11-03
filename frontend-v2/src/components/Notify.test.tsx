import { renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import React from 'react'
import { NotifyProvider, useNotify } from './Notify'

function wrapper({ children }: { children: React.ReactNode }) {
  return <NotifyProvider>{children}</NotifyProvider>
}

describe('useNotify', () => {
  it('supports callable notify and method helpers with backward compatibility', () => {
    const { result } = renderHook(() => useNotify(), { wrapper })

    expect(typeof result.current).toBe('function')
    expect(typeof result.current.success).toBe('function')
    expect(typeof result.current.error).toBe('function')
    expect(typeof result.current.info).toBe('function')
    expect(typeof result.current.show).toBe('function')

    expect(() => result.current('账号刷新完成', 'success', 1200)).not.toThrow()
    expect(() => result.current('error', '会话仍不可用', 1200)).not.toThrow()
    expect(() => result.current.success('已保存', 1200)).not.toThrow()
    expect(() => result.current.error('保存失败', 1200)).not.toThrow()
    expect(() => result.current.info('处理中', 1200)).not.toThrow()
    expect(() => result.current.show('通用提示', 'info', 1200)).not.toThrow()
  })
})
