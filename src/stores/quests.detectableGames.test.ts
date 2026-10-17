import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useQuestsStore } from './quests'
import type { DetectableGame } from '@/api/tauri'

const mocks = vi.hoisted(() => ({
  fetchDetectableGames: vi.fn(),
  getPlatformCapabilities: vi.fn().mockResolvedValue({ os: 'windows', defaultGameQuestMode: 'simulate' }),
}))

vi.mock('@/api/tauri', () => mocks)
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn() }))

describe('detectable catalog caching', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() })
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  const complete: DetectableGame[] = [
    { id: 'game', name: 'Game', type_name: 'Game', executables: [] },
    { id: 'app', name: 'App', type_name: 'App', executables: [] },
  ]

  it('shares a successful request between concurrent callers and caches the complete catalog', async () => {
    let resolve!: (games: DetectableGame[]) => void
    mocks.fetchDetectableGames.mockReturnValueOnce(new Promise<DetectableGame[]>(done => { resolve = done }))
    const store = useQuestsStore()
    const first = store.getDetectableGames()
    const second = store.getDetectableGames()
    expect(mocks.fetchDetectableGames).toHaveBeenCalledOnce()
    resolve(complete)
    await expect(first).resolves.toEqual(complete)
    await expect(second).resolves.toEqual(complete)
    await expect(store.getDetectableGames()).resolves.toEqual(complete)
    expect(mocks.fetchDetectableGames).toHaveBeenCalledOnce()
  })

  it('rejects all concurrent callers after a failed refresh, retains the old catalog, and allows retry', async () => {
    let reject!: (error: Error) => void
    mocks.fetchDetectableGames.mockReturnValueOnce(new Promise<DetectableGame[]>((_, fail) => { reject = fail }))
    const store = useQuestsStore()
    store.detectableGames = complete
    const first = store.getDetectableGames(true)
    const second = store.getDetectableGames()
    const outcomes = Promise.allSettled([first, second])
    const error = new Error('Failed to fetch detectable applications')
    reject(error)
    expect(await outcomes).toEqual([
      { status: 'rejected', reason: error },
      { status: 'rejected', reason: error },
    ])
    expect(store.detectableGames).toEqual(complete)
    mocks.fetchDetectableGames.mockResolvedValueOnce(complete)
    await expect(store.getDetectableGames(true)).resolves.toEqual(complete)
    expect(mocks.fetchDetectableGames).toHaveBeenCalledTimes(2)
  })
})
