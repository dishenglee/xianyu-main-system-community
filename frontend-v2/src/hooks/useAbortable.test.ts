import { describe, it, expect } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useAbortable } from './useAbortable'

describe('useAbortable', () => {
  it('provides a signal that is not aborted initially', () => {
    const { result } = renderHook(() => useAbortable())
    const signal = result.current.getSignal()
    expect(signal.aborted).toBe(false)
  })

  it('aborts previous signal when getSignal is called again', () => {
    const { result } = renderHook(() => useAbortable())
    const sig1 = result.current.getSignal()
    const sig2 = result.current.getSignal()
    expect(sig1.aborted).toBe(true)
    expect(sig2.aborted).toBe(false)
  })

  it('aborts signal on explicit abort()', () => {
    const { result } = renderHook(() => useAbortable())
    const signal = result.current.getSignal()
    act(() => { result.current.abort() })
    expect(signal.aborted).toBe(true)
  })

  it('aborts on unmount', () => {
    const { result, unmount } = renderHook(() => useAbortable())
    const signal = result.current.getSignal()
    unmount()
    expect(signal.aborted).toBe(true)
  })
})
