import { convertFileSrc, isTauri } from '@tauri-apps/api/core'

export function networkMediaUrl(source: string): string
export function networkMediaUrl(source: string | null | undefined): string | undefined
export function networkMediaUrl(source: string | null | undefined): string | undefined {
  if (!source || !isTauri() || !/^(?:https?:)?\/\//i.test(source)) return source ?? undefined
  const url = source.startsWith('//') ? `https:${source}` : source
  return `${convertFileSrc('asset', 'network-media')}?${new URLSearchParams({ source: url })}`
}
