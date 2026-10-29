<script setup lang="ts">
import { ref, watch } from 'vue'
import AccountSettings from '@/components/settings/AccountSettings.vue'
import AdvancedSettings from '@/components/settings/AdvancedSettings.vue'
import AppearanceSettings from '@/components/settings/AppearanceSettings.vue'
import AboutSettings from '@/components/settings/AboutSettings.vue'
import DiagnosticsSettings from '@/components/settings/DiagnosticsSettings.vue'
import DiscordIntegrationSettings from '@/components/settings/DiscordIntegrationSettings.vue'
import LiveTunnelSettings from '@/components/settings/LiveTunnelSettings.vue'
import QuestBehaviorSettings from '@/components/settings/QuestBehaviorSettings.vue'
import SettingsOverview from '@/components/settings/SettingsOverview.vue'
import TranslationSettings from '@/components/settings/TranslationSettings.vue'
import { useQuestsStore } from '@/stores/quests'
import type { SettingsSection } from '@/composables/useSettingsNavigation'
import { useSettingsScroll } from '@/composables/useSettingsScroll'
import { useSettingsSearch, type SettingsSearchResult } from '@/composables/useSettingsSearch'
import { isDebugModeEnabled } from '@/utils/debugMode'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const questsStore = useQuestsStore()
const props = withDefaults(defineProps<{
  selectedSection: SettingsSection
  navigationRequest?: number
  navigationTarget?: string | null
  searchQuery?: string
}>(), { navigationRequest: 0, searchQuery: '' })
const emit = defineEmits<{
  'update:selectedSection': [section: SettingsSection]
  'navigate-to-home': []
  'debug-unlocked': []
  'debug-disabled': []
  'search-results': [results: SettingsSearchResult[]]
}>()
const page = ref<HTMLElement | null>(null)
const developerModeEnabled = ref(isDebugModeEnabled())
function updateDeveloperMode(enabled: boolean) {
  developerModeEnabled.value = enabled
  if (enabled) emit('debug-unlocked')
  else emit('debug-disabled')
}
const { scrollToSection } = useSettingsScroll(page, {
  initialSection: () => props.selectedSection,
  hasInitialRequest: () => props.navigationRequest > 0,
  onVisible: section => emit('update:selectedSection', section),
})
watch(() => props.navigationRequest, () => scrollToSection(props.selectedSection, undefined, props.navigationTarget))
useSettingsSearch(page, () => props.searchQuery, results => emit('search-results', results))
</script>

<template>
  <div ref="page" class="settings-view fade-in">
    <header class="settings-page-heading">
      <h1 class="ui-page-heading text-2xl font-semibold">{{ t('settings.title') }}</h1>
      <p class="mt-2 max-w-3xl text-sm leading-relaxed text-muted-foreground">{{ t('settings.control_center_desc') }}</p>
    </header>
    <SettingsOverview @select-section="scrollToSection($event)" />
    <div class="settings-sections">
      <section id="settings-section-account" data-settings-section="account" :aria-label="t('settings.nav_account')">
        <AccountSettings @navigate-to-home="emit('navigate-to-home')" />
      </section>
      <section id="settings-section-quest_behavior" data-settings-section="quest_behavior" :aria-label="t('settings.nav_quest_behavior')">
        <QuestBehaviorSettings />
      </section>
      <section id="settings-section-discord_integration" data-settings-section="discord_integration" :aria-label="t('settings.nav_discord_integration')">
        <DiscordIntegrationSettings />
      </section>
      <section
        id="settings-section-translation"
        data-settings-section="translation"
        :aria-label="t('settings.nav_translation')"
      >
        <TranslationSettings />
      </section>
      <section
        v-if="questsStore.platformCapabilities?.os === 'windows'"
        id="settings-section-live_tunnel"
        data-settings-section="live_tunnel"
        :aria-label="t('settings.nav_live_tunnel')"
      >
        <LiveTunnelSettings />
      </section>
      <section id="settings-section-appearance" data-settings-section="appearance" :aria-label="t('settings.nav_appearance')">
        <AppearanceSettings />
      </section>
      <section id="settings-section-diagnostics" data-settings-section="diagnostics" :aria-label="t('settings.nav_diagnostics')">
        <DiagnosticsSettings />
      </section>
      <section id="settings-section-advanced" data-settings-section="advanced" :aria-label="t('settings.nav_advanced')">
        <AdvancedSettings :debug-mode-enabled="developerModeEnabled" />
      </section>
      <section id="settings-section-about" data-settings-section="about" :aria-label="t('settings.nav_about')">
        <AboutSettings @debug-unlocked="updateDeveloperMode(true)" @debug-disabled="updateDeveloperMode(false)" />
      </section>
    </div>
  </div>
</template>
