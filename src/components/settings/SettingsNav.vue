<script setup lang="ts">
import { computed, useId } from 'vue'
import type { Component } from 'vue'
import { Gamepad2, Info, Palette, Search, SlidersHorizontal, Stethoscope, User, Wifi, X } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import type { SettingsSection } from '@/composables/useSettingsNavigation'
import { cn } from '@/lib/utils'
import { Input } from '@/components/ui/input'
import type { SettingsSearchResult } from '@/composables/useSettingsSearch'

const { t } = useI18n()

const props = defineProps<{
  selected: SettingsSection
  query: string
  results: SettingsSearchResult[]
}>()

const emit = defineEmits<{
  'update:selected': [section: SettingsSection]
  'update:query': [query: string]
  'select-result': [result: SettingsSearchResult]
}>()
const resultsId = useId()

function handleSearchKey(event: KeyboardEvent) {
  if (event.key === 'Escape' && props.query) {
    event.preventDefault()
    event.stopPropagation()
    emit('update:query', '')
  }
  if (event.key === 'Enter' && props.query.trim() && props.results[0]) {
    event.preventDefault()
    emit('select-result', props.results[0])
  }
}

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
  <div class="settings-nav-search sticky top-0 z-10 px-3 pt-1 pb-3">
    <div class="relative">
      <Search class="pointer-events-none absolute top-3 left-2.5 z-10 h-4 w-4 text-muted-foreground" aria-hidden="true" />
      <Input type="search" :model-value="query" :placeholder="t('settings.search_settings')" :aria-label="t('settings.search_settings')" :aria-controls="query.trim() ? resultsId : undefined" class="settings-search-input pl-9 pr-8" @update:model-value="emit('update:query', String($event))" @keydown="handleSearchKey" />
      <button v-if="query" type="button" class="absolute top-1 right-1 flex h-8 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" :aria-label="t('general.clear')" @click="emit('update:query', '')">
        <X class="h-3.5 w-3.5" aria-hidden="true" />
      </button>
    </div>
  </div>
  <nav v-if="!query.trim()" class="settings-nav space-y-1 p-2 pt-0" :aria-label="t('settings.title')">
    <a
      v-for="section in sections"
      :key="section.key"
      :href="`#settings-section-${section.key}`"
      :aria-current="selected === section.key ? 'location' : undefined"
      :class="cn(
        'flex min-h-10 w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        props.selected === section.key
          ? 'ui-tab-active font-medium'
          : 'text-muted-foreground hover:bg-muted hover:text-foreground',
      )"
      @click.prevent="emit('update:selected', section.key)"
    >
      <component :is="section.icon" class="h-4 w-4 shrink-0" :stroke-width="1.7" />
      <span class="min-w-0 leading-snug">{{ section.label }}</span>
    </a>
  </nav>
  <div v-else :id="resultsId" class="px-2 pb-2">
    <p role="status" class="px-3 pb-2 text-xs leading-relaxed text-muted-foreground">{{ results.length ? t('settings.search_results', { count: results.length }) : t('settings.search_no_results') }}</p>
    <nav class="space-y-1" :aria-label="t('settings.search_settings')">
      <a v-for="result in results" :key="result.id" :href="`#${result.id}`" class="block rounded-xl px-3 py-2.5 text-sm transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" @click.prevent="emit('select-result', result)">
        <span class="block leading-snug break-words">{{ result.label }}</span>
        <span v-if="result.label !== result.sectionLabel" class="mt-1 block text-xs leading-snug text-muted-foreground">{{ result.sectionLabel }}</span>
      </a>
    </nav>
  </div>
</template>
