<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { CheckCircle2, CircleAlert, Languages, KeyRound, Loader2 } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  clearProviderKey,
  isProviderKeySet,
  setProviderKey,
  testTranslationProvider,
  type LlmConfig,
  type ProviderKeyRef,
  type TestProviderRequest,
} from '@/api/tauri'
import { useTranslationStore } from '@/stores/translation'
import { commandErrorMessage } from '@/utils/commandError'
import SettingRow from './SettingRow.vue'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import type { SettingsTone } from './settingTones'

const { t, locale } = useI18n()
const store = useTranslationStore()

const keyBusy = ref<ProviderKeyRef | null>(null)
const keyDrafts = ref<Record<ProviderKeyRef, string>>({ microsoft: '', deepl: '', llm: '' })
const keysConfigured = ref<Record<ProviderKeyRef, boolean>>({ microsoft: false, deepl: false, llm: false })
const testBusy = ref(false)
const testResult = ref('')
const testError = ref('')
const llmBusy = ref(false)
const llmResult = ref('')
const llmError = ref('')

const TARGET_LANGUAGES = [
  'auto', 'en', 'pt-BR', 'es', 'fr', 'de', 'it', 'ja', 'ko', 'ru', 'zh', 'zh-TW',
  'ar', 'tr', 'vi', 'th', 'id', 'pl', 'hi', 'nl',
] as const

const languageNames = computed<Record<string, string>>(() => {
  const display = new Intl.DisplayNames([locale.value], { type: 'language' })
  return Object.fromEntries(TARGET_LANGUAGES.map(code => [
    code,
    code === 'auto' ? t('translation.target_auto') : display.of(code) ?? code,
  ]))
})

const running = computed(() => store.status?.enabled === true)
const lastError = computed(() => store.error || store.status?.lastError || '')

function configSnapshot() {
  return JSON.stringify(store.buildConfig())
}
const appliedSnapshot = ref('')
const needsApply = computed(() => running.value && appliedSnapshot.value !== configSnapshot())

watch(
  () => store.settings,
  () => {
    store.persist()
    testResult.value = ''
    testError.value = ''
  },
  { deep: true },
)

function setLlmField(field: keyof LlmConfig, value: string) {
  const current = store.settings.llm ?? { baseUrl: '', model: '' }
  const next: LlmConfig = { ...current, [field]: value }
  store.settings.llm = next.baseUrl.trim() || next.model.trim() ? next : null
}

const llmBaseUrl = computed({
  get: () => store.settings.llm?.baseUrl ?? '',
  set: value => setLlmField('baseUrl', value),
})
const llmModel = computed({
  get: () => store.settings.llm?.model ?? '',
  set: value => setLlmField('model', value),
})

async function refreshKeyState() {
  for (const keyRef of ['microsoft', 'deepl', 'llm'] as const) {
    try {
      keysConfigured.value[keyRef] = await isProviderKeySet(keyRef)
    } catch {
      keysConfigured.value[keyRef] = false
    }
  }
}

async function saveKey(keyRef: ProviderKeyRef) {
  const draft = keyDrafts.value[keyRef].trim()
  if (!draft) return
  keyBusy.value = keyRef
  try {
    await setProviderKey(keyRef, draft)
    keyDrafts.value[keyRef] = ''
    await refreshKeyState()
  } catch (e) {
    testError.value = commandErrorMessage(e)
  } finally {
    keyBusy.value = null
  }
}

async function removeKey(keyRef: ProviderKeyRef) {
  keyBusy.value = keyRef
  try {
    await clearProviderKey(keyRef)
    await refreshKeyState()
  } catch (e) {
    testError.value = commandErrorMessage(e)
  } finally {
    keyBusy.value = null
  }
}

async function start() {
  testError.value = ''
  const enabled = await store.start()
  if (enabled) appliedSnapshot.value = configSnapshot()
}

async function stop() {
  await store.stop()
  appliedSnapshot.value = ''
}

async function testProvider() {
  testBusy.value = true
  testResult.value = ''
  testError.value = ''
  const request: TestProviderRequest = {
    kind: 'message',
    provider: store.settings.provider,
    targetLanguage: store.settings.targetLanguage === 'auto' ? 'en' : store.settings.targetLanguage,
    microsoftRegion: store.settings.microsoftRegion.trim() || null,
    llm: store.settings.llm,
  }
  try {
    testResult.value = await testTranslationProvider(request)
  } catch (e) {
    testError.value = commandErrorMessage(e)
  } finally {
    testBusy.value = false
  }
}

async function testLlm() {
  llmBusy.value = true
  llmResult.value = ''
  llmError.value = ''
  const request: TestProviderRequest = {
    kind: 'llm',
    provider: store.settings.provider,
    targetLanguage: 'en',
    microsoftRegion: null,
    llm: store.settings.llm,
  }
  try {
    llmResult.value = await testTranslationProvider(request)
  } catch (e) {
    llmError.value = commandErrorMessage(e)
  } finally {
    llmBusy.value = false
  }
}

