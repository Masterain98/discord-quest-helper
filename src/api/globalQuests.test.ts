import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchGlobalQuestCatalog, fetchGlobalQuestRegions, GLOBAL_QUEST_TIMEOUT } from './globalQuests'

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers() })
describe('public API transport', () => {
  it('uses fixed public URLs and omits credentials and authorization', async () => {
    const fetch = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ quests: [] }))))
    vi.stubGlobal('fetch', fetch)
    await fetchGlobalQuestCatalog(); await fetchGlobalQuestRegions()
    expect(fetch.mock.calls.map(call => call[0])).toEqual(['https://api.discordquest.com/api/quests', 'https://api.discordquest.com/api/regions'])
    const options = fetch.mock.calls[0][1]
    expect(options.credentials).toBe('omit')
    expect(options.cache).toBe('no-store')
    expect(options.headers).toEqual({ Accept: 'application/json' })
    expect(options.signal.aborted).toBe(false)
  })
  it('rejects HTTP errors and incompatible response shapes', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(new Response('', { status: 403 }))
      .mockResolvedValueOnce(new Response('{"unexpected":true}')))
    await expect(fetchGlobalQuestCatalog()).rejects.toThrow('HTTP 403')
    await expect(fetchGlobalQuestRegions()).rejects.toThrow('Invalid quest dataset')
  })
  it('aborts a stalled request after 20 seconds and releases its timer', async () => {
    vi.useFakeTimers()
    vi.stubGlobal('fetch', vi.fn((_url, { signal }: RequestInit) => new Promise((_resolve, reject) => {
      signal!.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')))
    })))
    const assertion = expect(fetchGlobalQuestCatalog()).rejects.toThrow('Aborted')
    await vi.advanceTimersByTimeAsync(GLOBAL_QUEST_TIMEOUT)
    await assertion
    expect(vi.getTimerCount()).toBe(0)
  })
  it('keeps the timeout active while reading the response body', async () => {
    vi.useFakeTimers()
    vi.stubGlobal('fetch', vi.fn((_url, { signal }: RequestInit) => Promise.resolve({ ok: true,
      json: () => new Promise((_resolve, reject) => signal!.addEventListener('abort', () => reject(new Error('Body timeout')))),
    })))
    const assertion = expect(fetchGlobalQuestCatalog()).rejects.toThrow('Body timeout')
    await vi.advanceTimersByTimeAsync(GLOBAL_QUEST_TIMEOUT)
    await assertion
    expect(vi.getTimerCount()).toBe(0)
  })
})
