import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({
  getQuestsFull: vi.fn(),
  startCdpQuest: vi.fn(),
  startVideoQuest: vi.fn(),
  startStreamQuest: vi.fn(),
  startPlayActivityQuest: vi.fn(),
}))

vi.mock('@/api/tauri', () => ({
  ...mocks,
  getPlatformCapabilities: vi.fn().mockResolvedValue({
    os: 'windows', defaultGameQuestMode: 'simulate', executableOsPriority: ['win32'],
  }),
  onQuestProgress: vi.fn().mockResolvedValue(() => undefined),
  onQuestComplete: vi.fn().mockResolvedValue(() => undefined),
  onQuestError: vi.fn().mockResolvedValue(() => undefined),
  startGameSimulationUsage: vi.fn().mockResolvedValue(undefined),
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

function quest(id: string, type = 'PLAY_ON_DESKTOP'): Quest {
  return {
    id,
    config: {
      application: { id: 'app', name: 'Game', link: '' },
      messages: { quest_name: id },
      task_config: { tasks: { [type]: { type, target: 3 } } },
    },
    user_status: { enrolled_at: '2026-10-01', completed_at: null, claimed_at: null, progress: {} },
  }
}

function response(quests: Quest[]) {
  return { quests, excluded_quests: [] }
}

beforeEach(() => {
  const values = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  })
  vi.stubGlobal('requestAnimationFrame', vi.fn())
  vi.stubGlobal('cancelAnimationFrame', vi.fn())
  vi.clearAllMocks()
  setActivePinia(createPinia())
})

afterEach(() => vi.unstubAllGlobals())

describe('quest list loading and refresh', () => {
  it('drops cached completion and ignores late responses when accounts change', async () => {
    const store = useQuestsStore()
    store.setQuestAccount('old')
    const oldRequest = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(oldRequest.promise)
    const oldFetch = store.fetchQuests(true, true)
    store.quests = [{ ...quest('old-completed'), user_status: { completed_at: 'done' } }]
    store.setQuestAccount('new')
    expect(store.quests).toEqual([])
    expect(store.hasLoadedQuests).toBe(false)
    const newRequest = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(newRequest.promise)
    const newFetch = store.fetchQuests()
    oldRequest.resolve(response([quest('old-completed')]))
    await oldFetch
    expect(store.quests).toEqual([])
    expect(store.loading).toBe(true)
    newRequest.resolve(response([quest('new-quest')]))
    await newFetch
    expect(store.quests.map(q => q.id)).toEqual(['new-quest'])
    expect(store.questAccountId).toBe('new')
  })

  it('invalidates requests on logout and preserves state when the same account is selected', async () => {
    const store = useQuestsStore()
    store.setQuestAccount('account')
    store.quests = [quest('existing')]
    store.setQuestAccount('account')
    expect(store.quests).toHaveLength(1)
    const pending = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(pending.promise)
    const fetch = store.fetchQuests(true, true)
    store.setQuestAccount(null)
    pending.resolve(response([quest('late')]))
    await fetch
    expect(store.quests).toEqual([])
    expect(store.questAccountId).toBeNull()
    expect(store.hasLoadedQuests).toBe(false)
  })

  it('uses list loading only until the first fetch completes', async () => {
    const pending = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(pending.promise)
    const store = useQuestsStore()
    const fetch = store.fetchQuests()
    expect(store.loading).toBe(true)
    expect(store.refreshing).toBe(false)
    pending.resolve(response([]))
    await fetch
    expect(store.hasLoadedQuests).toBe(true)

    const refresh = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(refresh.promise)
    const next = store.fetchQuests(false, true)
    expect(store.loading).toBe(false)
    expect(store.refreshing).toBe(true)
    refresh.resolve(response([]))
    await next
    expect(store.refreshing).toBe(false)
  })

  it('retains cards during refresh and replaces only changed quest data', async () => {
    const store = useQuestsStore()
    store.quests = [quest('first'), quest('second')]
    const first = store.quests[0]
    const second = store.quests[1]
    const pending = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(pending.promise)
    const fetch = store.fetchQuests(false, true)
    expect(store.loading).toBe(false)
    expect(store.refreshing).toBe(true)
    expect(store.quests[0]).toBe(first)
    const changed = quest('second')
    changed.user_status!.progress = { PLAY_ON_DESKTOP: { value: 2 } }
    pending.resolve(response([quest('first'), changed, quest('third')]))
    await fetch
    expect(store.quests[0]).toBe(first)
    expect(store.quests[1]).not.toBe(second)
    expect(store.quests[1].user_status?.progress?.PLAY_ON_DESKTOP?.value).toBe(2)
    expect(store.quests.map(item => item.id)).toEqual(['first', 'second', 'third'])
  })

  it('shares an in-flight poll with manual refresh and keeps the indicator until it settles', async () => {
    const pending = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(pending.promise)
    const store = useQuestsStore()
    store.quests = [quest('first')]
    const poll = store.fetchQuests(true, true)
    const refresh = store.fetchQuests(false, true)
    expect(mocks.getQuestsFull).toHaveBeenCalledTimes(1)
    expect(store.refreshing).toBe(true)
    pending.resolve(response([quest('first')]))
    await Promise.all([poll, refresh])
    expect(store.refreshing).toBe(false)
  })

  it('keeps existing cards after a failed refresh', async () => {
    mocks.getQuestsFull.mockRejectedValueOnce(new Error('Network unavailable'))
    const store = useQuestsStore()
    store.quests = [quest('first')]
    const first = store.quests[0]
    await store.fetchQuests(false, true)
    expect(store.quests[0]).toBe(first)
    expect(store.loading).toBe(false)
    expect(store.refreshing).toBe(false)
    expect(store.error).toBe('Network unavailable')
  })

  it('ignores an old response after logout without finishing the next account fetch', async () => {
    const old = deferred<ReturnType<typeof response>>()
    const current = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise)
    const store = useQuestsStore()
    const previous = store.fetchQuests()
    store.resetForLogout()
    const next = store.fetchQuests()
    old.resolve(response([quest('old-account')]))
    await previous
    expect(store.quests).toEqual([])
    expect(store.loading).toBe(true)
    expect(store.hasLoadedQuests).toBe(false)
    current.resolve(response([quest('new-account')]))
    await next
    expect(store.quests[0].id).toBe('new-account')
    expect(store.loading).toBe(false)
  })
})

