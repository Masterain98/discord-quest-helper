<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import type { QuestViewPreset } from '@/composables/useHomeQuestState'

const { t } = useI18n()

const props = defineProps<{
  selected: QuestViewPreset
  counts: Record<QuestViewPreset, number>
}>()

const emit = defineEmits<{
  'update:selected': [preset: QuestViewPreset]
}>()

const tabs = computed(() => [
  { key: 'recommended' as QuestViewPreset, label: t('home.view_recommended') },
  { key: 'to_accept' as QuestViewPreset, label: t('home.view_to_accept') },
  { key: 'ready_to_run' as QuestViewPreset, label: t('home.view_ready_to_run') },
  { key: 'ready_to_claim' as QuestViewPreset, label: t('home.view_ready_to_claim') },
  { key: 'completed' as QuestViewPreset, label: t('home.view_completed') },
  { key: 'all' as QuestViewPreset, label: t('home.view_all') },
])
</script>

<template>
  <div class="flex min-w-0 flex-wrap gap-2">
    <Button
      v-for="tab in tabs"
      :key="tab.key"
      type="button"
      :variant="selected === tab.key ? 'secondary' : 'ghost'"
      class="h-9 shrink-0 gap-2 px-3"
      @click="emit('update:selected', tab.key)"
    >
      {{ tab.label }}
      <Badge variant="outline" class="h-5 min-w-5 justify-center px-1.5 text-[10px]">
        {{ counts[tab.key] }}
      </Badge>
    </Button>
  </div>
</template>
