<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { CheckCircle2, Gift, Layers3, ListTodo, PlayCircle, Sparkles } from 'lucide-vue-next'
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
  { key: 'recommended' as QuestViewPreset, label: t('home.view_recommended'), icon: Sparkles, iconClass: 'text-violet-500' },
  { key: 'to_accept' as QuestViewPreset, label: t('home.view_to_accept'), icon: ListTodo, iconClass: 'text-slate-500' },
  { key: 'ready_to_run' as QuestViewPreset, label: t('home.view_ready_to_run'), icon: PlayCircle, iconClass: 'text-sky-500' },
  { key: 'ready_to_claim' as QuestViewPreset, label: t('home.view_ready_to_claim'), icon: Gift, iconClass: 'text-emerald-500' },
  { key: 'completed' as QuestViewPreset, label: t('home.view_completed'), icon: CheckCircle2, iconClass: 'text-green-500' },
  { key: 'all' as QuestViewPreset, label: t('home.view_all'), icon: Layers3, iconClass: 'text-muted-foreground' },
])
</script>

<template>
  <div class="grid min-w-0 grid-cols-3 gap-2 lg:grid-cols-[repeat(6,minmax(max-content,1fr))]">
    <Button
      v-for="tab in tabs"
      :key="tab.key"
      type="button"
      :variant="selected === tab.key ? 'secondary' : 'ghost'"
      class="h-9 min-w-max w-full justify-start gap-1.5 px-2.5"
      :title="tab.label"
      @click="emit('update:selected', tab.key)"
    >
      <component :is="tab.icon" :class="['h-4 w-4 shrink-0', tab.iconClass]" aria-hidden="true" />
      <span class="shrink-0 whitespace-nowrap">{{ tab.label }}</span>
      <Badge variant="outline" class="ml-auto h-5 min-w-5 shrink-0 justify-center px-1.5 text-[10px]">
        {{ counts[tab.key] }}
      </Badge>
    </Button>
  </div>
</template>
