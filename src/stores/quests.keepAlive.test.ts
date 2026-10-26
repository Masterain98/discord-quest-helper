import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({
  getPlatformCapabilities: vi.fn(),
  getQuestsFull: vi.fn(),
  startVideoQuest: vi.fn(),
  stopQuest: vi.fn(),
  stopSimulatedGame: vi.fn(),
  stopGameSimulationUsage: vi.fn(),
  onQuestComplete: vi.fn(),
}))

vi.mock('@/api/tauri', () => ({
  getPlatformCapabilities: mocks.getPlatformCapabilities,
  getQuestsFull: mocks.getQuestsFull,
  startVideoQuest: mocks.startVideoQuest,
  stopQuest: mocks.stopQuest,
  stopSimulatedGame: mocks.stopSimulatedGame,
  stopGameSimulationUsage: mocks.stopGameSimulationUsage,
  onQuestProgress: vi.fn().mockResolvedValue(() => undefined),
  onQuestComplete: mocks.onQuestComplete,
  onQuestError: vi.fn().mockResolvedValue(() => undefined),
}))

vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn() }))

function storage(): Storage {
  const values = new Map<string, string>()
  return {
    get length() { return values.size },
    clear: () => values.clear(),
    getItem: key => values.get(key) ?? null,
    key: index => [...values.keys()][index] ?? null,
    removeItem: key => values.delete(key),
    setItem: (key, value) => values.set(key, String(value)),
  }
}

function quest(id: string): Quest {
  return {
    id,
    config: {
      messages: { quest_name: id, game_title: id },
      task_config: { tasks: { PLAY: { type: 'PLAY', target: 60 } } },
    },
    user_status: { enrolled_at: '2026-10-10T00:00:00Z', progress: {} },
  } as Quest
}

