import { describe, expect, it } from 'vitest'
import { getAccountDataImportAccept } from './filePicker'

describe('account data import file picker', () => {
  it('allows Android document providers to select custom ygjdata files', () => {
    expect(getAccountDataImportAccept('Mozilla/5.0 (Linux; Android 14; Redmi K40) AppleWebKit/537.36')).toBe('*/*')
  })

  it('keeps desktop picker focused on account data packages', () => {
    const accept = getAccountDataImportAccept('Mozilla/5.0 (Macintosh; Intel Mac OS X 15_5) AppleWebKit/537.36')

    expect(accept).toContain('.ygjdata')
    expect(accept).toContain('application/zip')
  })
})
