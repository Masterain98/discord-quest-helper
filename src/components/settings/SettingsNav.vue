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

</script>

<template>
  <nav class="space-y-1 p-2" :aria-label="t('settings.title')">
    <button
      v-for="section in sections"
      :key="section.key"
      type="button"
      :aria-current="selected === section.key ? 'page' : undefined"
      :class="cn(
        'flex min-h-10 w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        props.selected === section.key
          ? 'ui-tab-active font-medium'
          : 'text-muted-foreground hover:bg-muted hover:text-foreground',
      )"
      @click="emit('update:selected', section.key)"
    >
      <component :is="section.icon" class="h-4 w-4 shrink-0" :stroke-width="1.7" />
      <span class="min-w-0 leading-snug">{{ section.label }}</span>
    </button>
  </nav>
</template>
