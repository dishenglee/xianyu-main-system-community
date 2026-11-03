/// <reference types="node" />
import { readdirSync, readFileSync, type Dirent } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const currentFile = fileURLToPath(import.meta.url)
const currentDir = path.dirname(currentFile)
const srcRoot = path.resolve(currentDir, '..')
const allowDialogMarkers = ['showAlert(', 'showConfirm(', 'showPrompt(']
const reversedNotifyPattern = /notify\(\s*['"`](success|error|info|warning)['"`]\s*,/g
const selfFile = path.resolve(currentFile)

function walkFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry: Dirent) => {
    const fullPath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules') return []
      return walkFiles(fullPath)
    }
    return entry.name.endsWith('.ts') || entry.name.endsWith('.tsx') ? [fullPath] : []
  })
}

function findNativeDialogCalls(keyword: 'alert(' | 'confirm(' | 'prompt('): string[] {
  return walkFiles(srcRoot).flatMap((file): string[] => {
    if (file === selfFile) return []
    const source = readFileSync(file, 'utf8')
    if (!source.includes(keyword)) return []
    const hits: string[] = []
    const lines = source.split('\n')
    lines.forEach((line: string, index: number) => {
      if (!line.includes(keyword)) return
      if (allowDialogMarkers.some((marker) => line.includes(marker))) return
      hits.push(`${path.relative(srcRoot, file)}:${index + 1}:${line.trim()}`)
    })
    return hits
  })
}

function findReversedNotifyCalls(): string[] {
  return walkFiles(srcRoot).flatMap((file): string[] => {
    if (file === selfFile) return []
    const source = readFileSync(file, 'utf8')
    return Array.from(source.matchAll(reversedNotifyPattern), (match: RegExpMatchArray) => {
      const before = source.slice(0, match.index ?? 0)
      const line = before.split('\n').length
      return `${path.relative(srcRoot, file)}:${line}:${match[0]}`
    })
  })
}

describe('notification consistency', () => {
  it('avoids native alert calls in frontend source', () => {
    expect(findNativeDialogCalls('alert(')).toEqual([])
  })

  it('avoids native confirm calls in frontend source', () => {
    expect(findNativeDialogCalls('confirm(')).toEqual([])
  })

  it('avoids native prompt calls in frontend source', () => {
    expect(findNativeDialogCalls('prompt(')).toEqual([])
  })

  it('uses canonical notify(message, type) ordering', () => {
    expect(findReversedNotifyCalls()).toEqual([])
  })
})
