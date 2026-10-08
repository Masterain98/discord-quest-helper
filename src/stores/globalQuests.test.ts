import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { GLOBAL_QUEST_CACHE_TTL, useGlobalQuestsStore } from './globalQuests'
import { parseGlobalQuests, unknownRestriction } from '@/utils/globalQuests'

const mocks = vi.hoisted(() => ({ fetchGlobalQuestCatalog: vi.fn(), fetchGlobalQuestRegions: vi.fn() }))
vi.mock('@/api/globalQuests', () => mocks)
beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks(); vi.useFakeTimers()
  mocks.fetchGlobalQuestCatalog.mockResolvedValue(parseGlobalQuests([{ id: '111', config: { messages: { quest_name: 'Sample' } } }]))
  mocks.fetchGlobalQuestRegions.mockResolvedValue({ '111': unknownRestriction() })
})
afterEach(() => vi.useRealTimers())
describe('public catalog store', () => {
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
