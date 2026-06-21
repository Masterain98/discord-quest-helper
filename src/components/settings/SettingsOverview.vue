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
  },
  {
    label: t('settings.overview_mode'),
    value: questsStore.gameQuestMode === 'cdp' ? t('settings.game_mode_cdp') : t('settings.game_mode_simulate'),
    ok: questsStore.gameQuestMode !== 'cdp' || questsStore.cdpAvailable,
    icon: Gamepad2,
  },
  {
    label: t('settings.overview_discord_client'),
    value: questsStore.cdpAvailable ? t('settings.cdp_connected') : t('settings.cdp_disconnected_short'),
    ok: questsStore.cdpAvailable,
    icon: questsStore.cdpAvailable ? Wifi : WifiOff,
  },
  {
    label: t('settings.overview_version'),
    value: `v${versionStore.currentVersion}`,
    ok: !versionStore.hasUpdate,
    icon: CheckCircle2,
    badge: versionStore.isChecking
      ? t('settings.version_checking')
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

  if (versionStore.hasUpdate) {
    return {
      text: t('settings.recommend_update'),
      action: t('settings.nav_about'),
      section: 'about',
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
        class="rounded-lg border bg-card p-4"
      >
        <div class="flex items-center justify-between gap-3">
          <span class="text-xs font-medium text-muted-foreground">{{ card.label }}</span>
          <component :is="card.icon" class="h-4 w-4 text-muted-foreground" />
        </div>
        <div class="mt-2 flex items-center gap-2">
          <p class="truncate text-sm font-semibold">{{ card.value }}</p>
          <Badge :variant="card.ok ? 'outline' : 'secondary'" class="shrink-0 text-[10px]">
            {{ card.badge ?? (card.ok ? t('settings.status_ok') : t('settings.status_attention')) }}
          </Badge>
        </div>
      </div>
    </div>

    <div
      v-if="recommendation"
      class="flex flex-col gap-3 rounded-lg border border-amber-500/30 bg-amber-500/10 p-4 text-sm sm:flex-row sm:items-center sm:justify-between"
    >
      <div class="flex items-start gap-2 text-amber-700 dark:text-amber-300">
        <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ recommendation.text }}</span>
      </div>
      <Button variant="outline" size="sm" class="shrink-0" @click="emit('selectSection', recommendation.section)">
        {{ recommendation.action }}
      </Button>
    </div>
  </div>
</template>
