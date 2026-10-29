import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// `@/i18n` (imported by the store) reads localStorage at module scope, so the
// storage stub must exist before any module import runs.
const storageStub = vi.hoisted(() => {
  const map = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value)
    },
    removeItem: (key: string) => {
      map.delete(key)
    },
  })
  return map
})

const mocks = vi.hoisted(() => ({
  getTranslationStatus: vi.fn(),
  startMessageTranslation: vi.fn(),
  stopMessageTranslation: vi.fn(),
  onTranslationStatus: vi.fn().mockReturnValue(() => {}),
  onTranslationError: vi.fn().mockReturnValue(() => {}),
}))

vi.mock('@/api/tauri', () => ({
  getTranslationStatus: mocks.getTranslationStatus,
  startMessageTranslation: mocks.startMessageTranslation,
  stopMessageTranslation: mocks.stopMessageTranslation,
  onTranslationStatus: mocks.onTranslationStatus,
  onTranslationError: mocks.onTranslationError,
}))

import { parsePersistedSettings, useTranslationStore } from './translation'

function statusPayload() {
  return { enabled: true, translated: 3, cacheHits: 1, errors: 0, lastError: null }
}

describe('translation settings parsing', () => {
  it('falls back to defaults on missing or corrupted data', () => {
    expect(parsePersistedSettings(null)).toMatchObject({ provider: 'google-translate', targetLanguage: 'en' })
    expect(parsePersistedSettings('not json{')).toMatchObject({ provider: 'google-translate' })
    expect(parsePersistedSettings('42')).toMatchObject({ displayMode: 'below' })
  })

  it('keeps valid values and clamps the summary count', () => {
    const parsed = parsePersistedSettings(JSON.stringify({
      provider: 'deepl',
      targetLanguage: 'pt-BR',
      autoTranslate: true,
      displayMode: 'replace',
      accentColor: '#ff8800',
      summaryCount: 500,
      microsoftRegion: 'global',
      llm: { baseUrl: 'https://api.example.com/v1', model: 'm' },
    }))
    expect(parsed.provider).toBe('deepl')
    expect(parsed.autoTranslate).toBe(true)
    expect(parsed.displayMode).toBe('replace')
    expect(parsed.summaryCount).toBe(100)
    expect(parsed.llm).toEqual({ baseUrl: 'https://api.example.com/v1', model: 'm' })
  })

  it('rejects invalid provider, display mode and llm shapes', () => {
    const parsed = parsePersistedSettings(JSON.stringify({
      provider: 'babelfish',
      displayMode: 'telepathy',
      llm: { baseUrl: 42 },
    }))
    expect(parsed.provider).toBe('google-translate')
    expect(parsed.displayMode).toBe('below')
    expect(parsed.llm).toBeNull()
  })
})

describe('translation store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    storageStub.clear()
    vi.stubGlobal('navigator', { language: 'pt-BR' })
    vi.clearAllMocks()
  })

  it('starts with the resolved auto target language and persists settings', async () => {
    const store = useTranslationStore()
    store.settings.targetLanguage = 'auto'
    const config = store.buildConfig()
    expect(config.targetLanguage).toBe('pt')
    expect(config.labels.translate).toBeTruthy()

    store.persist()
    expect(storageStub.get('questHelper_translationSettings')).toContain('"targetLanguage":"auto"')
  })

  it('start forwards the built config and surfaces the status', async () => {
    mocks.startMessageTranslation.mockResolvedValue(statusPayload())
    const store = useTranslationStore()
    const enabled = await store.start()
    expect(enabled).toBe(true)
    expect(mocks.startMessageTranslation).toHaveBeenCalledWith(expect.objectContaining({
      provider: 'google-translate',
      autoTranslate: false,
      summaryCount: 50,
    }))
    expect(store.status?.translated).toBe(3)
  })

  it('start keeps the store usable when the backend rejects the config', async () => {
    mocks.startMessageTranslation.mockRejectedValue('LLM base URL must use https://')
    const store = useTranslationStore()
    const enabled = await store.start()
    expect(enabled).toBe(false)
    expect(store.error).toContain('https://')
    expect(store.busy).toBe(false)
  })

  it('stop clears busy state even when the backend fails', async () => {
    mocks.stopMessageTranslation.mockRejectedValue(new Error('boom'))
    const store = useTranslationStore()
    await store.stop()
    expect(store.error).toBe('boom')
    expect(store.busy).toBe(false)
  })

  it('refreshStatus degrades to null status on failure', async () => {
    mocks.getTranslationStatus.mockRejectedValue(new Error('offline'))
    const store = useTranslationStore()
    await store.refreshStatus()
    expect(store.status).toBeNull()
  })
})
