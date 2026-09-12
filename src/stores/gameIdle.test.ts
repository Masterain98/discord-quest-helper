import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { formatSimulationDuration, useGameIdleStore } from './gameIdle'
import type { GameIdleStatus } from '@/api/tauri'

const mocks = vi.hoisted(() => ({
  getGameIdleStatus: vi.fn(),
  stopGameIdle: vi.fn(),
  onGameIdleStatus: vi.fn(),
  onGameSimulationHistoryUpdated: vi.fn(),
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
  getGameIdleStatus: mocks.getGameIdleStatus,
  getGameSimulationHistory: vi.fn().mockResolvedValue([]),
  onGameIdleStatus: mocks.onGameIdleStatus,
  onGameSimulationHistoryUpdated: mocks.onGameSimulationHistoryUpdated,
  removeGameIdleQueueItem: vi.fn(),
  startGameIdle: vi.fn(),
  stopGameIdle: mocks.stopGameIdle,
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
    mocks.getGameIdleStatus.mockResolvedValue(null)
    mocks.stopGameIdle.mockResolvedValue(null)
    mocks.onGameIdleStatus.mockResolvedValue(() => undefined)
    mocks.onGameSimulationHistoryUpdated.mockResolvedValue(() => undefined)
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

  it('can retry initialization after an API failure', async () => {
    mocks.getGameIdleStatus
      .mockRejectedValueOnce(new Error('temporary status failure'))
      .mockResolvedValueOnce(null)
    const store = useGameIdleStore()

    await expect(store.initialize()).rejects.toThrow('temporary status failure')
    await expect(store.initialize()).resolves.toBeUndefined()

    expect(mocks.getGameIdleStatus).toHaveBeenCalledTimes(2)
    expect(mocks.onGameIdleStatus).toHaveBeenCalledOnce()
    expect(mocks.onGameSimulationHistoryUpdated).toHaveBeenCalledOnce()
  })

  it('clears the stopped queue so the next run starts with a fresh preview', async () => {
    const stopped: GameIdleStatus = {
      sessionId: 'session',
      mode: 'cdp',
      phase: 'stopped',
      playMinutes: 60,
      restMinutes: 0,
      current: { id: 'current', name: 'Current', occurrenceId: 'current-1' },
      recent: [{ id: 'recent', name: 'Recent', occurrenceId: 'recent-1' }],
      upcoming: [{ id: 'next', name: 'Next', occurrenceId: 'next-1' }],
      phaseStartedAt: 0,
      phaseEndsAt: null,
      accumulatedPlayedSeconds: 12,
      warning: null,
    }
    mocks.stopGameIdle.mockResolvedValue(stopped)
    const store = useGameIdleStore()
    store.status = stopped

    await store.stop()

    expect(store.status?.phase).toBe('stopped')
    expect(store.status?.current).toBeNull()
    expect(store.status?.recent).toEqual([])
    expect(store.status?.upcoming).toEqual([])
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
