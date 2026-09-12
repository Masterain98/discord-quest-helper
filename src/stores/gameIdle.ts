import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import type {
  GameIdleMode,
  GameIdleItem,
  GameIdleStatus,
  GameSimulationHistoryEntry,
} from '@/api/tauri'
import {
  getGameIdleStatus,
  getGameSimulationHistory,
  onGameIdleStatus,
  onGameSimulationHistoryUpdated,
  removeGameIdleQueueItem,
  startGameIdle,
  stopGameIdle,
  stopGameSimulationUsage,
} from '@/api/tauri'
import { useQuestsStore } from './quests'

const PLAY_MINUTES_KEY = 'questHelper_gameIdlePlayMinutes'
const REST_MINUTES_KEY = 'questHelper_gameIdleRestMinutes'
const MODE_KEY = 'questHelper_gameIdleMode'

function savedInteger(key: string, fallback: number, minimum: number): number {
  const raw = localStorage.getItem(key)
  if (raw === null) return fallback
  const value = Number(raw)
  return Number.isSafeInteger(value) && value >= minimum ? value : fallback
}

export function formatSimulationDuration(totalSeconds: number): { hours: number; minutes: number } {
  const totalMinutes = Math.floor(Math.max(0, totalSeconds) / 60)
  return {
    hours: Math.floor(totalMinutes / 60),
    minutes: totalMinutes % 60,
  }
}

export const useGameIdleStore = defineStore('gameIdle', () => {
  const quests = useQuestsStore()
  const playMinutes = ref(savedInteger(PLAY_MINUTES_KEY, 60, 1))
  const restMinutes = ref(savedInteger(REST_MINUTES_KEY, 0, 0))
  const savedMode = localStorage.getItem(MODE_KEY)
  const mode = ref<GameIdleMode>(savedMode === 'process' || savedMode === 'cdp' ? savedMode : 'process')
  const status = ref<GameIdleStatus | null>(null)
  const history = ref<Record<string, GameSimulationHistoryEntry>>({})
  const loading = ref(false)
  const stopping = ref(false)
  const stopRequested = ref(false)
  const error = ref<string | null>(null)
  let initialized = false
  let statusUnlisten: (() => void) | null = null
  let historyUnlisten: (() => void) | null = null

  const isActive = computed(() => {
    const phase = status.value?.phase
    return phase !== undefined && phase !== 'stopped'
  })

  const sessionTotalSeconds = computed(() => {
    const current = status.value
    if (!current) return 0
    if (current.phase !== 'playing') return current.accumulatedPlayedSeconds
    const elapsed = Math.max(0, Math.floor((Date.now() - current.phaseStartedAt) / 1000))
    return current.accumulatedPlayedSeconds + elapsed
  })

  function persistConfig() {
    localStorage.setItem(PLAY_MINUTES_KEY, String(playMinutes.value))
    localStorage.setItem(REST_MINUTES_KEY, String(restMinutes.value))
    localStorage.setItem(MODE_KEY, mode.value)
  }

  function validateConfig(): string | null {
    if (!Number.isSafeInteger(playMinutes.value) || playMinutes.value <= 0) {
      return 'play_minutes'
    }
    if (!Number.isSafeInteger(restMinutes.value) || restMinutes.value < 0) {
      return 'rest_minutes'
    }
    return null
  }

  async function refreshHistory() {
    const entries = await getGameSimulationHistory()
    history.value = Object.fromEntries(entries.map(entry => [entry.appId, entry]))
  }

  async function initialize() {
    if (initialized) return
    initialized = true
    try {
      await Promise.all([quests.initPlatformCapabilities(), quests.initCdpMode().catch(() => undefined)])
      if (savedMode === null) {
        mode.value = quests.cdpAvailable ? 'cdp' : 'process'
      }
      const [activeStatus] = await Promise.all([
        getGameIdleStatus(),
        refreshHistory().catch(error => console.warn('Failed to load game history:', error)),
      ])
      status.value = activeStatus
      if (activeStatus && activeStatus.phase !== 'stopped') {
        playMinutes.value = activeStatus.playMinutes
        restMinutes.value = activeStatus.restMinutes
        mode.value = activeStatus.mode
      }
      statusUnlisten = await onGameIdleStatus(next => {
        status.value = next
      })
      historyUnlisten = await onGameSimulationHistoryUpdated(entry => {
        history.value = { ...history.value, [entry.appId]: entry }
      })
    } catch (cause) {
      statusUnlisten?.()
      historyUnlisten?.()
      statusUnlisten = null
      historyUnlisten = null
      initialized = false
      throw cause
    }
  }

  async function start() {
    const validation = validateConfig()
    if (validation) throw new Error(validation)
    loading.value = true
    stopRequested.value = false
    error.value = null
    try {
      const [games, simulationPath] = await Promise.all([
        quests.getDetectableGames(),
        quests.initSimulationPath(),
      ])
      persistConfig()
      status.value = await startGameIdle(
        {
          mode: mode.value,
          playMinutes: playMinutes.value,
          restMinutes: restMinutes.value,
          cdpPort: quests.cdpPort,
          simulationPath,
        },
        games
      )
      // A stop click can arrive while candidates/path are still being loaded.
      // The backend session is created by the time this resolves, so finish
      // the requested stop immediately instead of leaving a detached worker.
      if (stopRequested.value) await stop()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    } finally {
      loading.value = false
      if (!isActive.value) stopRequested.value = false
    }
  }

  async function stop() {
    if (stopping.value) return
    stopRequested.value = true
    if (loading.value && !isActive.value) return
    stopRequested.value = false
    stopping.value = true
    error.value = null
    try {
      status.value = await stopGameIdle()
      await refreshHistory()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    } finally {
      stopping.value = false
    }
  }

  async function removeUpcoming(item: GameIdleItem) {
    const current = status.value
    if (!current) return
    // After a session is stopped the backend manager is released, but the
    // final queue remains useful as a preview for the next run. Allow it to be
    // edited locally instead of sending a request to a manager that no longer
    // exists; the next start will build a fresh queue from the candidates.
    if (current.phase === 'stopped') {
      status.value = {
        ...current,
        upcoming: current.upcoming.filter(entry => entry.occurrenceId !== item.occurrenceId),
      }
      return
    }
    const sessionId = current.sessionId
    if (!sessionId) return
    status.value = await removeGameIdleQueueItem(sessionId, item.id, item.occurrenceId)
  }

  async function stopForAccountChange() {
    if (isActive.value) {
      await stop().catch(error => console.warn('Failed to stop game idle mode:', error))
    }
    await stopGameSimulationUsage().catch(() => undefined)
    status.value = null
    history.value = {}
  }

  function dispose() {
    statusUnlisten?.()
    historyUnlisten?.()
    statusUnlisten = null
    historyUnlisten = null
    initialized = false
  }

  return {
    playMinutes,
    restMinutes,
    mode,
    status,
    history,
    loading,
    stopping,
    stopRequested,
    error,
    isActive,
    sessionTotalSeconds,
    initialize,
    refreshHistory,
    start,
    stop,
    removeUpcoming,
    stopForAccountChange,
    validateConfig,
    dispose,
  }
})
