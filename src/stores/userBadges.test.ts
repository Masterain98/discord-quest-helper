import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, disposePinia, setActivePinia } from 'pinia'
import { reactive } from 'vue'
import type { UserBadge } from '@/api/userBadges'
import { useUserBadgesStore } from './userBadges'

const mocks = vi.hoisted(() => ({ getCurrentUserBadges: vi.fn() }))
const auth = reactive<{ user: { id: string } | null }>({ user: null })
vi.mock('./auth', () => ({ useAuthStore: () => auth }))
vi.mock('@/api/userBadges', () => mocks)

function deferred() {
  let resolve!: (value: UserBadge[]) => void
  const promise = new Promise<UserBadge[]>(res => { resolve = res })
  return { promise, resolve }
}
const badges: UserBadge[] = [{ badge_id: 21, progress: [{ current: 382, unit: 'games' }] }]
let pinia: ReturnType<typeof createPinia>

beforeEach(() => {
  auth.user = null
  pinia = createPinia()
  setActivePinia(pinia)
  vi.resetAllMocks()
  vi.useFakeTimers()
  mocks.getCurrentUserBadges.mockResolvedValue(badges)
})
afterEach(() => { disposePinia(pinia); vi.useRealTimers() })

describe('background account badges', () => {
  it('starts without awaiting or opening the menu, and shares in-flight and cached requests', async () => {
    const request = deferred()
    mocks.getCurrentUserBadges.mockReturnValue(request.promise)
    auth.user = { id: 'a' }
    const store = useUserBadgesStore()
    expect(store.loading).toBe(true)
    expect(store.badges).toEqual([])
    const refresh = store.refresh()
    expect(mocks.getCurrentUserBadges).toHaveBeenCalledTimes(1)
    request.resolve(badges)
    await refresh
    expect(store.badges).toEqual(badges)
    expect(store.loading).toBe(false)
    await store.refresh()
    expect(mocks.getCurrentUserBadges).toHaveBeenCalledTimes(1)
  })

  it('discards late responses when switching accounts, including switching back to the same ID', async () => {
    const first = deferred(), second = deferred(), third = deferred()
    mocks.getCurrentUserBadges.mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise).mockReturnValueOnce(third.promise)
    auth.user = { id: 'a' }
    const store = useUserBadgesStore()
    const firstRefresh = store.refresh()
    auth.user = { id: 'b' }
    const secondRefresh = store.refresh()
    auth.user = { id: 'a' }
    first.resolve(badges)
    second.resolve(badges)
    await Promise.all([firstRefresh, secondRefresh])
    expect(store.badges).toEqual([])
    expect(store.loading).toBe(true)
    third.resolve([{ badge_id: 14, info_label: '500 Quests completed' }])
    await store.refresh()
    expect(store.badges[0]?.badge_id).toBe(14)
    expect(store.loading).toBe(false)
  })

  it('clears cached data immediately on logout and ignores its pending response', async () => {
    auth.user = { id: 'a' }
    const store = useUserBadgesStore()
    await store.refresh()
    expect(store.badges).toEqual(badges)
    const request = deferred()
    mocks.getCurrentUserBadges.mockReturnValue(request.promise)
    vi.advanceTimersByTime(60_001)
    const refresh = store.refresh()
    auth.user = null
    expect(store.badges).toEqual([])
    expect(store.loading).toBe(false)
    request.resolve(badges)
    await refresh
    expect(store.badges).toEqual([])
  })

  it('isolates failures, avoids retry storms, and recovers on a later menu opening', async () => {
    mocks.getCurrentUserBadges.mockRejectedValueOnce(new Error('offline'))
    const store = useUserBadgesStore()
    expect(mocks.getCurrentUserBadges).not.toHaveBeenCalled()
    auth.user = { id: 'a' }
    await expect(store.refresh()).resolves.toBeUndefined()
    expect(store.failed).toBe(true)
    expect(store.loading).toBe(false)
    await store.refresh()
    expect(mocks.getCurrentUserBadges).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(60_001)
    await store.refresh()
    expect(store.failed).toBe(false)
    expect(store.badges).toEqual(badges)
  })
})
