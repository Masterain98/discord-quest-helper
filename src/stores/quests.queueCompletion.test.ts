import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({
  onQuestComplete: vi.fn(),
  stopGameSimulationUsage: vi.fn(),
  getPlatformCapabilities: vi.fn(),
  startVideoQuest: vi.fn(),
}))

vi.mock('@/api/tauri', () => ({
  getPlatformCapabilities: mocks.getPlatformCapabilities,
  startVideoQuest: mocks.startVideoQuest,
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
    mocks.onQuestComplete.mockImplementation(async (callback: () => Promise<void>) => {
      complete = callback
      return () => undefined
    })
    mocks.stopGameSimulationUsage.mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  async function queuedStore() {
    const store = useQuestsStore()
    store.addToQueue({ id: 'first' } as Quest)
    store.addToQueue({ id: 'second' } as Quest)
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
})
