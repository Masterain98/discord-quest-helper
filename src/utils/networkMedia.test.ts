import { afterEach, describe, expect, it, vi } from 'vitest'
import { networkMediaUrl } from './networkMedia'

afterEach(() => vi.unstubAllGlobals())

describe('desktop media proxy URL', () => {
  it('routes supported Discord CDN URLs through the native protocol', () => {
    vi.stubGlobal('isTauri', true)
    vi.stubGlobal('window', { __TAURI_INTERNALS__: { convertFileSrc: () => 'network-media://localhost/asset' } })
    for (const original of ['//cdn.discordapp.com/a.png', 'https://media.discordapp.net/a.png?size=64']) {
      const converted = new URL(networkMediaUrl(original))
      expect(converted.protocol).toBe('network-media:')
      expect(converted.searchParams.get('source')).toBe(original.startsWith('//') ? 'https:' + original : original)
    }
  })

  it('preserves sources that the native protocol cannot serve', () => {
    vi.stubGlobal('isTauri', true)
    for (const source of ['https://example.org/image.png', 'http://cdn.discordapp.com/a.png', '/icons/orbs.png', 'https://cdn.discordapp.com:8443/a.png']) {
      expect(networkMediaUrl(source)).toBe(source)
    }
  })
})
