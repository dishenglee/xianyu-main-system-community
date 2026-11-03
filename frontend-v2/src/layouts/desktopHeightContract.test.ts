import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const css = readFileSync(resolve(__dirname, '../styles/main.css'), 'utf8')

describe('desktop height contract', () => {
  it('uses the desktop shell remaining height for all desktop pages', () => {
    expect(css).toContain('--desktop-page-height:')
    expect(css).toContain('.desktop-page-viewport')
    expect(css).toMatch(/\.desktop-shell-main\s*\{[^}]*flex:\s*1 1 auto/s)
    expect(css).toMatch(/\.desktop-shell-main\s*\{[^}]*min-height:\s*0/s)
    expect(css).toMatch(/\.desktop-messages-grid\s*\{[^}]*height:\s*100%/s)
    expect(css).toMatch(/\.desktop-page-viewport\s*\{[^}]*height:\s*100%/s)
    expect(css).not.toContain('.dc-frame')
  })
})