describe('post-quest keep-alive', () => {
  let complete!: () => Promise<void>

  beforeEach(() => {
    vi.stubGlobal('localStorage', storage())
    vi.stubGlobal('requestAnimationFrame', vi.fn())
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
    setActivePinia(createPinia())
    vi.useFakeTimers()
    vi.clearAllMocks()
    mocks.getPlatformCapabilities.mockResolvedValue({
      os: 'windows', defaultGameQuestMode: 'simulate', executableOsPriority: ['win32'],
    })
    mocks.getQuestsFull.mockImplementation(async () => ({ quests: [quest('first'), quest('second')], excluded_quests: [] }))
    mocks.startVideoQuest.mockResolvedValue(undefined)
    mocks.stopQuest.mockResolvedValue(undefined)
    mocks.stopSimulatedGame.mockResolvedValue(undefined)
    mocks.stopGameSimulationUsage.mockResolvedValue(undefined)
    mocks.onQuestComplete.mockImplementation(async (callback: () => Promise<void>) => {
      complete = callback
      return () => undefined
    })
  })

  afterEach(() => {
    useQuestsStore().resetForLogout()
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  // startVideo is the lightest path that arms the completion listeners; the
  // test then swaps in the state a finished simulate-mode game quest would have.
  async function runningStore() {
    const store = useQuestsStore()
    store.quests = [quest('first'), quest('second')]
    store.addToQueue(quest('first'))
    store.addToQueue(quest('second'))
    store.isQueueRunning = true
    await store.startVideo('first', 60, 0)
    store.activeQuestType = 'game'
    store.activeGameExe = 'first.exe'
    return store
  }

  it('delays the teardown while the game stays connected, then closes it and advances the queue', async () => {
    const store = await runningStore()
    store.postQuestKeepAliveEnabled = true
    store.postQuestKeepAlive = { mode: 'fixed', fixed: 1, min: 1, max: 1 }

    await complete()

    expect(store.keepAliveEndsAt).toBe(Date.now() + 60_000)
    expect(mocks.stopSimulatedGame).not.toHaveBeenCalled()
    expect(store.questQueue.map(item => item.id)).toEqual(['first', 'second'])

    await vi.advanceTimersByTimeAsync(60_000)

    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
    expect(mocks.stopSimulatedGame).toHaveBeenCalledWith('first.exe')
    expect(mocks.stopGameSimulationUsage).toHaveBeenCalledTimes(1)
    expect(store.questQueue.map(item => item.id)).toEqual(['second'])
    expect(store.activeQuestId).toBeNull()
    expect(store.keepAliveEndsAt).toBeNull()
  })

  it('stops immediately when requested during keep-alive and never repeats the teardown', async () => {
    const store = await runningStore()
    store.postQuestKeepAliveEnabled = true
    store.postQuestKeepAlive = { mode: 'fixed', fixed: 1, min: 1, max: 1 }
    await complete()
    expect(store.keepAliveEndsAt).not.toBeNull()

    await store.stop('all')

    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
    expect(mocks.stopGameSimulationUsage).toHaveBeenCalledTimes(1)
    expect(store.keepAliveEndsAt).toBeNull()
    expect(store.questQueue).toEqual([])

    await vi.advanceTimersByTimeAsync(120_000)

    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
    expect(mocks.stopGameSimulationUsage).toHaveBeenCalledTimes(1)
  })

  it('tears down immediately when keep-alive is disabled', async () => {
    const store = await runningStore()

    await complete()

    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
    expect(store.keepAliveEndsAt).toBeNull()
    expect(store.questQueue.map(item => item.id)).toEqual(['second'])
  })

  it('never schedules keep-alive without a simulated game process', async () => {
    const store = await runningStore()
    store.postQuestKeepAliveEnabled = true
    store.activeGameExe = null

    await complete()

    expect(store.keepAliveEndsAt).toBeNull()
    expect(mocks.stopSimulatedGame).not.toHaveBeenCalled()
    expect(store.questQueue.map(item => item.id)).toEqual(['second'])
  })

  it('does not repeat the native teardown when Stop lands during completion cleanup', async () => {
    const store = await runningStore()
    store.postQuestKeepAliveEnabled = true
    store.postQuestKeepAlive = { mode: 'fixed', fixed: 1, min: 1, max: 1 }
    await complete()
    mocks.stopSimulatedGame.mockReturnValueOnce(new Promise<void>(() => undefined))

    await vi.advanceTimersByTimeAsync(60_000)
    expect(store.queueEditingDisabled).toBe(true)
    expect(store.activeQuestId).toBeNull()

    await store.stop('all')

    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
    // The in-flight teardown still owns the history finalization; Stop must
    // not start a second one while it waits.
    expect(mocks.stopGameSimulationUsage).toHaveBeenCalledTimes(0)
    expect(store.questQueue).toEqual([])
  })

  it('closes the kept-alive game when the account changes during keep-alive', async () => {
    const store = await runningStore()
    store.postQuestKeepAliveEnabled = true
    store.postQuestKeepAlive = { mode: 'fixed', fixed: 1, min: 1, max: 1 }
    await complete()
    expect(store.keepAliveEndsAt).not.toBeNull()

    store.setQuestAccount('account-2')
    await vi.advanceTimersByTimeAsync(0)

    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
    expect(store.keepAliveEndsAt).toBeNull()
    await vi.advanceTimersByTimeAsync(120_000)
    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
  })

  it('schedules waits longer than the webview timer limit in chunks', async () => {
    // Re-fake without rAF: the fake rAF machinery keeps a ~16ms internal
    // timer alive, which a multi-billion-ms tick would fire 100M+ times. The
    // store uses no animation frames on this path (the global is stubbed).
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date'] })
    vi.setSystemTime(0)
    const store = await runningStore()
    store.postQuestKeepAliveEnabled = true
    store.postQuestKeepAlive = { mode: 'fixed', fixed: 43_200, min: 43_200, max: 43_200 }
    await complete()
    expect(store.keepAliveEndsAt).toBe(2_592_000_000)

    await vi.advanceTimersByTimeAsync(2_147_000_000)
    expect(mocks.stopSimulatedGame).not.toHaveBeenCalled()
    expect(store.keepAliveEndsAt).not.toBeNull()

    await vi.advanceTimersByTimeAsync(600_000_000)
    expect(mocks.stopSimulatedGame).toHaveBeenCalledTimes(1)
    expect(store.keepAliveEndsAt).toBeNull()
  })
})
