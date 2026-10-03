function isMacOS(): boolean {
  return typeof navigator !== 'undefined' && navigator.userAgent.includes('Macintosh')
}

export function shortcutLabel(key: string): string {
  return isMacOS() ? `⌘${key}` : `Ctrl+${key}`
}

export function redoShortcutLabel(): string {
  return isMacOS() ? '⇧⌘Z' : 'Ctrl+Y'
}
