import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from '@/stores/quests'
import { useToastStore } from '@/stores/toast'
import { useQuestGroupActions } from './useQuestGroupActions'

const mocks = vi.hoisted(() => ({
  accept: vi.fn(), claim: vi.fn(), navigate: vi.fn(), fetch: vi.fn(), prepare: vi.fn(),
}))
vi.mock('@/api/tauri', () => ({
  acceptQuest: mocks.accept, claimQuestReward: mocks.claim, navigateDiscordSpa: mocks.navigate,
  getQuestsFull: mocks.fetch, getPlatformCapabilities: vi.fn(async () => ({ os: 'windows', defaultGameQuestMode: 'simulate', executableOsPriority: ['win32'] })),
}))

function quest(id: string, status: 'accept' | 'run' | 'claim' = 'run'): Quest {
  return {
    id,
    config: { messages: { quest_name: 'Clean Crew', game_title: 'Roblox' }, task_config_v2: { tasks: { PLAY_ON_DESKTOP: { target: 900 } } } },
    user_status: status === 'accept' ? null : { enrolled_at: '2026-01-01', completed_at: status === 'claim' ? '2026-01-02' : null },
  }
}
const scope = () => effectScope()
let activeScope: ReturnType<typeof scope>

function setup(members: Quest[], blocked = () => false) {
  const store = useQuestsStore()
  store.quests = members
  mocks.fetch.mockImplementation(async () => ({ quests: store.quests, excluded_quests: [] }))
  const actions = activeScope.run(() => useQuestGroupActions({ blocked, prepareExecutables: mocks.prepare, t: (key, params) => `${key}:${JSON.stringify(params)}` }))!
  return { store, actions }
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.useFakeTimers()
  const values = new Map<string, string>()
  vi.stubGlobal('localStorage', { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value) })
  vi.stubGlobal('requestAnimationFrame', vi.fn())
  vi.stubGlobal('cancelAnimationFrame', vi.fn())
  setActivePinia(createPinia())
  activeScope = scope()
  mocks.accept.mockResolvedValue(undefined)
  mocks.claim.mockResolvedValue(undefined)
  mocks.navigate.mockResolvedValue(undefined)
  mocks.prepare.mockResolvedValue(new Map())
})
afterEach(() => {
  activeScope.stop()
  useToastStore().clear()
  useQuestsStore().resetForLogout()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('group actions', () => {
  it('accepts only supplied members, retains partial success and releases locks', async () => {
    const members = [quest('a', 'accept'), quest('b', 'accept'), quest('hidden', 'accept')]
    const { store, actions } = setup(members)
    mocks.accept.mockRejectedValueOnce(new Error('no')).mockResolvedValueOnce(undefined)
    const operation = actions.accept(members.slice(0, 2))
    expect(actions.busyIds.value).toEqual(new Set(['a', 'b']))
    await vi.advanceTimersByTimeAsync(1000)
    await operation
    expect(mocks.accept.mock.calls.map(args => args[0])).toEqual(['a', 'b'])
    expect(store.quests[0].user_status).toBeNull()
    expect(store.quests[1].user_status?.enrolled_at).toBeTruthy()
    expect(store.quests[2].user_status).toBeNull()
    expect(actions.busy.value).toBe(false)
    expect(useToastStore().toasts[0].title).toContain('"success":1,"fail":1')
  })

  it('does not duplicate a group operation or operate through a blocked entry point', async () => {
    const members = [quest('a', 'accept')]
    const { actions } = setup(members)
    const operation = actions.accept(members)
    await actions.accept(members)
    await vi.advanceTimersByTimeAsync(1000)
    await operation
    expect(mocks.accept).toHaveBeenCalledTimes(1)
    const blocked = setup([quest('b', 'accept')], () => true)
    await blocked.actions.accept(blocked.store.quests)
    expect(mocks.accept).toHaveBeenCalledTimes(1)
  })

  it('does not enqueue any members when executable preselection is cancelled', async () => {
    const members = [quest('a'), quest('b')]
    const { store, actions } = setup(members)
    mocks.prepare.mockRejectedValueOnce(new Error('User cancelled'))
    await actions.run(members)
    expect(store.questQueue).toEqual([])
    expect(actions.busy.value).toBe(false)
    expect(useToastStore().toasts).toEqual([])
  })

  it('rechecks eligibility after preselection and keeps existing queue entries', async () => {
    const members = [quest('a'), quest('b'), quest('hidden')]
    const { store, actions } = setup(members)
    store.addToQueue(members[0])
    const start = vi.spyOn(store, 'startQueue').mockResolvedValue(undefined)
    mocks.prepare.mockImplementationOnce(async () => {
      store.quests[1].user_status!.completed_at = '2026-01-02'
      return new Map()
    })
    await actions.run(members.slice(0, 2))
    expect(mocks.prepare.mock.calls[0][0].map((q: Quest) => q.id)).toEqual(['b'])
    expect(store.questQueue.map(q => q.id)).toEqual(['a'])
    expect(start).toHaveBeenCalledOnce()
  })

  it('claims through the API without changing failed or hidden members', async () => {
    const members = [quest('a', 'claim'), quest('b', 'claim'), quest('hidden', 'claim')]
    const { store, actions } = setup(members)
    mocks.claim.mockImplementation(async id => {
      if (id === 'b') throw new Error('failed')
      store.quests.find(q => q.id === id)!.user_status!.claimed_at = '2026-01-03'
    })
    await actions.claim(members.slice(0, 2))
    expect(mocks.claim.mock.calls.map(args => args[0])).toEqual(['a', 'b'])
    expect(store.quests[1].user_status?.claimed_at).toBeFalsy()
    expect(store.quests[2].user_status?.claimed_at).toBeFalsy()
    expect(actions.busy.value).toBe(false)
    expect(useToastStore().toasts[0].title).toContain('"success":1,"fail":1')
  })

  it('opens one Discord member at a time and advances only after confirmed claim', async () => {
    const members = [quest('a', 'claim'), quest('b', 'claim')]
    const { store, actions } = setup(members)
    store.cdpAvailable = true
    await actions.claim(members)
    expect(mocks.navigate.mock.calls.map(args => args[0])).toEqual(['/quest-home#a'])
    expect(actions.busy.value).toBe(true)
    await actions.checkAndContinue()
    expect(actions.guide.value?.index).toBe(0)
    expect(actions.guideMessage.value).toContain('guide_unclaimed')
    expect(mocks.navigate).toHaveBeenCalledTimes(1)
    store.quests[0].user_status!.claimed_at = '2026-01-03'
    await actions.checkAndContinue()
    expect(mocks.navigate.mock.calls.map(args => args[0])).toEqual(['/quest-home#a', '/quest-home#b'])
    store.quests[1].user_status!.claimed_at = '2026-01-03'
    await actions.checkAndContinue()
    expect(actions.guide.value).toBeNull()
    expect(actions.busy.value).toBe(false)
    expect(mocks.claim).not.toHaveBeenCalled()
  })

  it('keeps the guide on its current member when refresh fails and supports cancel', async () => {
    const { store, actions } = setup([quest('a', 'claim'), quest('b', 'claim')])
    store.cdpAvailable = true
    await actions.claim(store.quests)
    mocks.fetch.mockRejectedValueOnce(new Error('offline'))
    await actions.checkAndContinue()
    expect(actions.guide.value?.index).toBe(0)
    expect(actions.guideMessage.value).toBe('offline')
    actions.cancel()
    expect(actions.guide.value).toBeNull()
    expect(actions.busy.value).toBe(false)
    expect(store.quests.every(q => !q.user_status?.claimed_at)).toBe(true)
  })

  it('does not advance a cancelled guide after an in-flight refresh returns', async () => {
    const { store, actions } = setup([quest('a', 'claim'), quest('b', 'claim')])
    store.cdpAvailable = true
    await actions.claim(store.quests)
    let resolve!: (value: unknown) => void
    mocks.fetch.mockReturnValueOnce(new Promise(r => { resolve = r }))
    const checking = actions.checkAndContinue()
    actions.cancel()
    const claimed = quest('a', 'claim'); claimed.user_status!.claimed_at = '2026-01-03'
    resolve({ quests: [claimed, quest('b', 'claim')], excluded_quests: [] })
    await checking
    expect(mocks.navigate).toHaveBeenCalledTimes(1)
    expect(actions.guide.value).toBeNull()
  })

  it('offers navigation retry after a disconnect without claiming automatically', async () => {
    const { store, actions } = setup([quest('a', 'claim'), quest('b', 'claim')])
    store.cdpAvailable = true
    await actions.claim(store.quests)
    store.cdpAvailable = false
    await actions.reopenGuideMember()
    expect(actions.guideMessage.value).toContain('guide_disconnected')
    expect(mocks.claim).not.toHaveBeenCalled()
    store.cdpAvailable = true
    await actions.reopenGuideMember()
    expect(mocks.navigate).toHaveBeenCalledTimes(2)
  })
})
