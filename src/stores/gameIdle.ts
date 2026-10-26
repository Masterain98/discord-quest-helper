import { computed, ref, watch, type Ref } from 'vue'
import { defineStore } from 'pinia'
import type {
  GameIdleCrashConfig,
  GameIdleMode,
  GameIdleSelectionMode,
  GameIdleCandidateType,
  DetectableGame,
  GameIdleDuration,
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
} from '@/api/tauri'
import { useQuestsStore } from './quests'
import { clampGameIdleDuration, isGameIdleDurationValid, loadGameIdleDuration } from '@/utils/durationSetting'
import { resolveSimulationExecutable } from '@/utils/executables'

const PLAY_MINUTES_KEY = 'questHelper_gameIdlePlayMinutes'
const REST_MINUTES_KEY = 'questHelper_gameIdleRestMinutes'
const MODE_KEY = 'questHelper_gameIdleMode'
const SELECTION_KEY = 'questHelper_gameIdleSelectionMode'
const CANDIDATE_TYPE_KEY = 'questHelper_gameIdleCandidateType'
const PLAYLIST_KEY = 'questHelper_gameIdlePlaylist'
const DEFAULT_PLAY_DURATION: GameIdleDuration = { mode: 'fixed', fixed: 60, min: 60, max: 60 }
const DEFAULT_REST_DURATION: GameIdleDuration = { mode: 'fixed', fixed: 0, min: 0, max: 0 }
const CRASH_ENABLED_KEY = 'questHelper_gameIdleCrashEnabled'
const CRASH_AFTER_KEY = 'questHelper_gameIdleCrashAfterMinutes'
const REOPEN_DELAY_KEY = 'questHelper_gameIdleReopenDelaySeconds'
const DEFAULT_CRASH_AFTER_DURATION: GameIdleDuration = { mode: 'random', fixed: 10, min: 5, max: 15 }
const DEFAULT_REOPEN_DELAY_DURATION: GameIdleDuration = { mode: 'random', fixed: 45, min: 30, max: 60 }

type PlaylistItem = Pick<DetectableGame, 'id' | 'name' | 'icon' | 'type_name'>

function uniqueGames(games: DetectableGame[]): DetectableGame[] {
  const seen = new Set<string>()
  return games.filter(game => {
    if (seen.has(game.id)) return false
    seen.add(game.id)
    return true
  })
}

function savedPlaylist(): PlaylistItem[] {
  try {
    const data: unknown = JSON.parse(localStorage.getItem(PLAYLIST_KEY) ?? '[]')
    if (!Array.isArray(data)) return []
    const seen = new Set<string>()
    return data.flatMap(item => {
      if (!item || typeof item.id !== 'string' || !item.id.trim() ||
          typeof item.name !== 'string' || !item.name.trim() || seen.has(item.id)) return []
      seen.add(item.id)
      return [{ id: item.id, name: item.name,
        icon: typeof item.icon === 'string' ? item.icon : undefined,
        type_name: item.type_name === 'Game' || item.type_name === 'App' ? item.type_name : undefined }]
    })
  } catch {
    return []
  }
}

