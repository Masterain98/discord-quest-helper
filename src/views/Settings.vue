<script setup lang="ts">
import { onMounted } from 'vue'
import AccountSettings from '@/components/settings/AccountSettings.vue'
import AdvancedSettings from '@/components/settings/AdvancedSettings.vue'
import AppearanceSettings from '@/components/settings/AppearanceSettings.vue'
import AboutSettings from '@/components/settings/AboutSettings.vue'
import DiagnosticsSettings from '@/components/settings/DiagnosticsSettings.vue'
import DiscordIntegrationSettings from '@/components/settings/DiscordIntegrationSettings.vue'
import QuestBehaviorSettings from '@/components/settings/QuestBehaviorSettings.vue'
import SettingsOverview from '@/components/settings/SettingsOverview.vue'
import type { SettingsSection } from '@/composables/useSettingsNavigation'
import { useQuestsStore } from '@/stores/quests'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const questsStore = useQuestsStore()
defineProps<{ selectedSection: SettingsSection }>()

const emit = defineEmits<{
  'update:selectedSection': [section: SettingsSection]
  'navigate-to-home': []
  'debug-unlocked': []
  'debug-disabled': []
}>()

onMounted(() => {
  questsStore.initCdpMode({ preserveMode: true }).catch(err => {
    console.warn('CDP init failed:', err)
  })
})
</script>

<template>
  <div class="settings-view fade-in space-y-6 select-none">
    <div class="space-y-2">
      <h2 class="ui-page-heading text-2xl font-semibold">{{ t('settings.title') }}</h2>
      <p class="max-w-3xl text-sm leading-relaxed text-muted-foreground">{{ t('settings.control_center_desc') }}</p>
    </div>

    <SettingsOverview @select-section="emit('update:selectedSection', $event)" />

    <div class="min-w-0">
      <AccountSettings
        v-if="selectedSection === 'account'"
        @navigate-to-home="emit('navigate-to-home')"
      />
      <QuestBehaviorSettings v-else-if="selectedSection === 'quest_behavior'" />
      <DiscordIntegrationSettings v-else-if="selectedSection === 'discord_integration'" />
      <AppearanceSettings v-else-if="selectedSection === 'appearance'" />
      <DiagnosticsSettings v-else-if="selectedSection === 'diagnostics'" />
      <AdvancedSettings v-else-if="selectedSection === 'advanced'" />
      <AboutSettings
        v-else
        @debug-unlocked="emit('debug-unlocked')"
        @debug-disabled="emit('debug-disabled')"
      />
    </div>
  </div>
</template>
