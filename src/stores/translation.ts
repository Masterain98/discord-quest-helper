import { ref } from 'vue'
import { defineStore } from 'pinia'
import i18n from '@/i18n'
import type {
  LlmConfig,
  TranslationConfig,
  TranslationDisplayMode,
  TranslationLabels,
  TranslationProvider,
  TranslationStatus,
} from '@/api/tauri'
import {
  getTranslationStatus,
  onTranslationError,
  onTranslationStatus,
  startMessageTranslation,
  stopMessageTranslation,
} from '@/api/tauri'

const SETTINGS_KEY = 'questHelper_translationSettings'

export interface PersistedTranslationSettings {
  provider: TranslationProvider
  targetLanguage: string
  autoTranslate: boolean
  displayMode: TranslationDisplayMode
  accentColor: string
  summaryCount: number
  microsoftRegion: string
  llm: LlmConfig | null
}

const DEFAULT_SETTINGS: PersistedTranslationSettings = {
  provider: 'google-translate',
  targetLanguage: 'en',
  autoTranslate: false,
  displayMode: 'below',
  accentColor: '#51efc5',
  summaryCount: 50,
  microsoftRegion: '',
  llm: null,
}

const VALID_PROVIDERS: readonly TranslationProvider[] = ['google-translate', 'microsoft-translator', 'deepl']
const VALID_DISPLAY_MODES: readonly TranslationDisplayMode[] = ['below', 'replace']

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Lenient parser: a corrupted or outdated entry degrades to defaults. */
export function parsePersistedSettings(raw: string | null): PersistedTranslationSettings {
  if (!raw) return { ...DEFAULT_SETTINGS }
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return { ...DEFAULT_SETTINGS }
  }
  if (!isRecord(parsed)) return { ...DEFAULT_SETTINGS }
  const provider = VALID_PROVIDERS.includes(parsed.provider as TranslationProvider)
    ? (parsed.provider as TranslationProvider)
    : DEFAULT_SETTINGS.provider
  const displayMode = VALID_DISPLAY_MODES.includes(parsed.displayMode as TranslationDisplayMode)
    ? (parsed.displayMode as TranslationDisplayMode)
    : DEFAULT_SETTINGS.displayMode
  const llm = isRecord(parsed.llm)
    && typeof parsed.llm.baseUrl === 'string'
    && typeof parsed.llm.model === 'string'
    ? { baseUrl: parsed.llm.baseUrl, model: parsed.llm.model }
    : null
  const summaryCount = typeof parsed.summaryCount === 'number' && Number.isFinite(parsed.summaryCount)
    ? Math.min(100, Math.max(1, Math.trunc(parsed.summaryCount)))
    : DEFAULT_SETTINGS.summaryCount
  return {
    provider,
    targetLanguage: typeof parsed.targetLanguage === 'string' && parsed.targetLanguage.trim()
      ? parsed.targetLanguage
      : DEFAULT_SETTINGS.targetLanguage,
    autoTranslate: parsed.autoTranslate === true,
    displayMode,
    accentColor: typeof parsed.accentColor === 'string' && parsed.accentColor
      ? parsed.accentColor
      : DEFAULT_SETTINGS.accentColor,
    summaryCount,
    microsoftRegion: typeof parsed.microsoftRegion === 'string' ? parsed.microsoftRegion : '',
    llm,
  }
}

export const useTranslationStore = defineStore('translation', () => {
  const settings = ref<PersistedTranslationSettings>(parsePersistedSettings(
    (() => {
      try {
        return localStorage.getItem(SETTINGS_KEY)
      } catch {
        return null
      }
    })(),
  ))
  const status = ref<TranslationStatus | null>(null)
  const busy = ref(false)
  const error = ref('')

  onTranslationStatus(payload => {
    status.value = payload
  })
  onTranslationError(payload => {
    error.value = payload.message
  })

  function labels(): TranslationLabels {
    const t = i18n.global.t
    return {
      translate: t('translation.button_translate'),
      translating: t('translation.button_translating'),
      summarize: t('translation.button_summarize', { count: settings.value.summaryCount }),
      summarizing: t('translation.button_summarizing'),
      summaryTitle: t('translation.summary_title'),
      summarySubtitle: t('translation.summary_subtitle', { count: settings.value.summaryCount }),
      copy: t('translation.copy'),
      copied: t('translation.copied'),
      close: '\u00d7',
      errorPrefix: t('translation.error_prefix'),
      composerEmpty: t('translation.composer_empty'),
      llmMissing: t('translation.llm_missing'),
    }
  }

  function resolveTargetLanguage(): string {
    const configured = settings.value.targetLanguage
    if (configured !== 'auto') return configured
    return (navigator.language || 'en').split('-')[0].toLowerCase()
  }

  function buildConfig(): TranslationConfig {
    return {
      provider: settings.value.provider,
      targetLanguage: resolveTargetLanguage(),
      autoTranslate: settings.value.autoTranslate,
      displayMode: settings.value.displayMode,
      accentColor: settings.value.accentColor,
      summaryCount: settings.value.summaryCount,
      microsoftRegion: settings.value.microsoftRegion.trim() || null,
      llm: settings.value.llm,
      labels: labels(),
    }
  }

  function persist() {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings.value))
    } catch { /* Settings stay in memory when storage is unavailable. */ }
  }

  async function start(): Promise<boolean> {
    busy.value = true
    error.value = ''
    try {
      status.value = await startMessageTranslation(buildConfig())
      return status.value.enabled
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      return false
    } finally {
      busy.value = false
    }
  }

  async function stop(): Promise<void> {
    busy.value = true
    error.value = ''
    try {
      status.value = await stopMessageTranslation()
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      busy.value = false
    }
  }

  async function refreshStatus(): Promise<void> {
    try {
      status.value = await getTranslationStatus()
    } catch {
      status.value = null
    }
  }

  return {
    settings,
    status,
    busy,
    error,
    buildConfig,
    labels,
    start,
    stop,
    refreshStatus,
    persist,
  }
})
