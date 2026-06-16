<script setup lang="ts">
import { computed } from 'vue'
import type { Quest } from '@/api/tauri'
import { getQuestTasks } from '@/utils/questTasks'
import { Badge } from '@/components/ui/badge'
import { Activity, Gamepad2, MonitorPlay, Smartphone, Trophy } from 'lucide-vue-next'

const props = defineProps<{
  quest: Quest
}>()

const tasks = computed(() => getQuestTasks(props.quest))

function taskBadgeClass(type: string): string {
  if (type === 'PLAY_ON_XBOX') {
    return 'border-emerald-500/55 bg-emerald-500/10 text-emerald-700 shadow-[0_1px_5px_rgb(16_185_129_/_0.35)] dark:text-emerald-300'
  }
  if (type === 'PLAY_ON_PLAYSTATION') {
    return 'border-blue-500/55 bg-blue-500/10 text-blue-700 shadow-[0_1px_5px_rgb(59_130_246_/_0.35)] dark:text-blue-300'
  }
  if (type === 'PLAY_ON_DESKTOP') {
    return 'border-indigo-500/55 bg-indigo-500/10 text-indigo-700 shadow-[0_1px_5px_rgb(99_102_241_/_0.35)] dark:text-indigo-300'
  }
  if (type === 'WATCH_VIDEO_ON_MOBILE') {
    return 'border-fuchsia-500/55 bg-fuchsia-500/10 text-fuchsia-700 shadow-[0_1px_5px_rgb(217_70_239_/_0.35)] dark:text-fuchsia-300'
  }
  if (type === 'WATCH_VIDEO') {
    return 'border-sky-500/55 bg-sky-500/10 text-sky-700 shadow-[0_1px_5px_rgb(14_165_233_/_0.35)] dark:text-sky-300'
  }
  if (type === 'ACHIEVEMENT_IN_ACTIVITY') {
    return 'border-amber-500/55 bg-amber-500/10 text-amber-700 shadow-[0_1px_5px_rgb(245_158_11_/_0.35)] dark:text-amber-300'
  }
  return 'border-border bg-background text-foreground shadow-[0_1px_4px_rgb(0_0_0_/_0.18)]'
}
</script>

<template>
  <div v-if="tasks.length > 0" class="flex flex-wrap gap-1.5">
    <Badge
      v-for="task in tasks"
      :key="task.key"
      variant="outline"
      :class="['gap-1 text-[11px] leading-none ring-1 ring-background/60', taskBadgeClass(task.type)]"
      :title="task.key"
    >
      <MonitorPlay v-if="task.type === 'WATCH_VIDEO'" class="h-3 w-3" />
      <Smartphone v-else-if="task.type === 'WATCH_VIDEO_ON_MOBILE'" class="h-3 w-3" />
      <Trophy v-else-if="task.type === 'ACHIEVEMENT_IN_ACTIVITY'" class="h-3 w-3" />
      <Gamepad2 v-else-if="task.type.includes('PLAY')" class="h-3 w-3" />
      <Activity v-else class="h-3 w-3" />
      <span>{{ task.label }}</span>
      <span v-if="task.targetText" class="opacity-70">{{ task.targetText }}</span>
    </Badge>
  </div>
</template>
