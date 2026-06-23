<script setup lang="ts">
import { Palette } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { useQuestsStore } from '@/stores/quests'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsSwitch from './SettingsSwitch.vue'
import { settingToneClass } from './settingTones'

const { t } = useI18n()
const questsStore = useQuestsStore()
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.display')"
    :description="t('settings.display_desc')"
    :icon="Palette"
    tone="primary"
  >
    <div class="flex items-center justify-between gap-4 rounded-lg border border-border bg-card p-4 transition-colors hover:bg-muted/40">
      <div class="flex min-w-0 items-start gap-3">
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-primary/10">
          <img src="/icons/orbs.png" alt="" class="h-6 w-6 object-contain" />
        </div>
        <div class="min-w-0">
          <p class="flex flex-wrap items-center gap-2 text-sm font-semibold">
            {{ t('settings.show_orbs_balance') }}
            <Badge
              variant="outline"
              :class="questsStore.showOrbsBalance ? settingToneClass.success.badge : settingToneClass.neutral.badge"
            >
              {{ questsStore.showOrbsBalance ? t('settings.status_ok') : t('settings.status_attention') }}
            </Badge>
          </p>
          <p class="mt-1 text-xs text-muted-foreground">
            {{ t('settings.show_orbs_balance_desc') }}
          </p>
        </div>
      </div>

      <SettingsSwitch v-model="questsStore.showOrbsBalance" />
    </div>
  </SettingsSectionCard>
</template>