describe('per-quest startup', () => {
  const cases = [
    { name: 'game', mock: mocks.startCdpQuest, start: (store: ReturnType<typeof useQuestsStore>) => store.startPlay(quest('first'), 60, 0) },
    { name: 'checkpoint activity', mock: mocks.startCdpQuest, start: (store: ReturnType<typeof useQuestsStore>) => store.startActivity(quest('first', 'ACHIEVEMENT_IN_ACTIVITY')) },
    { name: 'cloud activity', mock: mocks.startPlayActivityQuest, start: (store: ReturnType<typeof useQuestsStore>) => store.startPlayActivity(quest('first', 'PLAY_ACTIVITY'), 60, 0) },
    { name: 'video', mock: mocks.startVideoQuest, start: (store: ReturnType<typeof useQuestsStore>) => store.startVideo('first', 60, 0) },
    { name: 'stream', mock: mocks.startStreamQuest, start: (store: ReturnType<typeof useQuestsStore>) => store.startStream('first', 'stream', 60, 0) },
  ]

  it.each(cases)('keeps the list visible during $name startup and clears only startup on success', async ({ name, mock, start }) => {
    const pending = deferred<void>()
    mock.mockReturnValueOnce(pending.promise)
    const store = useQuestsStore()
    store.gameQuestMode = name === 'game' || name === 'checkpoint activity' ? 'cdp' : 'simulate'
    store.cdpAvailable = true
    store.quests = [quest('first'), quest('second')]
    const first = store.quests[0]
    const startup = start(store)
    expect(store.startingQuestId).toBe('first')
    expect(store.loading).toBe(false)
    expect(store.refreshing).toBe(false)
    expect(store.quests[0]).toBe(first)
    pending.resolve()
    await startup
    expect(store.startingQuestId).toBeNull()
    expect(store.activeQuestId).toBe('first')
    store.resetForLogout()
  })

  it('does not clear list loading when startup fails during the initial fetch', async () => {
    const pending = deferred<ReturnType<typeof response>>()
    mocks.getQuestsFull.mockReturnValueOnce(pending.promise)
    mocks.startCdpQuest.mockRejectedValueOnce(new Error('CDP unavailable'))
    const store = useQuestsStore()
    store.gameQuestMode = 'cdp'
    const fetch = store.fetchQuests()
    await expect(store.startPlay(quest('first'), 60, 0)).rejects.toThrow('CDP unavailable')
    expect(store.startingQuestId).toBeNull()
    expect(store.loading).toBe(true)
    pending.resolve(response([quest('first')]))
    await fetch
    expect(store.loading).toBe(false)
  })
})
