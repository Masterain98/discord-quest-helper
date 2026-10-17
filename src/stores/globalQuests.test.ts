import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { GLOBAL_QUEST_CACHE_TTL, useGlobalQuestsStore } from './globalQuests'
import { parseGlobalQuests, unknownRestriction } from '@/utils/globalQuests'

const mocks = vi.hoisted(() => ({
  fetchGlobalQuestCatalog: vi.fn(), fetchGlobalQuestRegions: vi.fn(),
  fetchIpInfo: vi.fn(), logIpInfoDiagnostic: vi.fn(),
}))
vi.mock('@/api/globalQuests', () => ({
  fetchGlobalQuestCatalog: mocks.fetchGlobalQuestCatalog,
  fetchGlobalQuestRegions: mocks.fetchGlobalQuestRegions,
}))
vi.mock('@/api/ipInfo', () => ({ fetchIpInfo: mocks.fetchIpInfo }))
vi.mock('@/api/tauri', () => ({ logIpInfoDiagnostic: mocks.logIpInfoDiagnostic }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks(); vi.useFakeTimers()
  vi.stubGlobal('window', { location: { origin: 'http://localhost:1420' } })
  vi.spyOn(console, 'debug').mockImplementation(() => undefined)
  vi.spyOn(console, 'info').mockImplementation(() => undefined)
  vi.spyOn(console, 'error').mockImplementation(() => undefined)
  mocks.fetchGlobalQuestCatalog.mockResolvedValue(parseGlobalQuests([{ id: '111', config: { messages: { quest_name: 'Sample' } } }]))
  mocks.fetchGlobalQuestRegions.mockResolvedValue({ '111': unknownRestriction() })
  mocks.fetchIpInfo.mockResolvedValue({ country: 'Taiwan', countryCode: 'TW' })
  mocks.logIpInfoDiagnostic.mockResolvedValue(undefined)
})
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks() })
describe('public catalog store', () => {
  it('starts all public requests without waiting and initializes only once per app session', async () => {
    const catalog = deferred<[]>()
    const regions = deferred<{}>()
    const location = deferred<{ country: string; countryCode: string }>()
    mocks.fetchGlobalQuestCatalog.mockReturnValue(catalog.promise)
    mocks.fetchGlobalQuestRegions.mockReturnValue(regions.promise)
    mocks.fetchIpInfo.mockReturnValue(location.promise)
    const store = useGlobalQuestsStore()
    expect(store.initialize()).toBeUndefined()
    store.initialize()
    expect(mocks.fetchGlobalQuestCatalog).toHaveBeenCalledTimes(1)
    expect(mocks.fetchGlobalQuestRegions).toHaveBeenCalledTimes(1)
    expect(mocks.fetchIpInfo).toHaveBeenCalledTimes(1)
    expect(store.loading).toBe(true)
    expect(store.ipInfoLoading).toBe(true)
    // Filters remain usable while all network requests are pending.
    store.filters.query = 'Game'
    expect(store.filters.query).toBe('Game')
    catalog.resolve([]); regions.resolve({}); location.resolve({ country: 'Taiwan', countryCode: 'TW' })
    await Promise.all([store.refresh(), store.refreshIpInfo()])
    await vi.advanceTimersByTimeAsync(GLOBAL_QUEST_CACHE_TTL * 2)
    const reopenedStore = useGlobalQuestsStore()
    reopenedStore.initialize()
    expect(reopenedStore).toBe(store)
    expect(reopenedStore.ipInfo?.countryCode).toBe('TW')
    expect(mocks.fetchGlobalQuestCatalog).toHaveBeenCalledTimes(1)
    expect(mocks.fetchGlobalQuestRegions).toHaveBeenCalledTimes(1)
    expect(mocks.fetchIpInfo).toHaveBeenCalledTimes(1)
    expect(vi.getTimerCount()).toBe(0)
  })
  it('does not automatically retry failed startup requests', async () => {
    mocks.fetchGlobalQuestCatalog.mockRejectedValueOnce(new Error('offline'))
    mocks.fetchGlobalQuestRegions.mockRejectedValueOnce(new Error('offline'))
    mocks.fetchIpInfo.mockRejectedValueOnce(new Error('offline'))
    const store = useGlobalQuestsStore()
    store.initialize()
    await Promise.all([store.refresh(), store.refreshIpInfo()])
    expect(store.catalogError).toBe(true)
    expect(store.regionsError).toBe(true)
    expect(store.ipInfoFailed).toBe(true)
    await vi.advanceTimersByTimeAsync(GLOBAL_QUEST_CACHE_TTL * 2)
    store.initialize()
    expect(mocks.fetchGlobalQuestCatalog).toHaveBeenCalledTimes(1)
    expect(mocks.fetchGlobalQuestRegions).toHaveBeenCalledTimes(1)
    expect(mocks.fetchIpInfo).toHaveBeenCalledTimes(1)
    await Promise.all([store.refresh(true), store.refreshIpInfo()])
    expect(store.hasLoaded).toBe(true)
    expect(store.catalogError).toBe(false)
    expect(store.regionsError).toBe(false)
    expect(store.ipInfoFailed).toBe(false)
  })
  it('publishes the catalog while regions are still pending', async () => {
    const regions = deferred<{}>()
    mocks.fetchGlobalQuestRegions.mockReturnValueOnce(regions.promise)
    const store = useGlobalQuestsStore()
    const request = store.refresh()
    await vi.advanceTimersByTimeAsync(0)
    expect(store.hasLoaded).toBe(true)
    expect(store.quests).toHaveLength(1)
    expect(store.updatedAt).not.toBeNull()
    expect(store.regionsUpdatedAt).toBeNull()
    expect(store.loading).toBe(true)
    regions.resolve({})
    await request
    expect(store.loading).toBe(false)
  })
  it('publishes regions while the catalog is still pending', async () => {
    const catalog = deferred<[]>()
    mocks.fetchGlobalQuestCatalog.mockReturnValueOnce(catalog.promise)
    const store = useGlobalQuestsStore()
    const request = store.refresh()
    await vi.advanceTimersByTimeAsync(0)
    expect(store.restrictions['111']).toBeDefined()
    expect(store.regionsUpdatedAt).not.toBeNull()
    expect(store.hasLoaded).toBe(false)
    catalog.resolve([])
    await request
  })
  it('loads both datasets, including an empty successful catalog', async () => {
    mocks.fetchGlobalQuestCatalog.mockResolvedValueOnce([])
    const store = useGlobalQuestsStore(); await store.refresh()
    expect(store.hasLoaded).toBe(true); expect(store.quests).toEqual([]); expect(store.loading).toBe(false)
  })
  it('coalesces simultaneous requests and preserves filters', async () => {
    let resolve!: (value: []) => void
    mocks.fetchGlobalQuestCatalog.mockReturnValue(new Promise<[]>(r => { resolve = r }))
    const store = useGlobalQuestsStore(); store.filters.country = 'US'
    const first = store.refresh(); const second = store.refresh(true)
    expect(mocks.fetchGlobalQuestCatalog).toHaveBeenCalledTimes(1)
    expect(mocks.fetchGlobalQuestRegions).toHaveBeenCalledTimes(1)
    expect(store.loading).toBe(true)
    resolve([]); await Promise.all([first, second])
    expect(store.filters.country).toBe('US')
  })
  it('caches for five minutes and allows forced refresh', async () => {
    const store = useGlobalQuestsStore(); await store.refresh(); await store.refresh()
    expect(mocks.fetchGlobalQuestCatalog).toHaveBeenCalledTimes(1)
    await store.refresh(true); expect(mocks.fetchGlobalQuestCatalog).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(GLOBAL_QUEST_CACHE_TTL)
    await store.refresh(); expect(mocks.fetchGlobalQuestCatalog).toHaveBeenCalledTimes(3)
  })
  it('retains old rows and restrictions on failed refresh and retries despite cache', async () => {
    const store = useGlobalQuestsStore(); await store.refresh()
    const previous = store.quests, previousRules = store.restrictions, previousTime = store.updatedAt
    mocks.fetchGlobalQuestCatalog.mockRejectedValueOnce(new Error('offline'))
    mocks.fetchGlobalQuestRegions.mockRejectedValueOnce(new Error('offline'))
    await store.refresh(true)
    expect(store.quests).toBe(previous); expect(store.restrictions).toBe(previousRules); expect(store.updatedAt).toBe(previousTime)
    expect(store.catalogError).toBe(true); expect(store.regionsError).toBe(true)
    await store.refresh(); expect(store.catalogError).toBe(false); expect(store.regionsError).toBe(false)
  })
  it('shows the catalog when region loading fails, leaving unknown restrictions', async () => {
    mocks.fetchGlobalQuestRegions.mockRejectedValue(new Error('offline'))
    const store = useGlobalQuestsStore(); await store.refresh()
    expect(store.hasLoaded).toBe(true); expect(store.quests).toHaveLength(1)
    expect(store.restrictions).toEqual({}); expect(store.regionsError).toBe(true); expect(store.regionsUpdatedAt).toBeNull()
  })
  it('retains successful region data when the catalog fails and supports recovery', async () => {
    mocks.fetchGlobalQuestCatalog.mockRejectedValueOnce(new Error('offline'))
    const store = useGlobalQuestsStore(); await store.refresh()
    expect(store.hasLoaded).toBe(false); expect(store.catalogError).toBe(true); expect(store.restrictions['111']).toBeDefined()
    await store.refresh(); expect(store.hasLoaded).toBe(true)
  })
  it('clears all filter groups and the name query', () => {
    const store = useGlobalQuestsStore(); store.filters.query = 'Game'; store.filters.ages = ['adult']; store.filters.country = 'US'
    store.clearFilters(); expect(store.filters.query).toBe(''); expect(store.filters.country).toBe(''); expect(store.filters.ages).toEqual([])
  })
})

