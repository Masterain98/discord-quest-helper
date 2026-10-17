import type { ConsoleException, RemoteObject } from '@/api/discordConsole'

export const consoleTabEnabled = (mode: string, available: boolean) => mode === 'cdp' && available
export const consoleFallbackTab = <T extends string>(current: T, enabled: boolean): T | 'settings' => current === 'console' && !enabled ? 'settings' : current

export function formatRemoteObject(object: RemoteObject): string {
  if (object.unserializableValue !== undefined) return object.unserializableValue
  if (object.type === 'undefined') return 'undefined'
  if (object.subtype === 'null') return 'null'
  if (object.value !== undefined) return typeof object.value === 'string' ? object.value : JSON.stringify(object.value)
  const preview = object.preview
  if (preview?.properties?.length) {
    return `${object.description ?? preview.description ?? object.type} { ${preview.properties.map(p => `${p.name}: ${p.value ?? p.type}`).join(', ')}${preview.overflow ? ', …' : ''} }`
  }
  return object.description ?? object.className ?? object.type
}

export function formatConsoleException(exception: ConsoleException): string {
  const text = exception.exception ? formatRemoteObject(exception.exception) : exception.text
  if (text.includes('\n    at ')) return text
  const frames = exception.stackTrace?.callFrames ?? []
  return [text, ...frames.map(f => `    at ${f.functionName || '<anonymous>'} (${f.url}:${f.lineNumber + 1}:${f.columnNumber + 1})`)].join('\n')
}

/** Protocol handles only; a user's by-value object may also have an objectId key. */
export function remoteObjectIds(value: unknown): string[] {
  const ids = new Set<string>()
  function visit(item: unknown) {
    if (!item || typeof item !== 'object') return
    if (Array.isArray(item)) { item.forEach(visit); return }
    const record = item as Record<string, unknown>
    if (typeof record.objectId === 'string') ids.add(record.objectId)
    for (const [key, child] of Object.entries(record)) if (key !== 'value' || typeof record.type !== 'string') visit(child)
  }
  visit(value)
  return [...ids]
}