function clearStoppedQueue(value: GameIdleStatus | null): GameIdleStatus | null {
  if (!value || value.phase !== 'stopped') return value
  return {
    ...value,
    current: null,
    recent: [],
    upcoming: [],
  }
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
  const playDuration = ref(loadGameIdleDuration(PLAY_MINUTES_KEY, DEFAULT_PLAY_DURATION, 1))
  const restDuration = ref(loadGameIdleDuration(REST_MINUTES_KEY, DEFAULT_REST_DURATION, 0))
  const savedMode = localStorage.getItem(MODE_KEY)
  const mode = ref<GameIdleMode>(savedMode === 'process' || savedMode === 'cdp' ? savedMode : 'process')
  const selectionMode = ref<GameIdleSelectionMode>(localStorage.getItem(SELECTION_KEY) === 'manual' ? 'manual' : 'random')
  const savedCandidateType = localStorage.getItem(CANDIDATE_TYPE_KEY)
  const candidateType = ref<GameIdleCandidateType>(savedCandidateType === 'game' || savedCandidateType === 'app' ? savedCandidateType : 'all')
  const playlist = ref<PlaylistItem[]>(savedPlaylist())
  const crashEnabled = ref(localStorage.getItem(CRASH_ENABLED_KEY) === 'true')
  const crashAfterMinutes = ref(loadGameIdleDuration(CRASH_AFTER_KEY, DEFAULT_CRASH_AFTER_DURATION, 1))
  const reopenDelaySeconds = ref(loadGameIdleDuration(REOPEN_DELAY_KEY, DEFAULT_REOPEN_DELAY_DURATION, 0))
  const status = ref<GameIdleStatus | null>(null)
  const history = ref<Record<string, GameSimulationHistoryEntry>>({})
  const loading = ref(false)
  const stopping = ref(false)
  const stopRequested = ref(false)
  const error = ref<string | null>(null)
  const selectionPersistenceError = ref(false)
  const configPersistenceError = ref(false)
  const persistenceError = computed(() => selectionPersistenceError.value || configPersistenceError.value)
  let initialized = false
  let accountGeneration = 0
  let pendingStart: Promise<void> | null = null
  let statusUnlisten: (() => void) | null = null
  let historyUnlisten: (() => void) | null = null

  const isActive = computed(() => {
    const phase = status.value?.phase
    return phase !== undefined && phase !== 'stopped'
  })

  function persistSelection(config = {
    selectionMode: selectionMode.value,
    candidateType: candidateType.value,
    playlist: playlist.value,
  }) {
    try {
      localStorage.setItem(SELECTION_KEY, config.selectionMode)
      localStorage.setItem(CANDIDATE_TYPE_KEY, config.candidateType)
      localStorage.setItem(PLAYLIST_KEY, JSON.stringify(config.playlist))
      selectionPersistenceError.value = false
    } catch {
      selectionPersistenceError.value = true
    }
  }

  watch([selectionMode, candidateType, playlist], () => persistSelection(), { deep: true, flush: 'sync' })

  // Duration edits can hold transient out-of-range values while the user is
  // typing; normalizing synchronously keeps every read (validation, start
  // payload) within the backend's bounds. An optional key persists the
  // normalized value immediately instead of waiting for the start-scoped
  // persistConfig write.
  function watchClampedDuration(target: Ref<GameIdleDuration>, minimum: number, persistKey?: string) {
    watch(target, (value) => {
      const next = clampGameIdleDuration(value, minimum)
      if (next !== value) target.value = next
      if (persistKey) localStorage.setItem(persistKey, JSON.stringify(next))
    }, { flush: 'sync' })
  }

  watchClampedDuration(playDuration, 1)
  watchClampedDuration(restDuration, 0)
  watchClampedDuration(crashAfterMinutes, 1, CRASH_AFTER_KEY)
  watchClampedDuration(reopenDelaySeconds, 0, REOPEN_DELAY_KEY)
  watch(crashEnabled, (enabled) => {
    localStorage.setItem(CRASH_ENABLED_KEY, String(enabled))
  }, { flush: 'sync' })

  function isCompatible(game: DetectableGame, selectedMode = mode.value): boolean {
    if (selectedMode === 'cdp') return true
    const capabilities = quests.platformCapabilities
    return !!capabilities && resolveSimulationExecutable(game.executables, capabilities.os).kind === 'supported'
  }

  function matchesType(game: DetectableGame, type = candidateType.value): boolean {
    return type === 'all' || game.type_name === (type === 'game' ? 'Game' : 'App')
  }

  const detectableGames = computed(() => uniqueGames(quests.detectableGames))
  function searchCatalog(type: GameIdleCandidateType, query: string): DetectableGame[] {
    const search = query.trim().toLocaleLowerCase()
    return uniqueGames(quests.detectableGames.filter(game => matchesType(game, type) &&
      (game.name.toLocaleLowerCase().includes(search) || game.id.includes(search))))
  }
  const playlistEntries = computed(() => playlist.value.map(saved => {
    const matching = quests.detectableGames.filter(item => item.id === saved.id)
    const game = matching.find(item => item.type_name === saved.type_name && isCompatible(item)) ??
      matching.find(item => isCompatible(item)) ??
      matching.find(item => item.type_name === saved.type_name) ?? matching[0]
    return { ...saved, ...(game ?? {}), unavailable: !game ? 'missing' : !isCompatible(game) ? 'incompatible' : null }
  }))
  const candidates = computed(() => selectionMode.value === 'manual'
    ? playlistEntries.value.filter(entry => !entry.unavailable)
    : uniqueGames(quests.detectableGames.filter(game => matchesType(game) && isCompatible(game))))
  const candidateCount = computed(() => candidates.value.length)

  function addToPlaylist(game: DetectableGame) {
    if (isActive.value || loading.value || playlist.value.some(item => item.id === game.id)) return
    playlist.value.push({ id: game.id, name: game.name, icon: game.icon, type_name: game.type_name })
  }

  function removeFromPlaylist(id: string) {
    if (isActive.value || loading.value) return
    playlist.value = playlist.value.filter(item => item.id !== id)
  }

  function movePlaylistItem(id: string, direction: -1 | 1) {
    if (isActive.value || loading.value) return
    const index = playlist.value.findIndex(item => item.id === id)
    const target = index + direction
    if (index < 0 || target < 0 || target >= playlist.value.length) return
    const reordered = [...playlist.value]
    const [item] = reordered.splice(index, 1)
    reordered.splice(target, 0, item!)
    playlist.value = reordered
  }

  const sessionTotalSeconds = computed(() => {
    const current = status.value
    if (!current) return 0
    if (current.phase !== 'playing') return current.accumulatedPlayedSeconds
    const elapsed = Math.max(0, Math.floor((Date.now() - current.phaseStartedAt) / 1000))
    return current.accumulatedPlayedSeconds + elapsed
  })

  function persistConfig(config: { mode: GameIdleMode; playDuration: GameIdleDuration; restDuration: GameIdleDuration }) {
    try {
      localStorage.setItem(PLAY_MINUTES_KEY, JSON.stringify(config.playDuration))
      localStorage.setItem(REST_MINUTES_KEY, JSON.stringify(config.restDuration))
      localStorage.setItem(MODE_KEY, config.mode)
      configPersistenceError.value = false
    } catch {
      configPersistenceError.value = true
    }
  }

  function validateDurations(): string | null {
    if (!isGameIdleDurationValid(playDuration.value, 1)) {
      return 'play_minutes'
    }
    if (!isGameIdleDurationValid(restDuration.value, 0)) {
      return 'rest_minutes'
    }
    return null
  }

  function validateConfig(): string | null {
    const durationError = validateDurations()
    if (durationError) return durationError
    if (crashEnabled.value && (!isGameIdleDurationValid(crashAfterMinutes.value, 1)
      || !isGameIdleDurationValid(reopenDelaySeconds.value, 0))) {
      return 'crash_simulation'
    }
    if (selectionMode.value === 'manual') {
      if (!playlist.value.length) return 'playlist_empty'
      if (playlistEntries.value.some(entry => entry.unavailable)) return 'playlist_unavailable'
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
      status.value = clearStoppedQueue(activeStatus)
      if (activeStatus && activeStatus.phase !== 'stopped') {
        playDuration.value = activeStatus.playDuration
        restDuration.value = activeStatus.restDuration
        crashEnabled.value = activeStatus.crashSimulation.enabled
        crashAfterMinutes.value = activeStatus.crashSimulation.crashAfterMinutes
        reopenDelaySeconds.value = activeStatus.crashSimulation.reopenDelaySeconds
        mode.value = activeStatus.mode
        selectionMode.value = activeStatus.selectionMode
        candidateType.value = activeStatus.candidateType
        if (activeStatus.selectionMode === 'manual') {
          const saved = new Map(playlist.value.map(item => [item.id, item]))
          playlist.value = [...new Set(activeStatus.selectedAppIds)].map(id => {
            const previous = saved.get(id)
            const item = quests.detectableGames.find(game => game.id === id && game.type_name === previous?.type_name) ??
              quests.detectableGames.find(game => game.id === id) ?? previous ?? { id, name: id }
            return { id: item.id, name: item.name, icon: item.icon, type_name: item.type_name }
          })
        }
      }
      statusUnlisten = await onGameIdleStatus(next => {
        status.value = clearStoppedQueue(next)
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
    const validation = validateDurations()
    if (validation) throw new Error(validation)
    if (loading.value || isActive.value) return
    const selectedMode = mode.value
    const selectedSelectionMode = selectionMode.value
    const selectedCandidateType = candidateType.value
    const selectedPlaylist = playlist.value.map(item => ({ ...item }))
    const selectedAppIds = selectedPlaylist.map(item => item.id)
    const selectedPlayDuration = playDuration.value
    const selectedRestDuration = restDuration.value
    const selectedCrash: GameIdleCrashConfig = {
      enabled: crashEnabled.value,
      crashAfterMinutes: crashAfterMinutes.value,
      reopenDelaySeconds: reopenDelaySeconds.value,
    }
    const cdpPort = quests.cdpPort
    const generation = accountGeneration
    loading.value = true
    stopRequested.value = false
    error.value = null
    const task = (async () => {
      try {
        const [games, simulationPath] = await Promise.all([
          quests.getDetectableGames(),
          quests.initSimulationPath(),
        ])
        if (generation !== accountGeneration) return
        if (selectedSelectionMode === 'manual') {
          if (!selectedAppIds.length) throw new Error('playlist_empty')
          if (selectedAppIds.some(id => !games.some(game => game.id === id && isCompatible(game, selectedMode)))) {
            throw new Error('playlist_unavailable')
          }
        }
        persistSelection({ selectionMode: selectedSelectionMode, candidateType: selectedCandidateType, playlist: selectedPlaylist })
        persistConfig({ mode: selectedMode, playDuration: selectedPlayDuration, restDuration: selectedRestDuration })
        status.value = await startGameIdle(
          {
            mode: selectedMode,
            selectionMode: selectedSelectionMode,
            candidateType: selectedCandidateType,
            selectedAppIds: selectedSelectionMode === 'manual' ? selectedAppIds : [],
            playDuration: selectedPlayDuration,
            restDuration: selectedRestDuration,
            crashSimulation: selectedCrash,
            cdpPort,
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
    })()
    pendingStart = task
    try {
      await task
    } finally {
      if (pendingStart === task) pendingStart = null
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
      // A stopped session will be rebuilt from a fresh shuffle bag on the
      // next start. Do not leave the old reel on screen as if it were still
      // actionable; clear it before rendering the stopped state.
      status.value = clearStoppedQueue(await stopGameIdle())
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
    accountGeneration++
    stopRequested.value = true
    await pendingStart?.catch(() => undefined)
    if (isActive.value) {
      await stop()
    }
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
    playDuration,
    restDuration,
    crashEnabled,
    crashAfterMinutes,
    reopenDelaySeconds,
    mode,
    selectionMode,
    candidateType,
    playlist,
    playlistEntries,
    detectableGames,
    candidateCount,
    isCompatible,
    addToPlaylist,
    removeFromPlaylist,
    movePlaylistItem,
    status,
    history,
    loading,
    stopping,
    stopRequested,
    error,
    persistenceError,
    isActive,
    sessionTotalSeconds,
    initialize,
    refreshHistory,
    start,
    stop,
    removeUpcoming,
    stopForAccountChange,
    validateConfig,
    searchCatalog,
    dispose,
  }
})
