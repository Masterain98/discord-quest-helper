<script setup lang="ts">
import { Palette } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { useQuestsStore } from '@/stores/quests'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingRow from './SettingRow.vue'
import { Switch } from '@/components/ui/switch'

const { t } = useI18n()
const questsStore = useQuestsStore()
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.nav_appearance')"
    :description="t('settings.display_desc')"
    :icon="Palette"
    tone="primary"
  >
    <SettingRow :label="t('settings.show_orbs_balance')" :description="t('settings.show_orbs_balance_desc')">
      <Switch v-model="questsStore.showOrbsBalance" :aria-label="t('settings.show_orbs_balance')" />
    </SettingRow>
  </SettingsSectionCard>
</template>
