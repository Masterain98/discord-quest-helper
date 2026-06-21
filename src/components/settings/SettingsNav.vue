<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SettingsSection } from '@/composables/useSettingsNavigation'
import { cn } from '@/lib/utils'

const { t } = useI18n()

const props = defineProps<{
  selected: SettingsSection
}>()

const emit = defineEmits<{
  'update:selected': [section: SettingsSection]
}>()

const sections = computed<Array<{ key: SettingsSection, label: string }>>(() => [
  { key: 'account', label: t('settings.nav_account') },
  { key: 'quest_behavior', label: t('settings.nav_quest_behavior') },
  { key: 'discord_integration', label: t('settings.nav_discord_integration') },
  { key: 'appearance', label: t('settings.nav_appearance') },
  { key: 'diagnostics', label: t('settings.nav_diagnostics') },
  { key: 'advanced', label: t('settings.nav_advanced') },
  { key: 'about', label: t('settings.nav_about') },
])

function handleSelect(event: Event) {
  emit('update:selected', (event.target as HTMLSelectElement).value as SettingsSection)
}
</script>

<template>
  <div class="space-y-3">
    <select
      class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm lg:hidden"
      :value="selected"
      @change="handleSelect"
    >
      <option v-for="section in sections" :key="section.key" :value="section.key">
        {{ section.label }}
      </option>
    </select>

    <nav class="hidden rounded-lg border bg-card p-2 lg:block">
      <button
        v-for="section in sections"
        :key="section.key"
        type="button"
        :class="cn(
          'mb-1 flex w-full items-center rounded-md px-3 py-2 text-left text-sm transition-colors last:mb-0',
          props.selected === section.key
            ? 'bg-secondary text-secondary-foreground'
            : 'text-muted-foreground hover:bg-muted hover:text-foreground',
        )"
        @click="emit('update:selected', section.key)"
      >
        {{ section.label }}
      </button>
    </nav>
  </div>
</template>
