import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import type { Quest } from '@/api/tauri'
import {
  getQuests,
  startVideoQuest,
  startStreamQuest,
  stopQuest,
  onQuestProgress,
  onQuestComplete,
  onQuestError,
  createSimulatedGame,
  runSimulatedGame,
  stopSimulatedGame,
  fetchDetectableGames,
  connectToDiscordRpc,
  acceptQuest,
  startGameHeartbeatQuest,
  forceVideoProgress
} from '@/api/tauri'
import { homeDir, sep } from '@tauri-apps/api/path'
import { emit } from '@tauri-apps/api/event'


// localStorage keys
const STORAGE_SPEED_KEY = 'questHelper_speedMultiplier'

export const useQuestsStore = defineStore('quests', () => {
  const quests = ref<Quest[]>([])
  const loading = ref(false)
  const stopping = ref(false)
  const error = ref<string | null>(null)

  const activeQuestId = ref<string | null>(null)
  const activeQuestType = ref<'video' | 'stream' | 'game' | null>(null)
  const activeQuestProgress = ref(0)
  const activeQuestTargetDuration = ref(0)

  // Local Progress Simulation State
  const localProgress = ref(0)
  const activeGameExe = ref<string | null>(null)

  // Speed multiplier - read from localStorage, default 7
  const savedSpeed = localStorage.getItem(STORAGE_SPEED_KEY)
  const speedMultiplier = ref(savedSpeed ? parseInt(savedSpeed) : 7)

  // Heartbeat interval (seconds) - for Video quests API heartbeat requests
  const STORAGE_INTERVAL_KEY = 'questHelper_heartbeatInterval'
  const savedInterval = localStorage.getItem(STORAGE_INTERVAL_KEY)
  const heartbeatInterval = ref(savedInterval ? parseInt(savedInterval) : 3)

  // Game polling interval (seconds) - for Play/Game quests progress detection
  const STORAGE_GAME_POLLING_KEY = 'questHelper_gamePollingInterval'
  const savedGamePolling = localStorage.getItem(STORAGE_GAME_POLLING_KEY)
  const gamePollingInterval = ref(savedGamePolling ? parseInt(savedGamePolling) : 30)

  // Game Quest Mode - 'simulate' runs a fake game exe, 'heartbeat' sends direct API heartbeats
  const STORAGE_GAME_QUEST_MODE_KEY = 'questHelper_gameQuestMode'
  const savedGameQuestMode = localStorage.getItem(STORAGE_GAME_QUEST_MODE_KEY)
  const gameQuestMode = ref<'simulate' | 'heartbeat'>(savedGameQuestMode === 'heartbeat' ? 'heartbeat' : 'simulate')

  // Persist speed changes to localStorage
  watch(speedMultiplier, (newSpeed) => {
    localStorage.setItem(STORAGE_SPEED_KEY, String(newSpeed))
  })

  // Persist heartbeat interval changes
  watch(heartbeatInterval, (newInterval) => {
    localStorage.setItem(STORAGE_INTERVAL_KEY, String(newInterval))
  })

  // Persist game polling interval changes
  watch(gamePollingInterval, (newInterval) => {
    localStorage.setItem(STORAGE_GAME_POLLING_KEY, String(newInterval))
  })

  // Persist game quest mode changes
  watch(gameQuestMode, (newMode) => {
    localStorage.setItem(STORAGE_GAME_QUEST_MODE_KEY, newMode)
  })

  let progressUnlisten: (() => void) | null = null
  let completeUnlisten: (() => void) | null = null
  let errorUnlisten: (() => void) | null = null
  let pollingTimer: ReturnType<typeof setInterval> | null = null

  // Simulation internal vars
  let simAnimationFrame: number | null = null
  let simLastTime = 0
  let simCurrentSpeed = 1.0

  async function fetchQuests(silent = false) {
    if (!silent) loading.value = true
    error.value = null
    try {
      quests.value = await getQuests()
    } catch (e) {
      error.value = e as string
    } finally {
      if (!silent) loading.value = false
    }
  }

  function checkActiveQuestStatus() {
    if (!activeQuestId.value) return
    const quest = quests.value.find(q => q.id === activeQuestId.value)
    if (!quest) return

    // Check completion
    if (quest.user_status?.completed_at) {
      console.log('Quest completed detected via polling, stopping game.')
      stop()
      return
    }

    // Update progress
    const progressObj = quest.user_status?.progress
    let currentSeconds = 0
    if (progressObj && typeof progressObj === 'object') {
      const vals = Object.values(progressObj as Record<string, { value?: number }>)
      if (vals.length > 0 && vals[0]?.value) currentSeconds = vals[0].value
    }

    const target = activeQuestTargetDuration.value
    if (target > 0) {
      const pct = (currentSeconds / target) * 100
      activeQuestProgress.value = pct
    }
  }

  function startPolling() {
    if (pollingTimer) clearInterval(pollingTimer)
    // Use user-configurable game polling interval (in seconds, convert to ms)
    const intervalMs = gamePollingInterval.value * 1000
    pollingTimer = setInterval(async () => {
      await fetchQuests(true)
      checkActiveQuestStatus()
    }, intervalMs)
  }

  function stopPolling() {
    if (pollingTimer) {
      clearInterval(pollingTimer)
      pollingTimer = null
    }
  }

  // --- Local Progress Simulation ---
  function startProgressSimulation(speed: number) {
    stopProgressSimulation() // Clear any existing
    simCurrentSpeed = speed
    simLastTime = Date.now()
    localProgress.value = activeQuestProgress.value

    // Loop
    const loop = () => {
      if (!activeQuestId.value || activeQuestProgress.value >= 100) {
        stopProgressSimulation()
        return
      }

      const now = Date.now()
      const deltaSeconds = (now - simLastTime) / 1000
      simLastTime = now

      const targetSeconds = activeQuestTargetDuration.value
      if (targetSeconds > 0) {
        const addedPercent = (deltaSeconds * simCurrentSpeed / targetSeconds) * 100
        localProgress.value += addedPercent
      }

      // Clamp logic:
      // Always at least activeQuestProgress (blue bar)
      // Never more than 100
      localProgress.value = Math.max(localProgress.value, activeQuestProgress.value)
      localProgress.value = Math.min(localProgress.value, 100)

      simAnimationFrame = requestAnimationFrame(loop)
    }

    simAnimationFrame = requestAnimationFrame(loop)
  }

  function stopProgressSimulation() {
    if (simAnimationFrame !== null) {
      cancelAnimationFrame(simAnimationFrame)
      simAnimationFrame = null
    }
  }

  // Watch activeQuestProgress to re-anchor local progress
  // If backend reports new progress (blue bar jumps), update local (green bar) to ensure it's not lagging behind
  watch(activeQuestProgress, (newVal) => {
    localProgress.value = Math.max(localProgress.value, newVal)
  })

  // Update a quest's enrollment status locally (no full refresh)
  function updateQuestEnrollment(questId: string, enrolledAt: string) {
    const questIndex = quests.value.findIndex(q => q.id === questId)
    if (questIndex !== -1) {
      const quest = quests.value[questIndex]
      // Create new user_status or update existing one
      quests.value[questIndex] = {
        ...quest,
        user_status: {
          ...quest.user_status,
          enrolled_at: enrolledAt,
          completed_at: quest.user_status?.completed_at || null,
          claimed_at: quest.user_status?.claimed_at || null,
          progress: quest.user_status?.progress || {}
        }
      }
    }
  }

  async function startVideo(questId: string, secondsNeeded: number, initialProgress: number) {
    try {
      const progressPct = (secondsNeeded > 0) ? (initialProgress / secondsNeeded) * 100 : 0
      await startVideoQuest(questId, secondsNeeded, progressPct, speedMultiplier.value, heartbeatInterval.value)
      activeQuestId.value = questId
      activeQuestType.value = 'video'
      activeQuestProgress.value = progressPct
      activeQuestTargetDuration.value = secondsNeeded

      startProgressSimulation(speedMultiplier.value)
      setupListeners()
    } catch (e) {
      error.value = e as string
      throw e
    }
  }

  async function startStream(questId: string, streamKey: string, secondsNeeded: number, initialProgress: number) {
    try {
      const progressPct = (secondsNeeded > 0) ? (initialProgress / secondsNeeded) * 100 : 0
      await startStreamQuest(questId, streamKey, secondsNeeded, progressPct)
      activeQuestId.value = questId
      activeQuestType.value = 'stream'
      activeQuestProgress.value = progressPct
      activeQuestTargetDuration.value = secondsNeeded

      startProgressSimulation(1.0)
      setupListeners()
    } catch (e) {
      error.value = e as string
      throw e
    }
  }

  async function startPlay(quest: Quest, secondsNeeded: number, initialProgress: number) {
    loading.value = true
    error.value = null
    try {
      // 1. Get Application ID
      const appId = quest.config.application?.id
      if (!appId) throw new Error('Quest missing application ID')

      // Check mode: 'heartbeat' uses direct API calls, 'simulate' runs fake game
      if (gameQuestMode.value === 'heartbeat') {
        // Direct heartbeat mode - no game simulation needed
        console.log(`Starting game quest via direct heartbeat for AppID: ${appId}`)

        const progressPct = (secondsNeeded > 0) ? (initialProgress / secondsNeeded) * 100 : 0
        await startGameHeartbeatQuest(
          quest.id,
          appId,
          secondsNeeded,
          progressPct
        )

        activeQuestId.value = quest.id
        activeQuestType.value = 'game'
        activeQuestProgress.value = progressPct
        activeQuestTargetDuration.value = secondsNeeded

        startProgressSimulation(1.0)

        // Setup listeners for progress/complete/error events
        setupListeners()

      } else {
        // Simulate mode - original behavior
        // 2. Fetch detectable games to find executable name
        const detectableGames = await fetchDetectableGames()
        const game = detectableGames.find(g => g.id === appId)
        if (!game) throw new Error(`Game not found in Discord's detectable list (AppID: ${appId})`)

        const winExe = game.executables.find(e => e.os === 'win32')
        if (!winExe) throw new Error(`No Windows executable definition for game ${game.name}`)

        console.log(`Starting simulated game for ${game.name} (${winExe.name})...`)

        // 3. Setup path
        const home = await homeDir()
        const separator = await sep()
        const installPath = `${home}${separator}Documents${separator}DiscordQuestGames`

        // 4. Create simulated game executable
        await createSimulatedGame(installPath, winExe.name, appId)
        activeGameExe.value = winExe.name

        // 5. Run simulated game
        await runSimulatedGame(game.name, installPath, winExe.name, appId)

        // 6. Connect RPC
        const activity = {
          app_id: appId,
          state: "In Game",
          details: `Playing ${game.name}`,
          largeImageKey: "logo",
          largeImageText: game.name,
          timestamp: Date.now()
        }

        await connectToDiscordRpc(JSON.stringify(activity), 'connect')

        // 7. Update state
        activeQuestId.value = quest.id
        activeQuestType.value = 'game'
        activeQuestProgress.value = (secondsNeeded > 0) ? (initialProgress / secondsNeeded) * 100 : 0
        activeQuestTargetDuration.value = secondsNeeded

        startProgressSimulation(1.0)

        // Start polling for Play quests (no backend events)
        setupListeners()
        startPolling()
      }
    } catch (e) {
      error.value = e as string
      // Clean up if started (only for simulate mode)
      if (activeGameExe.value) {
        try {
          await stopSimulatedGame(activeGameExe.value)
        } catch { }
        activeGameExe.value = null
      }
      throw e
    } finally {
      loading.value = false
    }
  }

  async function stop() {
    stopping.value = true
    console.trace('questsStore.stop() called')

    stopProgressSimulation()

    try {
      // Force Save Logic for Video Quests
      if (activeQuestId.value && activeQuestType.value === 'video' && activeQuestTargetDuration.value > 0) {
        try {
          const currentSeconds = (localProgress.value / 100) * activeQuestTargetDuration.value
          // Only force if we have significant progress
          if (currentSeconds > 0) {
            console.log(`Force submitting video progress: ${currentSeconds.toFixed(1)}s (ID: ${activeQuestId.value})`)
            await forceVideoProgress(activeQuestId.value, currentSeconds)
          }
        } catch (e) {
          console.error('Failed to force submit progress on stop:', e)
        }
      }

      // If manually stopping, ensure queue is also stopped/cleared
      if (isQueueRunning.value) {
        isQueueRunning.value = false
        questQueue.value = [] // Clear queue on manual stop
      }

      let exeToStop = activeGameExe.value

      // Recovery: If activeGameExe is missing but we have a quest, try to find it
      if (!exeToStop && activeQuestId.value && activeQuestType.value !== 'video') { // Don't look for exe if video
        console.warn('activeGameExe is null, attempting to recover from activeQuestId...')
        const quest = quests.value.find(q => q.id === activeQuestId.value)
        if (quest && quest.config.application?.id) {
          try {
            const appId = quest.config.application.id
            const detectableGames = await fetchDetectableGames()
            const game = detectableGames.find(g => g.id === appId)
            if (game) {
              const winExe = game.executables.find(e => e.os === 'win32')
              if (winExe) {
                exeToStop = winExe.name
                console.log('Recovered executable name:', exeToStop)
              }
            }
          } catch (err) {
            console.error('Failed to recover executable name:', err)
          }
        }
      }

      // Stop simulated game if running
      if (exeToStop) {
        try {
          console.log(`Stopping simulated game: ${exeToStop}`)
          await stopSimulatedGame(exeToStop)
          // Disconnect RPC
          await emit('event_disconnect')
        } catch (e) {
          console.error('Failed to stop game process:', e)
        }
        activeGameExe.value = null
      }

      try {
        await stopQuest()
      } catch (e) {
        // Ignore error if no quest running
      }

      activeQuestId.value = null
      activeQuestType.value = null
      activeQuestProgress.value = 0
      activeQuestTargetDuration.value = 0
      localProgress.value = 0

      cleanupListeners()

      // Refresh quests to get latest status
      await fetchQuests(true)

    } finally {
      stopping.value = false
    }
  }

  function setupListeners() {
    cleanupListeners()

    console.log('Setting up quest progress listeners...')

    onQuestProgress((progress) => {
      console.log('Received quest-progress event:', progress)
      activeQuestProgress.value = progress
      // For Play quests, update local state or log since no direct feedback loop? 
      // Discord RPC is one-way, but we might listen to Discord Gateway for activity updates if needed.
      // But user_status updates come from backend polling or events.
    }).then((unlisten) => {
      progressUnlisten = unlisten
      console.log('Quest progress listener ready')
    })

    onQuestComplete(() => {
      console.log('Received quest-complete event')

      // If queue is running, handle transition
      if (isQueueRunning.value && questQueue.value.length > 0) {
        // The active quest just finished. It should be the head of the queue.
        // (Unless user manually stopped?)
        // Let's assume head is active.
        const finished = questQueue.value.shift()
        console.log(`Queue item finished: ${finished?.id}. Remaining: ${questQueue.value.length}`)

        // Reset state
        activeQuestId.value = null
        activeQuestType.value = null
        activeQuestProgress.value = 0
        activeGameExe.value = null
        localProgress.value = 0
        stopProgressSimulation()

        // Refresh quests to update status in UI
        fetchQuests(true)

        // Trigger next item
        setTimeout(() => {
          processQueue()
        }, 2000)

        // We do NOT cleanup listeners fully if we want to reuse them?
        // Actually processQueue calls startVideo which calls setupListeners.
        // So cleaning up here is fine/correct.
        cleanupListeners()
      } else {
        // Normal single quest completion
        activeQuestId.value = null
        activeQuestType.value = null
        activeQuestProgress.value = 0
        activeQuestTargetDuration.value = 0
        activeGameExe.value = null
        localProgress.value = 0
        stopProgressSimulation()

        fetchQuests()
        cleanupListeners()
      }
    }).then((unlisten) => {
      completeUnlisten = unlisten
      console.log('Quest complete listener ready')
    })

    onQuestError((err) => {
      console.log('Received quest-error event:', err)
      error.value = err
      activeQuestId.value = null
      activeQuestType.value = null
      activeQuestProgress.value = 0
      activeQuestTargetDuration.value = 0
      activeGameExe.value = null
      localProgress.value = 0
      stopProgressSimulation()

      cleanupListeners()
    }).then((unlisten) => {
      errorUnlisten = unlisten
      console.log('Quest error listener ready')
    })
  }

  function cleanupListeners() {
    stopPolling()
    if (progressUnlisten) {
      progressUnlisten()
      progressUnlisten = null
    }
    if (completeUnlisten) {
      completeUnlisten()
      completeUnlisten = null
    }
    if (errorUnlisten) {
      errorUnlisten()
      errorUnlisten = null
    }
  }

  function setSpeedMultiplier(speed: number) {
    speedMultiplier.value = speed
  }

  async function acceptQuestWrapper(questId: string) {
    try {
      await acceptQuest(questId)
      // Optimistic update
      updateQuestEnrollment(questId, new Date().toISOString())
    } catch (e) {
      error.value = e as string
      throw e
    }
  }

  async function acceptAllQuests(questIds: string[]) {
    loading.value = true
    error.value = null
    let successCount = 0
    let failCount = 0
    try {
      for (const id of questIds) {
        try {
          await acceptQuest(id)
          updateQuestEnrollment(id, new Date().toISOString())
          successCount++
          // Small delay to be nice to API
          await new Promise(r => setTimeout(r, 500))
        } catch (e) {
          console.error(`Failed to accept quest ${id}:`, e)
          failCount++
        }
      }
    } finally {
      loading.value = false
      if (failCount > 0) {
        error.value = `Accepted ${successCount} quests, failed ${failCount}`
      }
    }
  }


  // Refined Complete All Video:
  // We can't blocking-wait in the UI thread for 15 mins x N quests.
  // But we can start a "Queue Mode".
  const questQueue = ref<Quest[]>([])
  const isQueueRunning = ref(false)

  async function processQueue() {
    if (questQueue.value.length === 0) {
      isQueueRunning.value = false
      return
    }

    isQueueRunning.value = true
    const quest = questQueue.value[0]

    try {
      console.log(`Queue processing: ${quest.id}`)
      // Start video
      // Calculate duration needed
      let seconds = 0
      const taskConfig = quest.config.task_config || quest.config.task_config_v2
      if (taskConfig?.tasks) {
        const tasks = Object.values(taskConfig.tasks)
        if (tasks.length > 0) seconds = tasks[0].target || 0
      }

      // Check if already partial
      let progress = 0
      if (quest.user_status?.progress) {
        const vals = Object.values(quest.user_status.progress)
        if (vals.length > 0) progress = vals[0].value || 0
      }

      // If completed, skip
      if (quest.user_status?.completed_at) {
        questQueue.value.shift()
        processQueue()
        return
      }

      await startVideo(quest.id, seconds, progress)

      // Now we wait for completion event. 
      // setupListeners handles `onQuestComplete`.
      // We need to hook into that.
      // Modifying setupListeners to check queue?

    } catch (e) {
      console.error("Queue error:", e)
      questQueue.value.shift() // Skip failed
      processQueue()
    }
  }

  // We need to modify `onQuestComplete` to trigger next in queue.
  // See `setupListeners`.

  return {
    quests,
    loading,
    error,
    activeQuestId,
    activeQuestType,
    activeQuestProgress,
    activeQuestTargetDuration,
    localProgress, // Export local progress
    speedMultiplier,
    heartbeatInterval,
    gamePollingInterval,
    gameQuestMode,
    stopping,
    activeGameExe,
    questQueue, // Export queue
    isQueueRunning,
    fetchQuests,
    updateQuestEnrollment,
    startVideo,
    startStream,
    startPlay,
    stop,
    setSpeedMultiplier,
    acceptQuest: acceptQuestWrapper,
    acceptAllQuests,
    // Add to queue logic needs integration with listeners
    addToQueue: (q: Quest) => {
      if (!questQueue.value.find(x => x.id === q.id)) {
        questQueue.value.push(q)
      }
    },
    startQueue: processQueue,
    clearQueue: () => {
      questQueue.value = []
      isQueueRunning.value = false
      stop()
    }
  }
})
