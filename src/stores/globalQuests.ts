import { defineStore } from 'pinia'
import { ref } from 'vue'
import { fetchGlobalQuestCatalog, fetchGlobalQuestRegions } from '@/api/globalQuests'
import { fetchIpInfo, type IpInfo } from '@/api/ipInfo'
import { logIpInfoDiagnostic, type IpInfoDiagnosticLevel } from '@/api/tauri'
import { emptyGlobalQuestFilters } from '@/utils/globalQuests'
import type { GlobalQuest, GlobalQuestRestriction } from '@/types/globalQuests'

export const GLOBAL_QUEST_CACHE_TTL = 5 * 60_000

async function reportIpInfoDiagnostic(
  level: IpInfoDiagnosticLevel,
  event: string,
  details: Record<string, string | number>,
) {
  const prefix = `[ip-info] ${event}`
  if (level === 'error') console.error(prefix, details)
  else if (level === 'warn') console.warn(prefix, details)
  else if (level === 'info') console.info(prefix, details)
  else console.debug(prefix, details)

  try {
    await logIpInfoDiagnostic(level, event, details)
  } catch (error) {
    console.error('[ip-info] failed to forward diagnostics to the app terminal', error)
  }
}

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
  const ipInfo = ref<IpInfo | null>(null)
  const ipInfoFailed = ref(false)
  const ipInfoLoading = ref(false)
  let inFlight: Promise<void> | null = null
  let ipInfoInFlight: Promise<void> | null = null
  let initialized = false

  function initialize(): void {
    if (initialized) return
    initialized = true
    void refresh()
    void refreshIpInfo()
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

  function refreshIpInfo(): Promise<void> {
    if (ipInfoInFlight) return ipInfoInFlight
    ipInfoLoading.value = true
    ipInfoFailed.value = false
    const startedAt = performance.now()
    const origin = window.location.origin
    // Diagnostics must not delay the lookup or keep its loading state active.
    void reportIpInfoDiagnostic('debug', 'lookup started', {
      endpoint: 'https://my.ippure.com/v1/info', origin,
    })
    ipInfoInFlight = fetchIpInfo().then(info => {
      ipInfo.value = info
      void reportIpInfoDiagnostic('info', 'lookup succeeded', {
        countryCode: info.countryCode, country: info.country,
        durationMs: Math.round(performance.now() - startedAt),
      })
    }).catch(error => {
      ipInfoFailed.value = true
      void reportIpInfoDiagnostic('error', 'lookup failed', {
        command: 'get_ip_info', origin,
        errorName: error instanceof Error ? error.name : typeof error === 'string' ? 'TauriCommandError' : 'UnknownError',
        errorMessage: error instanceof Error ? error.message : String(error),
        durationMs: Math.round(performance.now() - startedAt),
      })
    }).finally(() => { ipInfoLoading.value = false; ipInfoInFlight = null })
    return ipInfoInFlight
  }

  function clearFilters() { filters.value = emptyGlobalQuestFilters() }
  return { quests, restrictions, filters, showFilters, loading, hasLoaded, catalogError, regionsError,
    updatedAt, regionsUpdatedAt, ipInfo, ipInfoFailed, ipInfoLoading, initialize, refresh, refreshIpInfo, clearFilters }
})
