import { defineStore } from 'pinia'
import { ref } from 'vue'
import { fetchGlobalQuestCatalog, fetchGlobalQuestRegions } from '@/api/globalQuests'
import { emptyGlobalQuestFilters } from '@/utils/globalQuests'
import type { GlobalQuest, GlobalQuestRestriction } from '@/types/globalQuests'

export const GLOBAL_QUEST_CACHE_TTL = 5 * 60_000

export const useGlobalQuestsStore = defineStore('globalQuests', () => {
  const quests = ref<GlobalQuest[]>([])
  const restrictions = ref<Record<string, GlobalQuestRestriction>>({})
  const filters = ref(emptyGlobalQuestFilters())
  const showFilters = ref(false)
  const loading = ref(false)
  const hasLoaded = ref(false)
  const catalogError = ref(false)
  const regionsError = ref(false)
  const updatedAt = ref<number | null>(null)
  const regionsUpdatedAt = ref<number | null>(null)
  let inFlight: Promise<void> | null = null
  let initialized = false

  function initialize(): void {
    if (initialized) return
    initialized = true
    void refresh()
  }

  function refresh(force = false): Promise<void> {
    if (inFlight) return inFlight
    if (!force && updatedAt.value !== null && !catalogError.value && !regionsError.value
      && Date.now() - updatedAt.value < GLOBAL_QUEST_CACHE_TTL) return Promise.resolve()
    loading.value = true
    // Publish each dataset as soon as it arrives; a slow regions response must
    // not hold back the catalog or replace it with a loading screen.
    const catalog = fetchGlobalQuestCatalog().then(data => {
      quests.value = data
      hasLoaded.value = true
      updatedAt.value = Date.now()
      catalogError.value = false
    }).catch(() => { catalogError.value = true })
    const regions = fetchGlobalQuestRegions().then(data => {
      restrictions.value = data
      regionsUpdatedAt.value = Date.now()
      regionsError.value = false
    }).catch(() => { regionsError.value = true })
    inFlight = Promise.all([catalog, regions]).then(() => undefined)
      .finally(() => { loading.value = false; inFlight = null })
    return inFlight
  }

  function clearFilters() { filters.value = emptyGlobalQuestFilters() }
  return { quests, restrictions, filters, showFilters, loading, hasLoaded, catalogError, regionsError,
    updatedAt, regionsUpdatedAt, initialize, refresh, clearFilters }
})
