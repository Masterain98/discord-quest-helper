<script setup lang="ts">
import { computed } from 'vue'
import type { Component } from 'vue'
import { Gamepad2, Info, Palette, SlidersHorizontal, Stethoscope, User, Wifi } from 'lucide-vue-next'
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

const sections = computed<Array<{ key: SettingsSection, label: string, icon: Component }>>(() => [
  { key: 'account', label: t('settings.nav_account'), icon: User },
  { key: 'quest_behavior', label: t('settings.nav_quest_behavior'), icon: Gamepad2 },
  { key: 'discord_integration', label: t('settings.nav_discord_integration'), icon: Wifi },
  { key: 'appearance', label: t('settings.nav_appearance'), icon: Palette },
  { key: 'diagnostics', label: t('settings.nav_diagnostics'), icon: Stethoscope },
  { key: 'advanced', label: t('settings.nav_advanced'), icon: SlidersHorizontal },
  { key: 'about', label: t('settings.nav_about'), icon: Info },
])

function handleSelect(event: Event) {
  emit('update:selected', (event.target as HTMLSelectElement).value as SettingsSection)
}
</script>

<template>
  <div class="space-y-3">
    <select
      class="w-full rounded-md border border-border bg-card px-3 py-2 text-sm shadow-sm lg:hidden"
      :value="selected"
      @change="handleSelect"
    >
      <option v-for="section in sections" :key="section.key" :value="section.key">
        {{ section.label }}
      </option>
    </select>

    <nav class="hidden rounded-lg border bg-card p-2 shadow-sm lg:block">
      <button
        v-for="section in sections"
        :key="section.key"
        type="button"
        :class="cn(
          'mb-1 flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors last:mb-0',
          props.selected === section.key
            ? 'border border-primary/30 bg-primary/10 text-primary shadow-sm'
            : 'text-muted-foreground hover:bg-muted hover:text-foreground',
        )"
        @click="emit('update:selected', section.key)"
      >
        <component :is="section.icon" class="h-4 w-4 shrink-0" />
        <span class="truncate">{{ section.label }}</span>
      </button>
    </nav>
  </div>
</template>
