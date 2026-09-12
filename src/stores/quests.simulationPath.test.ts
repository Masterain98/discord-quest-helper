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

  it('preserves a saved directory exactly without resolving a default', async () => {
    localStorage.setItem('questHelper_simulationPath', '/tmp/Discord Games ')
    const store = useQuestsStore()

    await expect(store.initSimulationPath()).resolves.toBe('/tmp/Discord Games ')
    expect(store.simulationPath).toBe('/tmp/Discord Games ')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe('/tmp/Discord Games ')
    expect(mocks.appLocalDataDir).not.toHaveBeenCalled()
  })

  it('persists a custom directory exactly and rejects whitespace-only values', () => {
    const store = useQuestsStore()

    store.setSimulationPath('/tmp/Discord Games ')
    expect(store.simulationPath).toBe('/tmp/Discord Games ')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe('/tmp/Discord Games ')
    expect(() => store.setSimulationPath('   ')).toThrow('Simulation path cannot be empty')
  })

  it('replaces a saved whitespace-only value with the default directory', async () => {
    localStorage.setItem('questHelper_simulationPath', '   ')
    const store = useQuestsStore()

    await expect(store.initSimulationPath()).resolves.toBe(
      'C:\\Users\\Test\\AppData\\Local\\QuestHelper\\GameRuntime',
    )
    expect(store.simulationPath).toBe('C:\\Users\\Test\\AppData\\Local\\QuestHelper\\GameRuntime')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe(store.simulationPath)
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

  it('does not overwrite a custom path selected while reset is in flight', async () => {
    let resolveBase!: (value: string) => void
    mocks.appLocalDataDir.mockImplementationOnce(() => new Promise<string>((resolve) => {
      resolveBase = resolve
    }))
    const store = useQuestsStore()

    const reset = store.resetSimulationPath()
    store.setSimulationPath('/tmp/New Selection ')
    resolveBase('C:\\Users\\Test\\AppData\\Local\\QuestHelper')

    await expect(reset).resolves.toBe('/tmp/New Selection ')
    expect(store.simulationPath).toBe('/tmp/New Selection ')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe('/tmp/New Selection ')
  })

  it('treats re-selecting the same path as newer intent than an in-flight reset', async () => {
    localStorage.setItem('questHelper_simulationPath', '/tmp/Current Selection')
    let resolveBase!: (value: string) => void
    mocks.appLocalDataDir.mockImplementationOnce(() => new Promise<string>((resolve) => {
      resolveBase = resolve
    }))
    const store = useQuestsStore()

    const reset = store.resetSimulationPath()
    store.setSimulationPath('/tmp/Current Selection')
    resolveBase('C:\\Users\\Test\\AppData\\Local\\QuestHelper')

    await expect(reset).resolves.toBe('/tmp/Current Selection')
    expect(store.simulationPath).toBe('/tmp/Current Selection')
    expect(localStorage.getItem('questHelper_simulationPath')).toBe('/tmp/Current Selection')
  })
})
