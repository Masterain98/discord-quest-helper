import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({ checkCdpStatus: vi.fn(), getPlatformCapabilities: vi.fn() }))
vi.mock('@/api/tauri', () => mocks)
beforeEach(() => {
  const storage = new Map<string, string>([['questHelper_gameQuestMode', 'cdp']])
  vi.stubGlobal('localStorage', { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) })
  setActivePinia(createPinia())
  vi.resetAllMocks()
  mocks.getPlatformCapabilities.mockResolvedValue({ os: 'windows', defaultGameQuestMode: 'simulate', executableOsPriority: ['win32'] })
})
afterEach(() => { vi.unstubAllGlobals() })
describe('CDP settings status checks', () => {
  it.each(['disconnected', 'failed'])('preserves CDP mode after a %s check when returning to settings', async condition => {
    const store = useQuestsStore()
    store.gameQuestMode = 'cdp'
    store.cdpAvailable = true
    if (condition === 'failed') mocks.checkCdpStatus.mockRejectedValue(new Error('offline'))
    else mocks.checkCdpStatus.mockResolvedValue({ connected: false })
    await store.initCdpMode({ preserveMode: true })
    expect(store.cdpAvailable).toBe(false)
    expect(store.gameQuestMode).toBe('cdp')
    expect(localStorage.getItem('questHelper_gameQuestMode')).toBe('cdp')
  })
  it('keeps the existing fallback for task startup callers', async () => {
    const store = useQuestsStore()
    store.gameQuestMode = 'cdp'
    mocks.checkCdpStatus.mockResolvedValue({ connected: false })
    await store.initCdpMode()
    expect(store.gameQuestMode).toBe('simulate')
  })
})