const statusPanel = computed<{ tone: SettingsTone, icon: typeof CheckCircle2, text: string }>(() => {
  if (!store.status) {
    return { tone: 'neutral', icon: Loader2, text: t('general.loading') }
  }
  if (running.value) {
    return { tone: 'success', icon: CheckCircle2, text: t('translation.status_running') }
  }
  if (lastError.value) {
    return { tone: 'warning', icon: CircleAlert, text: t('translation.status_error') }
  }
  return { tone: 'neutral', icon: Languages, text: t('translation.status_stopped') }
})

onMounted(() => {
  void store.refreshStatus()
  void refreshKeyState()
})
</script>

<template>
  <SettingsSectionCard
    :title="t('translation.section_title')"
    :description="t('translation.section_desc')"
    :icon="Languages"
    tone="info"
  >
    <SettingsStatusPanel :tone="statusPanel.tone" :icon="statusPanel.icon">
      <span role="status">{{ statusPanel.text }}</span>
      <span
        v-if="running"
        class="text-xs text-muted-foreground"
      >{{ t('translation.stats', { translated: store.status?.translated ?? 0, cacheHits: store.status?.cacheHits ?? 0, errors: store.status?.errors ?? 0 }) }}</span>
      <template #actions>
        <Button
          v-if="!running"
          size="sm"
          :disabled="store.busy"
          @click="start"
        >
          <Loader2
            v-if="store.busy"
            class="mr-2 h-4 w-4 animate-spin"
          />
          {{ t('translation.start') }}
        </Button>
        <Button
          v-else-if="needsApply"
          size="sm"
          :disabled="store.busy"
          @click="start"
        >
          {{ t('translation.apply') }}
        </Button>
        <Button
          v-if="running"
          variant="destructive"
          size="sm"
          :disabled="store.busy"
          @click="stop"
        >
          {{ t('translation.stop') }}
        </Button>
      </template>
    </SettingsStatusPanel>

    <SettingRow
      :label="t('translation.auto_translate_label')"
      :description="t('translation.auto_translate_desc')"
    >
      <Switch
        v-model="store.settings.autoTranslate"
        :aria-label="t('translation.auto_translate_label')"
      />
    </SettingRow>

    <SettingRow
      :label="t('translation.target_label')"
      :description="t('translation.target_desc')"
    >
      <select
        v-model="store.settings.targetLanguage"
        class="h-10 max-w-56 rounded-md border bg-background px-2 text-xs"
        :aria-label="t('translation.target_label')"
      >
        <option
          v-for="code in TARGET_LANGUAGES"
          :key="code"
          :value="code"
        >
          {{ languageNames[code] }}
        </option>
      </select>
    </SettingRow>

    <SettingRow
      :label="t('translation.display_label')"
      :description="t('translation.display_desc')"
    >
      <select
        v-model="store.settings.displayMode"
        class="h-10 max-w-56 rounded-md border bg-background px-2 text-xs"
        :aria-label="t('translation.display_label')"
      >
        <option value="below">
          {{ t('translation.display_below') }}
        </option>
        <option value="replace">
          {{ t('translation.display_replace') }}
        </option>
      </select>
    </SettingRow>

    <SettingRow
      :label="t('translation.accent_label')"
      :description="t('translation.accent_desc')"
    >
      <Input
        v-model="store.settings.accentColor"
        type="color"
        class="h-10 w-20 p-1"
        :aria-label="t('translation.accent_label')"
      />
    </SettingRow>

    <SettingRow
      :label="t('translation.provider_label')"
      :description="t('translation.provider_desc')"
    >
      <select
        v-model="store.settings.provider"
        class="h-10 max-w-64 rounded-md border bg-background px-2 text-xs"
        :aria-label="t('translation.provider_label')"
      >
        <option value="google-translate">
          {{ t('translation.provider_google') }}
        </option>
        <option value="microsoft-translator">
          {{ t('translation.provider_microsoft') }}
        </option>
        <option value="deepl">
          {{ t('translation.provider_deepl') }}
        </option>
      </select>
    </SettingRow>

    <SettingRow
      v-if="store.settings.provider === 'microsoft-translator'"
      :label="t('translation.microsoft_region_label')"
      :description="t('translation.microsoft_region_desc')"
    >
      <Input
        v-model="store.settings.microsoftRegion"
        class="max-w-56"
        placeholder="global"
        autocomplete="off"
        :aria-label="t('translation.microsoft_region_label')"
      />
    </SettingRow>

    <SettingRow
      v-if="store.settings.provider === 'microsoft-translator'"
      :label="t('translation.key_microsoft_label')"
      :description="t('translation.key_microsoft_desc')"
    >
      <div class="flex flex-wrap items-center gap-2">
        <Input
          v-model="keyDrafts.microsoft"
          type="password"
          class="max-w-56"
          autocomplete="off"
          :placeholder="keysConfigured.microsoft ? t('translation.key_saved') : t('translation.key_missing')"
          :aria-label="t('translation.key_microsoft_label')"
        />
        <Button
          variant="outline"
          size="sm"
          :disabled="keyBusy !== null || !keyDrafts.microsoft.trim()"
          @click="saveKey('microsoft')"
        >
          {{ t('translation.key_save') }}
        </Button>
        <Button
          v-if="keysConfigured.microsoft"
          variant="ghost"
          size="sm"
          :disabled="keyBusy !== null"
          @click="removeKey('microsoft')"
        >
          {{ t('translation.key_clear') }}
        </Button>
      </div>
    </SettingRow>

    <SettingRow
      v-if="store.settings.provider === 'deepl'"
      :label="t('translation.key_deepl_label')"
      :description="t('translation.key_deepl_desc')"
    >
      <div class="flex flex-wrap items-center gap-2">
        <Input
          v-model="keyDrafts.deepl"
          type="password"
          class="max-w-56"
          autocomplete="off"
          :placeholder="keysConfigured.deepl ? t('translation.key_saved') : t('translation.key_missing')"
          :aria-label="t('translation.key_deepl_label')"
        />
        <Button
          variant="outline"
          size="sm"
          :disabled="keyBusy !== null || !keyDrafts.deepl.trim()"
          @click="saveKey('deepl')"
        >
          {{ t('translation.key_save') }}
        </Button>
        <Button
          v-if="keysConfigured.deepl"
          variant="ghost"
          size="sm"
          :disabled="keyBusy !== null"
          @click="removeKey('deepl')"
        >
          {{ t('translation.key_clear') }}
        </Button>
      </div>
    </SettingRow>

    <div class="flex flex-wrap items-center gap-3 pt-2">
      <Button
        variant="outline"
        size="sm"
        :disabled="testBusy"
        @click="testProvider"
      >
        <Loader2
          v-if="testBusy"
          class="mr-2 h-4 w-4 animate-spin"
        />
        {{ t('translation.test_button') }}
      </Button>
      <span
        v-if="testResult"
        class="text-xs text-muted-foreground"
      >{{ t('translation.test_result', { result: testResult }) }}</span>
    </div>
    <p
      v-if="testError"
      role="alert"
      class="text-xs leading-relaxed text-destructive"
    >
      {{ testError }}
    </p>
    <p
      v-if="lastError"
      role="alert"
      class="text-xs leading-relaxed text-destructive"
    >
      {{ lastError }}
    </p>
  </SettingsSectionCard>

  <SettingsSectionCard
    class="mt-7 sm:mt-9"
    :title="t('translation.llm_title')"
    :description="t('translation.llm_desc')"
    :icon="KeyRound"
    tone="violet"
  >
    <SettingRow
      :label="t('translation.llm_url_label')"
      :description="t('translation.llm_url_desc')"
    >
      <Input
        :model-value="llmBaseUrl"
        class="max-w-96"
        placeholder="https://api.openai.com/v1"
        autocomplete="off"
        :aria-label="t('translation.llm_url_label')"
        @update:model-value="value => setLlmField('baseUrl', String(value))"
      />
    </SettingRow>
    <SettingRow
      :label="t('translation.llm_model_label')"
      :description="t('translation.llm_model_desc')"
    >
      <Input
        :model-value="llmModel"
        class="max-w-72"
        placeholder="gpt-4o-mini"
        autocomplete="off"
        :aria-label="t('translation.llm_model_label')"
        @update:model-value="value => setLlmField('model', String(value))"
      />
    </SettingRow>
    <SettingRow
      :label="t('translation.key_llm_label')"
      :description="t('translation.key_llm_desc')"
    >
      <div class="flex flex-wrap items-center gap-2">
        <Input
          v-model="keyDrafts.llm"
          type="password"
          class="max-w-56"
          autocomplete="off"
          :placeholder="keysConfigured.llm ? t('translation.key_saved') : t('translation.key_missing')"
          :aria-label="t('translation.key_llm_label')"
        />
        <Button
          variant="outline"
          size="sm"
          :disabled="keyBusy !== null || !keyDrafts.llm.trim()"
          @click="saveKey('llm')"
        >
          {{ t('translation.key_save') }}
        </Button>
        <Button
          v-if="keysConfigured.llm"
          variant="ghost"
          size="sm"
          :disabled="keyBusy !== null"
          @click="removeKey('llm')"
        >
          {{ t('translation.key_clear') }}
        </Button>
      </div>
    </SettingRow>

    <div class="flex flex-wrap items-center gap-3 pt-2">
      <Button
        variant="outline"
        size="sm"
        :disabled="llmBusy"
        @click="testLlm"
      >
        <Loader2
          v-if="llmBusy"
          class="mr-2 h-4 w-4 animate-spin"
        />
        {{ t('translation.llm_test_button') }}
      </Button>
      <span
        v-if="llmResult"
        class="text-xs text-muted-foreground"
      >{{ t('translation.test_result', { result: llmResult }) }}</span>
    </div>
    <p
      v-if="llmError"
      role="alert"
      class="text-xs leading-relaxed text-destructive"
    >
      {{ llmError }}
    </p>
    <p class="text-xs leading-relaxed text-muted-foreground">
      {{ t('translation.llm_privacy_note') }}
    </p>
  </SettingsSectionCard>
</template>
