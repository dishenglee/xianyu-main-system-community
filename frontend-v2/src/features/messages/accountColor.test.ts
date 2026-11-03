import { describe, expect, it } from 'vitest'

import { colorForAccount } from './accountColor'

describe('colorForAccount', () => {
  it('returns the neutral gray regardless of uid (per minimalist redesign)', () => {
    expect(colorForAccount('abc')).toBe('#C7C7CC')
    expect(colorForAccount('u2')).toBe('#C7C7CC')
    expect(colorForAccount('')).toBe('#C7C7CC')
  })

  it('is deterministic (same uid always maps to same colour)', () => {
    expect(colorForAccount('x')).toBe(colorForAccount('x'))
  })
})
