import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { formatSimulationDuration, useGameIdleStore } from './gameIdle'
import type { GameIdleStatus } from '@/api/tauri'

const mocks = vi.hoisted(() => ({
  getGameIdleStatus: vi.fn(),
  startGameIdle: vi.fn(),
  stopGameIdle: vi.fn(),
  onGameIdleStatus: vi.fn(),
  onGameSimulationHistoryUpdated: vi.fn(),
  quests: {
    detectableGames: [] as import('@/api/tauri').DetectableGame[],
    platformCapabilities: { os: 'windows' },
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
  startGameIdle: mocks.startGameIdle,
  stopGameIdle: mocks.stopGameIdle,
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
    mocks.quests.detectableGames = []
    mocks.getGameIdleStatus.mockResolvedValue(null)
    mocks.stopGameIdle.mockResolvedValue(null)
    mocks.quests.getDetectableGames.mockResolvedValue([])
    mocks.quests.initSimulationPath.mockResolvedValue('C:/simulations')
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
      selectionMode: 'random', candidateType: 'all', selectedAppIds: [],
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

  it('cancels a pending start before an account change', async () => {
    let releaseGames!: (games: []) => void
    mocks.quests.getDetectableGames.mockReturnValueOnce(new Promise<[]>(resolve => {
      releaseGames = resolve
    }))
    const store = useGameIdleStore()
    const pendingStart = store.start()
    const accountChange = store.stopForAccountChange()
    releaseGames([])

    await Promise.all([pendingStart, accountChange])
    expect(mocks.startGameIdle).not.toHaveBeenCalled()
    expect(store.status).toBeNull()
  })

  it('waits for an admitted start and stops it before changing accounts', async () => {
    let admitSession!: (status: GameIdleStatus) => void
    mocks.startGameIdle.mockReturnValueOnce(new Promise<GameIdleStatus>(resolve => {
      admitSession = resolve
    }))
    const active: GameIdleStatus = {
      sessionId: 'new-session', mode: 'process', phase: 'starting',
      selectionMode: 'random', candidateType: 'all', selectedAppIds: [],
      playMinutes: 60, restMinutes: 0, current: null, recent: [], upcoming: [],
      phaseStartedAt: 0, phaseEndsAt: null, accumulatedPlayedSeconds: 0, warning: null,
    }
    mocks.stopGameIdle.mockResolvedValue({ ...active, phase: 'stopped' })
    const store = useGameIdleStore()
    const pendingStart = store.start()
    await vi.waitFor(() => expect(mocks.startGameIdle).toHaveBeenCalledOnce())

    const accountChange = store.stopForAccountChange()
    admitSession(active)
    await Promise.all([pendingStart, accountChange])

    expect(mocks.stopGameIdle).toHaveBeenCalledOnce()
    expect(store.status).toBeNull()
  })

  function game(id: string, type_name = 'Game', os = 'win32') {
    return { id, name: `Item ${id}`, icon: 'icon', type_name, executables: os ? [{ name: `${id}.exe`, os }] : [] }
  }

  it('defaults to random selection of all types', () => {
    const store = useGameIdleStore()
    expect(store.selectionMode).toBe('random')
    expect(store.candidateType).toBe('all')
    expect(store.playlist).toEqual([])
  })

  it('filters types before deduplication and applies process compatibility', () => {
    mocks.quests.detectableGames = [game('1'), game('1', 'App'), game('2', 'App', ''), game('3', 'Game', 'linux')]
    const store = useGameIdleStore()
    store.mode = 'cdp'
    expect(store.candidateCount).toBe(3)
    store.candidateType = 'app'
    expect(store.candidateCount).toBe(2)
    store.mode = 'process'
    expect(store.candidateCount).toBe(1)
    store.candidateType = 'game'
    expect(store.candidateCount).toBe(1)
  })

  it('adds unique items, reorders and removes them and persists immediately', () => {
    const store = useGameIdleStore()
    store.selectionMode = 'manual'
    store.candidateType = 'app'
    store.addToPlaylist(game('a'))
    store.addToPlaylist(game('b'))
    store.addToPlaylist(game('a'))
    store.movePlaylistItem('b', -1)
    expect(store.playlist.map(item => item.id)).toEqual(['b', 'a'])
    store.movePlaylistItem('b', -1)
    expect(store.playlist.map(item => item.id)).toEqual(['b', 'a'])
    expect(JSON.parse(storage.get('questHelper_gameIdlePlaylist')!)).toEqual([
      { id: 'b', name: 'Item b', icon: 'icon', type_name: 'Game' },
      { id: 'a', name: 'Item a', icon: 'icon', type_name: 'Game' },
    ])
    expect(storage.get('questHelper_gameIdleSelectionMode')).toBe('manual')
    expect(storage.get('questHelper_gameIdleCandidateType')).toBe('app')
    store.removeFromPlaylist('a')
    expect(JSON.parse(storage.get('questHelper_gameIdlePlaylist')!)).toHaveLength(1)
  })

  it('restores saved snapshots and reports missing items instead of dropping them', () => {
    storage.set('questHelper_gameIdleSelectionMode', 'manual')
    storage.set('questHelper_gameIdleCandidateType', 'game')
    storage.set('questHelper_gameIdlePlaylist', JSON.stringify([game('old'), game('present')]))
    mocks.quests.detectableGames = [game('present')]
    const store = useGameIdleStore()
    store.mode = 'cdp'
    expect(store.playlist.map(item => item.id)).toEqual(['old', 'present'])
    expect(store.playlistEntries[0]?.unavailable).toBe('missing')
    expect(store.validateConfig()).toBe('playlist_unavailable')
    store.removeFromPlaylist('old')
    expect(store.validateConfig()).toBeNull()
    store.removeFromPlaylist('present')
    expect(store.validateConfig()).toBe('playlist_empty')
  })

  it('ignores malformed saved data and sanitizes duplicated snapshots', () => {
    storage.set('questHelper_gameIdlePlaylist', '{broken')
    expect(useGameIdleStore().playlist).toEqual([])
    setActivePinia(createPinia())
    storage.set('questHelper_gameIdlePlaylist', JSON.stringify([null, {}, game('a'), game('a'), { id: 2, name: 'bad' }]))
    const store = useGameIdleStore()
    expect(store.playlist.map(item => item.id)).toEqual(['a'])
    expect(store.playlist[0]).not.toHaveProperty('executables')
  })

  it('validates the current metadata and keeps incompatible selections when modes change', () => {
    mocks.quests.detectableGames = [game('a', 'App', '')]
    const store = useGameIdleStore()
    store.selectionMode = 'manual'
    store.candidateType = 'game'
    store.mode = 'cdp'
    store.addToPlaylist(game('a'))
    expect(store.candidateCount).toBe(1)
    expect(store.validateConfig()).toBeNull()
    store.mode = 'process'
    expect(store.candidateCount).toBe(0)
    expect(store.validateConfig()).toBe('playlist_unavailable')
    expect(store.playlistEntries[0]?.unavailable).toBe('incompatible')
    expect(store.playlist).toHaveLength(1)
  })

  it('rejects missing or incompatible selected IDs before invoking the backend', async () => {
    const store = useGameIdleStore()
    store.selectionMode = 'manual'
    store.addToPlaylist(game('a'))
    await expect(store.start()).rejects.toThrow('playlist_unavailable')
    mocks.quests.getDetectableGames.mockResolvedValue([game('a', 'Game', '')])
    await expect(store.start()).rejects.toThrow('playlist_unavailable')
    store.removeFromPlaylist('a')
    await expect(store.start()).rejects.toThrow('playlist_empty')
    expect(mocks.startGameIdle).not.toHaveBeenCalled()
  })

  it('freezes start parameters while async preparation is pending and sends the full catalog', async () => {
    let release!: (games: ReturnType<typeof game>[]) => void
    mocks.quests.getDetectableGames.mockReturnValueOnce(new Promise(resolve => { release = resolve }))
    const store = useGameIdleStore()
    store.mode = 'cdp'
    store.selectionMode = 'manual'
    store.candidateType = 'app'
    store.addToPlaylist(game('b'))
    store.addToPlaylist(game('a'))
    const pending = store.start()
    store.mode = 'process'
    store.selectionMode = 'random'
    store.playMinutes = 90
    store.addToPlaylist(game('c')) // Editing methods are locked during startup.
    const catalog = [game('a'), game('b'), game('c')]
    release(catalog)
    await pending
    expect(mocks.startGameIdle).toHaveBeenCalledWith(expect.objectContaining({
      mode: 'cdp', selectionMode: 'manual', candidateType: 'app', selectedAppIds: ['b', 'a'],
      playMinutes: 60, restMinutes: 0,
    }), catalog)
    expect(store.playlist).toHaveLength(2)
  })

  it('restores an active playlist configuration and protects saved entries from queue skips', async () => {
    const status: GameIdleStatus = {
      sessionId: 'active', mode: 'cdp', selectionMode: 'manual', candidateType: 'app', selectedAppIds: ['b', 'a'],
      phase: 'playing', playMinutes: 1, restMinutes: 1, current: null, recent: [], upcoming: [],
      phaseStartedAt: 0, phaseEndsAt: null, accumulatedPlayedSeconds: 0, warning: null,
    }
    mocks.getGameIdleStatus.mockResolvedValue(status)
    mocks.quests.detectableGames = [game('a'), game('b')]
    const store = useGameIdleStore()
    await store.initialize()
    expect(store.selectionMode).toBe('manual')
    expect(store.candidateType).toBe('app')
    expect(store.playlist.map(item => item.id)).toEqual(['b', 'a'])
    store.removeFromPlaylist('a')
    store.movePlaylistItem('a', -1)
    expect(store.playlist.map(item => item.id)).toEqual(['b', 'a'])
    await store.removeUpcoming({ id: 'a', name: 'Item a', occurrenceId: 'future-a' })
    expect(store.playlist.map(item => item.id)).toEqual(['b', 'a'])
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
