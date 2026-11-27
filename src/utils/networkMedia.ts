import { convertFileSrc, isTauri } from '@tauri-apps/api/core'

export function networkMediaUrl(source: string): string
export function networkMediaUrl(source: string | null | undefined): string | undefined
export function networkMediaUrl(source: string | null | undefined): string | undefined {
  if (!source || !isTauri() || !/^(?:https?:)?\/\//i.test(source)) return source ?? undefined
  const url = source.startsWith('//') ? `https:${source}` : source
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'https:' || (parsed.hostname !== 'cdn.discordapp.com' && parsed.hostname !== 'media.discordapp.net')
      || parsed.username || parsed.password || parsed.port || parsed.href.includes('#')) return source
    return `${convertFileSrc('asset', 'network-media')}?${new URLSearchParams({ source: url })}`
  } catch {
    return source
  }
}
