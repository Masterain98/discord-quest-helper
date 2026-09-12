import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({
  appLocalDataDir: vi.fn(),
  join: vi.fn(),
  getPlatformCapabilities: vi.fn(),
}))

vi.mock('@tauri-apps/api/path', () => ({
  appLocalDataDir: mocks.appLocalDataDir,
  join: mocks.join,
}))

vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn(),
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

describe('quests simulation path', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', createMemoryStorage())
    setActivePinia(createPinia())
    vi.clearAllMocks()
    mocks.appLocalDataDir.mockResolvedValue('C:\\Users\\Test\\AppData\\Local\\QuestHelper')
    mocks.join.mockImplementation(async (base: string, child: string) => `${base}\\${child}`)
    mocks.getPlatformCapabilities.mockResolvedValue({
      os: 'windows',
      defaultGameQuestMode: 'simulate',
      executableOsPriority: ['win32'],
    })
  })

  it('initializes and persists the app-local GameRuntime directory', async () => {
    const store = useQuestsStore()

    await expect(store.initSimulationPath()).resolves.toBe(
      'C:\\Users\\Test\\AppData\\Local\\QuestHelper\\GameRuntime',
    )
    expect(store.simulationPath).toBe('C:\\Users\\Test\\AppData\\Local\\QuestHelper\\GameRuntime')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe(store.simulationPath)
  })

  it('preserves and trims a saved directory without resolving a default', async () => {
    localStorage.setItem('questHelper_simulationPath', '  D:\\DiscordGames  ')
    const store = useQuestsStore()

    await expect(store.initSimulationPath()).resolves.toBe('D:\\DiscordGames')
    expect(store.simulationPath).toBe('D:\\DiscordGames')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe('D:\\DiscordGames')
    expect(mocks.appLocalDataDir).not.toHaveBeenCalled()
  })

  it('persists a custom directory immediately and rejects empty values', () => {
    const store = useQuestsStore()

    store.setSimulationPath('  D:\\DiscordGames  ')
    expect(store.simulationPath).toBe('D:\\DiscordGames')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe('D:\\DiscordGames')
    expect(() => store.setSimulationPath('   ')).toThrow('Simulation path cannot be empty')
  })

  it('restores and persists the app-local default', async () => {
    localStorage.setItem('questHelper_simulationPath', 'D:\\DiscordGames')
    const store = useQuestsStore()

    await expect(store.resetSimulationPath()).resolves.toBe(
      'C:\\Users\\Test\\AppData\\Local\\QuestHelper\\GameRuntime',
    )
    expect(localStorage.getItem('questHelper_simulationPath')).toBe(store.simulationPath)
  })

  it('does not overwrite a custom path selected while default resolution is in flight', async () => {
    let resolveBase!: (value: string) => void
    mocks.appLocalDataDir.mockImplementationOnce(() => new Promise<string>((resolve) => {
      resolveBase = resolve
    }))
    const store = useQuestsStore()

    const initialization = store.initSimulationPath()
    store.setSimulationPath('D:\\DiscordGames')
    resolveBase('C:\\Users\\Test\\AppData\\Local\\QuestHelper')

    await expect(initialization).resolves.toBe('D:\\DiscordGames')
    expect(store.simulationPath).toBe('D:\\DiscordGames')
  })
})
