import { describe, expect, it } from 'vitest'
import { dicebearAvatarStyles, generatedAvatarDataUri } from './generatedAvatar'

describe('generatedAvatarDataUri', () => {
  it('uses DiceBear 9.x png avatars with supported styles', () => {
    const url = generatedAvatarDataUri('17452146585')
    const parsed = new URL(url)
    const style = parsed.pathname.split('/')[2]

    expect(parsed.origin).toBe('https://api.dicebear.com')
    expect(parsed.pathname).toMatch(/^\/9\.x\/[^/]+\/png$/)
    expect(dicebearAvatarStyles).toContain(style)
    expect(parsed.searchParams.get('seed')).toBe('17452146585')
    expect(parsed.searchParams.get('size')).toBe('256')
    expect(parsed.searchParams.get('radius')).toBe('50')
    expect(parsed.searchParams.get('backgroundColor')).toBe('b6e3f4,c0aede,d1d4f9,ffd5dc,ffdfbf')
  })
})
