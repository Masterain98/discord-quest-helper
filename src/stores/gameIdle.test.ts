import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { formatSimulationDuration, useGameIdleStore } from './gameIdle'

const mocks = vi.hoisted(() => ({
  quests: {
    cdpAvailable: true,
    cdpPort: 9223,
    initPlatformCapabilities: vi.fn().mockResolvedValue(undefined),
    initCdpMode: vi.fn().mockResolvedValue(undefined),
    getDetectableGames: vi.fn().mockResolvedValue([]),
    initSimulationPath: vi.fn().mockResolvedValue('C:/simulations'),
  },
}))

vi.mock('@/api/tauri', () => ({
  getGameIdleStatus: vi.fn().mockResolvedValue(null),
  getGameSimulationHistory: vi.fn().mockResolvedValue([]),
  onGameIdleStatus: vi.fn().mockResolvedValue(() => undefined),
  onGameSimulationHistoryUpdated: vi.fn().mockResolvedValue(() => undefined),
  removeGameIdleQueueItem: vi.fn(),
  startGameIdle: vi.fn(),
  stopGameIdle: vi.fn().mockResolvedValue(null),
  stopGameSimulationUsage: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('./quests', () => ({
  useQuestsStore: () => mocks.quests,
}))

describe('game idle store', () => {
  let storage: Map<string, string>

  beforeEach(() => {
    setActivePinia(createPinia())
    storage = new Map()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    })
    vi.clearAllMocks()
    mocks.quests.cdpAvailable = true
  })

  it('defaults to 60/0 and chooses CDP when it is available', async () => {
    const store = useGameIdleStore()
    await store.initialize()

    expect(store.playMinutes).toBe(60)
    expect(store.restMinutes).toBe(0)
    expect(store.mode).toBe('cdp')
  })

  it('restores valid values and rejects fractional input', () => {
    storage.set('questHelper_gameIdlePlayMinutes', '90')
    storage.set('questHelper_gameIdleRestMinutes', '5')
    storage.set('questHelper_gameIdleMode', 'process')
    const store = useGameIdleStore()

    expect(store.playMinutes).toBe(90)
    expect(store.restMinutes).toBe(5)
    expect(store.mode).toBe('process')
    store.playMinutes = 1.5
    expect(store.validateConfig()).toBe('play_minutes')
    store.playMinutes = 1
    store.restMinutes = -1
    expect(store.validateConfig()).toBe('rest_minutes')
  })
})

describe('game idle duration formatting', () => {
  it('floors partial minutes and carries complete hours', () => {
    expect(formatSimulationDuration(59)).toEqual({ hours: 0, minutes: 0 })
    expect(formatSimulationDuration(3_719)).toEqual({ hours: 1, minutes: 1 })
  })

  it('never exposes negative history values', () => {
    expect(formatSimulationDuration(-120)).toEqual({ hours: 0, minutes: 0 })
  })
})
