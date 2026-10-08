<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Quest } from '@/api/tauri'
import { getQuestTasks, questTaskBadgeClass } from '@/utils/questTasks'
import { Badge } from '@/components/ui/badge'
import { Activity, Cloud, Gamepad2, MonitorPlay, Smartphone, Trophy } from 'lucide-vue-next'

const props = defineProps<{
  quest: Quest
}>()

const { t } = useI18n()

const tasks = computed(() => getQuestTasks(props.quest))

</script>

<template>
  <div v-if="tasks.length > 0" class="flex flex-wrap gap-1.5">
    <Badge
      v-for="task in tasks"
      :key="task.key"
      variant="outline"
      :class="['gap-1 text-[11px] leading-none ring-1 ring-background/60', questTaskBadgeClass(task.type)]"
      :title="task.key"
    >
      <MonitorPlay v-if="task.type === 'WATCH_VIDEO'" class="h-3 w-3" />
      <Smartphone v-else-if="task.type === 'WATCH_VIDEO_ON_MOBILE'" class="h-3 w-3" />
      <Trophy v-else-if="task.type === 'ACHIEVEMENT_IN_ACTIVITY'" class="h-3 w-3" />
      <Cloud v-else-if="task.type === 'PLAY_ACTIVITY'" class="h-3 w-3" />
      <Gamepad2 v-else-if="task.type.includes('PLAY')" class="h-3 w-3" />
      <Activity v-else class="h-3 w-3" />
      <span>{{ task.type === 'PLAY_ACTIVITY' ? t('filter.activity_cloud_game') : task.label }}</span>
      <span v-if="task.targetText" class="opacity-70">{{ task.targetText }}</span>
    </Badge>
  </div>
</template>