describe('shared IP country lookup', () => {
  it('coalesces requests and keeps diagnostics from blocking lookup completion', async () => {
    const location = deferred<{ country: string; countryCode: string }>()
    mocks.fetchIpInfo.mockReturnValueOnce(location.promise)
    mocks.logIpInfoDiagnostic.mockReturnValue(new Promise(() => undefined))
    const store = useGlobalQuestsStore()
    const first = store.refreshIpInfo()
    const second = store.refreshIpInfo()
    expect(mocks.fetchIpInfo).toHaveBeenCalledTimes(1)
    expect(store.ipInfoLoading).toBe(true)
    location.resolve({ country: 'Taiwan', countryCode: 'TW' })
    await Promise.all([first, second])
    expect(store.ipInfo?.countryCode).toBe('TW')
    expect(store.ipInfoLoading).toBe(false)
    expect(mocks.logIpInfoDiagnostic).toHaveBeenCalledWith('info', 'lookup succeeded', expect.objectContaining({ countryCode: 'TW' }))
  })
  it('retains cached country on failed refresh and supports manual recovery', async () => {
    const store = useGlobalQuestsStore()
    await store.refreshIpInfo()
    const previous = store.ipInfo
    mocks.fetchIpInfo.mockRejectedValueOnce(new Error('offline'))
    await store.refreshIpInfo()
    expect(store.ipInfo).toBe(previous)
    expect(store.ipInfoFailed).toBe(true)
    expect(store.ipInfoLoading).toBe(false)
    mocks.fetchIpInfo.mockResolvedValueOnce({ country: 'Japan', countryCode: 'JP' })
    await store.refreshIpInfo()
    expect(store.ipInfo?.countryCode).toBe('JP')
    expect(store.ipInfoFailed).toBe(false)
  })
  it('still loads the country when diagnostic forwarding fails', async () => {
    mocks.logIpInfoDiagnostic.mockRejectedValue(new Error('terminal unavailable'))
    const store = useGlobalQuestsStore()
    await store.refreshIpInfo()
    expect(store.ipInfo?.countryCode).toBe('TW')
    expect(store.ipInfoFailed).toBe(false)
    expect(store.ipInfoLoading).toBe(false)
  })
})
