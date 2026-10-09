import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({
  onQuestComplete: vi.fn(),
  stopGameSimulationUsage: vi.fn(),
  getPlatformCapabilities: vi.fn(),
  startVideoQuest: vi.fn(),
  stopQuest: vi.fn(),
  forceVideoProgress: vi.fn(),
}))

vi.mock('@/api/tauri', () => ({
  getPlatformCapabilities: mocks.getPlatformCapabilities,
  startVideoQuest: mocks.startVideoQuest,
  stopQuest: mocks.stopQuest,
  forceVideoProgress: mocks.forceVideoProgress,
  onQuestProgress: vi.fn().mockResolvedValue(() => undefined),
  onQuestComplete: mocks.onQuestComplete,
  onQuestError: vi.fn().mockResolvedValue(() => undefined),
  stopGameSimulationUsage: mocks.stopGameSimulationUsage,
  getQuestsFull: vi.fn().mockResolvedValue({ quests: [], excluded_quests: [] }),
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

function quest(id: string, taskType = 'WATCH_VIDEO'): Quest {
  return {
    id,
    config: {
      messages: { quest_name: id, game_title: id },
      task_config: { tasks: { [taskType]: { type: taskType, target: 60 } } },
    },
    user_status: { enrolled_at: '2026-10-10T00:00:00Z', progress: {} },
  } as Quest
}

describe('queued quest completion', () => {
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
    mocks.startVideoQuest.mockResolvedValue(undefined)
    mocks.stopQuest.mockResolvedValue(undefined)
    mocks.forceVideoProgress.mockResolvedValue(undefined)
    mocks.onQuestComplete.mockImplementation(async (callback: () => Promise<void>) => {
      complete = callback
      return () => undefined
    })
    mocks.stopGameSimulationUsage.mockResolvedValue(undefined)
  })

  afterEach(() => {
    useQuestsStore().resetForLogout()
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  async function queuedStore() {
    const store = useQuestsStore()
    store.quests = [quest('first'), quest('second'), quest('third')]
    store.addToQueue(quest('first'))
    store.addToQueue(quest('second'))
    store.isQueueRunning = true
    await store.startVideo('first', 60, 0)
    return store
  }

  it('claims a completed queue item before asynchronous cleanup', async () => {
    let releaseHistory!: () => void
    mocks.stopGameSimulationUsage.mockReturnValueOnce(new Promise<void>(resolve => {
      releaseHistory = resolve
    }))
    const store = await queuedStore()

    const first = complete()
    const duplicate = complete()
    expect(mocks.stopGameSimulationUsage).toHaveBeenCalledTimes(1)
    expect(store.questQueue.map(item => item.id)).toEqual(['first', 'second'])
    releaseHistory()
    await Promise.all([first, duplicate])
    expect(store.questQueue.map(item => item.id)).toEqual(['second'])
  })

  it('pauses the queue when the completed item cannot save its history', async () => {
    mocks.stopGameSimulationUsage.mockRejectedValueOnce(new Error('history disk full'))
    const store = await queuedStore()

    await complete()
    expect(store.questQueue.map(item => item.id)).toEqual(['first', 'second'])
    expect(store.isQueueRunning).toBe(false)
    expect(store.activeQuestId).toBe('first')
    expect(store.error).toContain('history disk full')
  })

  it('protects the current head while removing and reordering only upcoming tasks', async () => {
    const store = await queuedStore()
    store.addToQueue(quest('third'))

    expect(store.removeQueuedQuest('first')).toBe(false)
    expect(store.moveQueuedQuest('first', 1)).toBe(false)
    expect(store.moveQueuedQuest('second', -1)).toBe(false)
    expect(store.moveQueuedQuest('third', -1)).toBe(true)
    expect(store.questQueue.map(item => item.id)).toEqual(['first', 'third', 'second'])
    expect(store.removeQueuedQuest('second')).toBe(true)
    expect(store.activeQuestId).toBe('first')
    expect(mocks.stopQuest).not.toHaveBeenCalled()

    await complete()
    await vi.advanceTimersByTimeAsync(2000)
    expect(mocks.startVideoQuest.mock.calls.map(args => args[0])).toEqual(['first', 'third'])
  })

  it('turns a manually started task into the queue head and excludes Activity tasks', async () => {
    const store = useQuestsStore()
    store.quests = [quest('first'), quest('second')]
    await store.startVideo('first', 60, 0)
    expect(store.addToQueueBehindActive(quest('second'))).toBe(true)
    expect(store.addToQueueBehindActive(quest('second'))).toBe(false)
    expect(store.addToQueueBehindActive(quest('activity', 'PLAY_ACTIVITY'))).toBe(false)
    store.addToQueue(quest('activity', 'ACHIEVEMENT_IN_ACTIVITY'))
    expect(store.questQueue.map(item => item.id)).toEqual(['first', 'second'])
    await complete()
    await vi.advanceTimersByTimeAsync(2000)
    expect(store.activeQuestId).toBe('second')
  })

  it('keeps pending tasks visible and locked until stopping the current task succeeds', async () => {
    let releaseStop!: () => void
    mocks.stopQuest.mockReturnValueOnce(new Promise<void>(resolve => { releaseStop = resolve }))
    const store = await queuedStore()
    const stopping = store.stop('current')
    expect(store.stopping).toBe(true)
    expect(store.upcomingQuests.map(item => item.id)).toEqual(['second'])
    expect(store.removeQueuedQuest('second')).toBe(false)
    expect(store.addToQueueBehindActive(quest('third'))).toBe(false)
    releaseStop()
    expect(await stopping).toBe(true)
    expect(store.activeQuestId).toBeNull()
    expect(store.questQueue.map(item => item.id)).toEqual(['second'])
    expect(store.stopping).toBe(false)
    await vi.advanceTimersByTimeAsync(2000)
    expect(store.activeQuestId).toBe('second')
  })

  it('stops the entire queue without starting another task', async () => {
    const store = await queuedStore()
    expect(await store.stop('all')).toBe(true)
    expect(store.questQueue).toEqual([])
    expect(store.isQueueRunning).toBe(false)
    await vi.advanceTimersByTimeAsync(4000)
    expect(mocks.startVideoQuest).toHaveBeenCalledTimes(1)
  })

  it('keeps the queue paused when native history cleanup fails during stop', async () => {
    const store = await queuedStore()
    mocks.stopGameSimulationUsage.mockRejectedValueOnce(new Error('history disk full'))
    expect(await store.stop('current')).toBe(false)
    expect(store.questQueue.map(item => item.id)).toEqual(['first', 'second'])
    expect(store.activeQuestId).toBe('first')
    expect(store.isQueueRunning).toBe(false)
    await vi.advanceTimersByTimeAsync(4000)
    expect(mocks.startVideoQuest).toHaveBeenCalledTimes(1)
  })

  it('does not advance when stopping the native task fails', async () => {
    const store = await queuedStore()
    mocks.stopQuest.mockRejectedValueOnce(new Error('native cleanup failed'))
    expect(await store.stop('current')).toBe(false)
    expect(store.activeQuestId).toBe('first')
    expect(store.questQueue.map(item => item.id)).toEqual(['first', 'second'])
    expect(store.error).toContain('native cleanup failed')
    await vi.advanceTimersByTimeAsync(4000)
    expect(mocks.startVideoQuest).toHaveBeenCalledTimes(1)
  })

  it('protects a task while its native start request is pending', async () => {
    let releaseStart!: () => void
    mocks.startVideoQuest.mockReturnValueOnce(new Promise<void>(resolve => { releaseStart = resolve }))
    const store = useQuestsStore()
    store.addToQueue(quest('first'))
    store.addToQueue(quest('second'))
    const starting = store.startQueue()
    expect(store.upcomingQuests.map(item => item.id)).toEqual(['second'])
    expect(store.removeQueuedQuest('first')).toBe(false)
    expect(store.moveQueuedQuest('second', -1)).toBe(false)
    releaseStart()
    await starting
    expect(store.activeQuestId).toBe('first')
  })

  it('locks the completed head during asynchronous cleanup', async () => {
    let releaseHistory!: () => void
    mocks.stopGameSimulationUsage.mockReturnValueOnce(new Promise<void>(resolve => { releaseHistory = resolve }))
    const store = await queuedStore()
    const finishing = complete()
    expect(store.activeQuestId).toBeNull()
    expect(store.upcomingQuests.map(item => item.id)).toEqual(['second'])
    expect(store.removeQueuedQuest('first')).toBe(false)
    expect(store.moveQueuedQuest('second', -1)).toBe(false)
    releaseHistory()
    await finishing
    expect(store.questQueue.map(item => item.id)).toEqual(['second'])
  })

  it('clears pending tasks without stopping the active task', async () => {
    const store = await queuedStore()
    store.clearUpcomingQueue()
    expect(store.questQueue.map(item => item.id)).toEqual(['first'])
    expect(store.activeQuestId).toBe('first')
    expect(mocks.stopQuest).not.toHaveBeenCalled()
  })

  it('stops the remaining queue during completion without repeating native cleanup', async () => {
    let releaseHistory!: () => void
    mocks.stopGameSimulationUsage.mockReturnValueOnce(new Promise<void>(resolve => { releaseHistory = resolve }))
    const store = await queuedStore()
    const finishing = complete()
    expect(await store.stop('all')).toBe(true)
    expect(store.questQueue).toEqual([])
    expect(mocks.stopGameSimulationUsage).toHaveBeenCalledTimes(1)
    releaseHistory()
    await finishing
    await vi.advanceTimersByTimeAsync(4000)
    expect(mocks.startVideoQuest).toHaveBeenCalledTimes(1)
  })

  it('cancels advancement when the last waiting task is removed between tasks', async () => {
    const store = await queuedStore()
    await complete()
    expect(store.removeQueuedQuest('second')).toBe(true)
    await vi.advanceTimersByTimeAsync(4000)
    expect(store.isQueueRunning).toBe(false)
    expect(mocks.startVideoQuest).toHaveBeenCalledTimes(1)
  })
})
