<script setup lang="ts">
import { computed } from 'vue'
import { AlertCircle, CheckCircle2, Gift, ListTodo, PlayCircle } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import type { QuestViewPreset } from '@/composables/useHomeQuestState'
import { cn } from '@/lib/utils'

const { t } = useI18n()

const props = defineProps<{
  toAccept: number
  readyToRun: number
  running: number
  readyToClaim: number
  attentionNeeded: number
}>()

const emit = defineEmits<{
  select: [preset: QuestViewPreset]
}>()

const items = computed(() => [
  {
    key: 'to_accept' as QuestViewPreset,
    label: t('home.summary_to_accept'),
    value: props.toAccept,
    icon: ListTodo,
    tone: 'text-slate-600 dark:text-slate-300 bg-slate-500/10 border-slate-500/20',
  },
  {
    key: 'ready_to_run' as QuestViewPreset,
    label: t('home.summary_ready_to_run'),
    value: props.readyToRun,
    icon: PlayCircle,
    tone: 'text-sky-600 dark:text-sky-300 bg-sky-500/10 border-sky-500/20',
  },
  {
    key: 'recommended' as QuestViewPreset,
    label: t('home.summary_running'),
    value: props.running,
    icon: CheckCircle2,
    tone: 'text-violet-600 dark:text-violet-300 bg-violet-500/10 border-violet-500/20',
  },
  {
    key: 'ready_to_claim' as QuestViewPreset,
    label: t('home.summary_ready_to_claim'),
    value: props.readyToClaim,
    icon: Gift,
    tone: 'text-emerald-600 dark:text-emerald-300 bg-emerald-500/10 border-emerald-500/20',
  },
  {
    key: 'recommended' as QuestViewPreset,
    label: t('home.summary_attention'),
    value: props.attentionNeeded,
    icon: AlertCircle,
    tone: 'text-amber-600 dark:text-amber-300 bg-amber-500/10 border-amber-500/20',
  },
])
</script>

<template>
  <div class="flex gap-2 overflow-x-auto pb-1">
    <button
      v-for="item in items"
      :key="item.label"
      type="button"
      :class="cn(
        'flex h-28 w-28 shrink-0 flex-col justify-between rounded-md border p-2.5 text-left transition-colors hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        item.tone,
      )"
      @click="emit('select', item.key)"
    >
      <div class="flex items-start justify-between gap-2">
        <span class="text-[11px] font-medium leading-tight text-muted-foreground">{{ item.label }}</span>
        <component :is="item.icon" class="h-3.5 w-3.5 shrink-0 opacity-80" />
      </div>
      <p class="text-2xl font-semibold leading-none">{{ item.value }}</p>
    </button>
  </div>
</template>
