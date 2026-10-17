<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { Activity, Cloud, Gamepad2, MonitorPlay, Smartphone, Trophy } from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { formatDuration, questTaskBadgeClass } from '@/utils/questTasks'
import type { GlobalQuestTask } from '@/types/globalQuests'

withDefaults(defineProps<{
  tasks: GlobalQuestTask[]
  compact?: boolean
}>(), { compact: false })
const { t } = useI18n()

function taskLabel(type: string) {
  const known = ['WATCH_VIDEO', 'WATCH_VIDEO_ON_MOBILE', 'PLAY_ON_DESKTOP', 'PLAY_ON_XBOX', 'PLAY_ON_PLAYSTATION',
    'STREAM_ON_DESKTOP', 'ACHIEVEMENT_IN_ACTIVITY', 'PLAY_ACTIVITY', 'ACHIEVEMENT_IN_GAME']
  return known.includes(type) ? t(`global_quests.platforms.${type}`) : type.replace(/_/g, ' ')
}

function targetLabel(task: GlobalQuestTask) {
  if (task.target === null) return ''
  return task.type.includes('ACHIEVEMENT') ? t('global_quests.target_count', { count: task.target }) : formatDuration(task.target)
}
</script>

<template>
  <div v-if="tasks.length" class="flex flex-wrap gap-1.5">
    <Badge
      v-for="(task, index) in tasks"
      :key="`${task.type}-${index}`"
      variant="outline"
      :class="[
        'gap-1 ring-1 ring-background/60',
        compact ? 'px-1.5 py-0 text-[10px] font-normal leading-4' : 'text-[11px] leading-none',
        questTaskBadgeClass(task.type),
      ]"
    >
      <MonitorPlay v-if="task.type === 'WATCH_VIDEO'" class="h-3 w-3" />
      <Smartphone v-else-if="task.type === 'WATCH_VIDEO_ON_MOBILE'" class="h-3 w-3" />
      <Trophy v-else-if="task.type === 'ACHIEVEMENT_IN_ACTIVITY' || task.type === 'ACHIEVEMENT_IN_GAME'" class="h-3 w-3" />
      <Cloud v-else-if="task.type === 'PLAY_ACTIVITY'" class="h-3 w-3" />
      <Gamepad2 v-else-if="task.type.includes('PLAY')" class="h-3 w-3" />
      <Activity v-else class="h-3 w-3" />
      <span>{{ taskLabel(task.type) }}</span>
      <span v-if="task.target !== null" class="opacity-70">· {{ targetLabel(task) }}</span>
    </Badge>
  </div>
</template>
