import { afterEach, describe, expect, it, vi } from 'vitest'
import { redoShortcutLabel, shortcutLabel } from './context-menu.utils'

function stubUserAgent(userAgent: string): void {
  vi.stubGlobal('navigator', { userAgent })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('shortcutLabel', () => {
  it('macOS 渲染 ⌘，不再给出平台上不存在的 Ctrl 键位（issue #858）', () => {
    stubUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15')

    expect(shortcutLabel('V')).toBe('⌘V')
    expect(shortcutLabel('A')).toBe('⌘A')
  })

  it('非 macOS 平台保持 Ctrl+ 文案', () => {
    stubUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')

    expect(shortcutLabel('V')).toBe('Ctrl+V')
  })

  it('没有 navigator 时按非 macOS 兜底，不抛异常', () => {
    vi.stubGlobal('navigator', undefined)

    expect(shortcutLabel('R')).toBe('Ctrl+R')
  })
})

describe('redoShortcutLabel', () => {
  it('macOS 按 AppKit 约定给 ⇧⌘Z 而不是 Ctrl+Y', () => {
    stubUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')

    expect(redoShortcutLabel()).toBe('⇧⌘Z')
  })

  it('非 macOS 平台的重做保持 Ctrl+Y', () => {
    stubUserAgent('Mozilla/5.0 (X11; Linux x86_64)')

    expect(redoShortcutLabel()).toBe('Ctrl+Y')
  })
})
