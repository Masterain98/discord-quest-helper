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

  function refresh(force = false): Promise<void> {
    if (inFlight) return inFlight
    if (!force && updatedAt.value !== null && !catalogError.value && !regionsError.value
      && Date.now() - updatedAt.value < GLOBAL_QUEST_CACHE_TTL) return Promise.resolve()
    loading.value = true
    inFlight = (async () => {
      const [catalog, regions] = await Promise.allSettled([fetchGlobalQuestCatalog(), fetchGlobalQuestRegions()])
      catalogError.value = catalog.status === 'rejected'
      regionsError.value = regions.status === 'rejected'
      if (catalog.status === 'fulfilled') {
        quests.value = catalog.value
        hasLoaded.value = true
        updatedAt.value = Date.now()
      }
      if (regions.status === 'fulfilled') {
        restrictions.value = regions.value
        regionsUpdatedAt.value = Date.now()
      }
    })().finally(() => { loading.value = false; inFlight = null })
    return inFlight
  }

  function clearFilters() { filters.value = emptyGlobalQuestFilters() }
  return { quests, restrictions, filters, showFilters, loading, hasLoaded, catalogError, regionsError,
    updatedAt, regionsUpdatedAt, refresh, clearFilters }
})
