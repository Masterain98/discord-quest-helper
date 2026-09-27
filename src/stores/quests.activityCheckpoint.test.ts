import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({
  getPlatformCapabilities: vi.fn(),
}))

vi.mock('@/api/tauri', () => ({
  getPlatformCapabilities: mocks.getPlatformCapabilities,
}))

function createMemoryStorage(): Storage {
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

describe('quests Activity checkpoint settings', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', createMemoryStorage())
    setActivePinia(createPinia())
    vi.clearAllMocks()
    mocks.getPlatformCapabilities.mockResolvedValue({
      os: 'windows',
      defaultGameQuestMode: 'simulate',
      executableOsPriority: ['win32'],
    })
  })

  it('restores saved checkpoint values instead of the built-in defaults', () => {
    localStorage.setItem('questHelper_activityCheckpointMin', '240')
    localStorage.setItem('questHelper_activityCheckpointMax', '480')

    const store = useQuestsStore()

    expect(store.activityCheckpointMin).toBe(240)
    expect(store.activityCheckpointMax).toBe(480)
  })

  it('persists checkpoint changes synchronously', () => {
    const store = useQuestsStore()

    store.activityCheckpointMin = 240
    store.activityCheckpointMax = 480

    expect(localStorage.getItem('questHelper_activityCheckpointMin')).toBe('240')
    expect(localStorage.getItem('questHelper_activityCheckpointMax')).toBe('480')
  })

  it('persists linked values when enforcing min/max ordering', () => {
    const store = useQuestsStore()

    store.activityCheckpointMin = 420

    expect(store.activityCheckpointMax).toBe(420)
    expect(localStorage.getItem('questHelper_activityCheckpointMin')).toBe('420')
    expect(localStorage.getItem('questHelper_activityCheckpointMax')).toBe('420')
  })
})
