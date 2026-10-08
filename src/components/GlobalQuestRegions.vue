<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Globe2, Slash } from 'lucide-vue-next'
import type { GlobalQuestRestriction } from '@/types/globalQuests'

const props = defineProps<{ restriction: GlobalQuestRestriction }>()
const { t, locale } = useI18n()
const names = computed(() => new Intl.DisplayNames([locale.value], { type: 'region' }))
const groups = computed(() => [
  { codes: props.restriction.include, excluded: false },
  { codes: props.restriction.exclude, excluded: true },
].filter(group => group.codes.length))
const summary = computed(() => {
  if (props.restriction.region !== 'restricted') return t(`global_quests.regions.${props.restriction.region}`)
  return groups.value.map(group => t(group.excluded ? 'global_quests.except_countries' : 'global_quests.only_countries', {
    countries: group.codes.map(code => `${names.value.of(code) ?? code} (${code})`).join(', '),
  })).join('; ')
})
</script>

<template>
  <span class="inline-flex flex-wrap items-center gap-1.5 text-xs" :title="summary" :aria-label="`${t('global_quests.region_label')}: ${summary}`">
    <span class="text-muted-foreground" aria-hidden="true">{{ t('global_quests.region_label') }}:</span>
    <span v-if="restriction.region === 'unknown'">{{ t('global_quests.unknown') }}</span>
    <template v-else>
      <Globe2 v-if="restriction.region === 'global' || !restriction.include.length" class="h-4 w-4 text-muted-foreground" aria-hidden="true" />
      <span v-for="group in groups" :key="String(group.excluded)" class="inline-flex items-center gap-1" aria-hidden="true">
        <span v-for="code in group.codes.slice(0, 3)" :key="code" class="relative inline-flex" :title="names.of(code) ?? code">
          <img :src="`/flags/${code}.svg`" alt="" width="21" height="14" class="h-3.5 w-[21px] rounded-[2px] ring-1 ring-border/50">
          <Slash v-if="group.excluded" class="absolute -inset-y-0.5 inset-x-0 h-[18px] w-[21px] text-red-600 drop-shadow-[0_0_1px_white]" :stroke-width="3" />
        </span>
        <span v-if="group.codes.length > 3" class="text-[10px] text-muted-foreground">+{{ group.codes.length - 3 }}</span>
      </span>
    </template>
  </span>
</template>
