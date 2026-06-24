<script setup lang="ts">
import { computed } from 'vue'
import { AlertTriangle, CheckCircle2, Gamepad2, User, Wifi, WifiOff } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'
import type { SettingsSection } from '@/composables/useSettingsNavigation'
import { cn } from '@/lib/utils'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import { settingToneClass, type SettingsTone } from './settingTones'

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
    ok: !!authStore.user,
    icon: User,
    tone: 'success' as SettingsTone,
  },
  {
    label: t('settings.overview_mode'),
    value: questsStore.gameQuestMode === 'cdp' ? t('settings.game_mode_cdp') : t('settings.game_mode_simulate'),
    ok: questsStore.gameQuestMode !== 'cdp' || questsStore.cdpAvailable,
    icon: Gamepad2,
    tone: 'violet' as SettingsTone,
  },
  {
    label: t('settings.overview_discord_client'),
    value: questsStore.cdpAvailable ? t('settings.cdp_connected') : t('settings.cdp_disconnected_short'),
    ok: questsStore.cdpAvailable,
    icon: questsStore.cdpAvailable ? Wifi : WifiOff,
    tone: 'info' as SettingsTone,
  },
  {
    label: t('settings.overview_version'),
    value: `v${versionStore.currentVersion}`,
    ok: !versionStore.hasUpdate,
    icon: CheckCircle2,
    tone: 'primary' as SettingsTone,
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
    <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
      <div
        v-for="card in cards"
        :key="card.label"
        :class="cn(
          'rounded-lg border bg-card p-4 transition-all hover:-translate-y-0.5 hover:shadow-sm',
          card.ok ? settingToneClass[card.tone].card : settingToneClass.warning.card,
        )"
      >
        <div class="flex items-center justify-between gap-3">
          <span class="text-xs font-medium text-muted-foreground">{{ card.label }}</span>
          <div :class="cn('flex h-8 w-8 shrink-0 items-center justify-center rounded-md', settingToneClass[card.ok ? card.tone : 'warning'].icon)">
            <component :is="card.icon" class="h-4 w-4" />
          </div>
        </div>
        <div class="mt-2 flex items-center gap-2">
          <p class="truncate text-sm font-semibold">{{ card.value }}</p>
          <Badge
            variant="outline"
            :class="cn('shrink-0 text-[10px]', settingToneClass[card.ok ? card.tone : 'warning'].badge)"
          >
            {{ card.badge ?? (card.ok ? t('settings.status_ok') : t('settings.status_attention')) }}
          </Badge>
        </div>
      </div>
    </div>

    <SettingsStatusPanel
      v-if="recommendation"
      tone="warning"
      :icon="AlertTriangle"
    >
      <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <span>{{ recommendation.text }}</span>
        <Button
          variant="outline"
          size="sm"
          :class="cn('shrink-0', settingToneClass.warning.buttonSoft)"
          @click="emit('selectSection', recommendation.section)"
        >
          {{ recommendation.action }}
        </Button>
      </div>
    </SettingsStatusPanel>
  </div>
</template>
