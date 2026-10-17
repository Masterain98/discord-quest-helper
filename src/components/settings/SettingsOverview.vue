<script setup lang="ts">
import { computed } from 'vue'
import { AlertTriangle, CheckCircle2, Gamepad2, User, Wifi, WifiOff } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'
import type { SettingsSection } from '@/composables/useSettingsNavigation'
import { cn } from '@/lib/utils'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import { settingToneClass } from './settingTones'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const versionStore = useVersionStore()

const emit = defineEmits<{
  selectSection: [section: SettingsSection]
}>()

const cards = computed(() => [
  {
    label: t('settings.overview_account'),
    value: authStore.user ? `@${authStore.user.username}` : t('settings.overview_not_connected'),
    icon: User,
    tone: authStore.user ? 'success' : 'warning',
    emphasizeValue: !authStore.user,
  },
  {
    label: t('settings.overview_mode'),
    value: t(`settings.tooltip_${questsStore.gameQuestMode}_title`),
    icon: Gamepad2,
    tone: questsStore.gameQuestMode === 'heartbeat' ? 'danger' : 'primary',
    emphasizeValue: true,
  },
  {
    label: t('settings.overview_discord_client'),
    value: questsStore.cdpAvailable ? t('settings.cdp_connected') : t('settings.cdp_disconnected_short'),
    icon: questsStore.cdpAvailable ? Wifi : WifiOff,
    tone: questsStore.cdpAvailable ? 'success' : 'warning',
    emphasizeValue: true,
  },
  {
    label: t('settings.overview_version'),
    value: `v${versionStore.currentVersion}`,
    icon: CheckCircle2,
    tone: versionStore.hasUpdate ? 'primary' : versionStore.isPreRelease ? 'warning' : versionStore.isLatest ? 'success' : 'neutral',
    emphasizeValue: false,
    badge: versionStore.isChecking
      ? t('settings.version_checking')
      : versionStore.isPreRelease
        ? t('settings.version_prerelease')
        : versionStore.hasUpdate
          ? t('version.update_available')
          : t('settings.version_latest'),
  },
])

const recommendation = computed<{ text: string, action: string, section: SettingsSection } | null>(() => {
  if (!authStore.user) {
    return {
      text: t('settings.recommend_account'),
      action: t('settings.nav_account'),
      section: 'account',
    }
  }

  if (questsStore.gameQuestMode === 'cdp' && !questsStore.cdpAvailable) {
    return {
      text: t('settings.recommend_integration'),
      action: t('settings.nav_discord_integration'),
      section: 'discord_integration',
    }
  }

  return null
})
</script>

<template>
  <div class="space-y-3">
    <dl class="settings-overview-grid ui-recess">
      <div v-for="card in cards" :key="card.label" class="settings-overview-item" :data-tone="card.tone">
        <dt class="flex items-center gap-2 text-xs text-muted-foreground">
          <component :is="card.icon" class="settings-overview-icon h-3.5 w-3.5 shrink-0" :stroke-width="1.7" aria-hidden="true" />
          {{ card.label }}
        </dt>
        <dd class="mt-2 flex min-w-0 flex-wrap items-center gap-2">
          <span class="settings-overview-value min-w-0 break-words text-sm font-semibold" :class="{ 'settings-overview-value-status': card.emphasizeValue }">{{ card.value }}</span>
          <span v-if="card.badge" class="settings-overview-status">{{ card.badge }}</span>
        </dd>
      </div>
    </dl>

    <SettingsStatusPanel
      v-if="recommendation"
      tone="warning"
      :icon="AlertTriangle"
    >
      <span>{{ recommendation.text }}</span>
      <template #actions>
        <Button
          variant="outline"
          size="sm"
          :class="cn('shrink-0', settingToneClass.warning.buttonSoft)"
          @click="emit('selectSection', recommendation.section)"
        >
          {{ recommendation.action }}
        </Button>
      </template>
    </SettingsStatusPanel>
  </div>
</template>
