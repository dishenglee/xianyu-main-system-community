import { describe, it, expect } from 'vitest'
import { getDeviceTypeForWidth } from './useDevice'

describe('getDeviceTypeForWidth (pure logic)', () => {
  it('returns mobile for width < 768', () => {
    expect(getDeviceTypeForWidth(375)).toBe('mobile')
    expect(getDeviceTypeForWidth(767)).toBe('mobile')
  })

  it('returns tablet for 768 <= width < 1024', () => {
    expect(getDeviceTypeForWidth(768)).toBe('tablet')
    expect(getDeviceTypeForWidth(1023)).toBe('tablet')
  })

  it('returns desktop for width >= 1024', () => {
    expect(getDeviceTypeForWidth(1024)).toBe('desktop')
    expect(getDeviceTypeForWidth(1440)).toBe('desktop')
  })
})
