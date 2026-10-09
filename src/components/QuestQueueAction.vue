<script setup lang="ts">
import { computed } from 'vue'
import { Check, ListPlus, X } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import type { Quest } from '@/api/tauri'
import { Button } from '@/components/ui/button'
import { useQuestsStore } from '@/stores/quests'

const props = defineProps<{ quest: Quest, disabled?: boolean }>()
const { t } = useI18n()
const quests = useQuestsStore()
const queued = computed(() => quests.questQueue.some(item => item.id === props.quest.id))
const controlsDisabled = computed(() => props.disabled || quests.queueEditingDisabled)
const removeLabel = computed(() => t('queue.remove', { name: props.quest.config.messages.quest_name }))
</script>

<template>
  <div class="inline-flex items-center">
    <div v-if="queued" class="inline-flex h-10 items-center overflow-hidden rounded-md border border-emerald-600/25 bg-emerald-500/10 text-emerald-700 dark:border-emerald-400/25 dark:text-emerald-300">
      <span class="inline-flex items-center gap-2 px-3 text-sm font-medium" role="status">
        <Check class="h-4 w-4 shrink-0" aria-hidden="true" />
        {{ t('home.already_queued') }}
      </span>
      <Button
        variant="ghost"
        size="icon"
        class="h-10 w-9 rounded-none border-l border-emerald-600/20 text-emerald-700 hover:bg-red-500/10 hover:text-red-600 dark:border-emerald-400/20 dark:text-emerald-300 dark:hover:text-red-400"
        :title="removeLabel"
        :aria-label="removeLabel"
        :disabled="controlsDisabled"
        @click="quests.removeQueuedQuest(quest.id)"
      >
        <X class="h-4 w-4" aria-hidden="true" />
      </Button>
    </div>
    <Button
      v-else
      class="gap-2 shadow-sm shadow-primary/15 active:scale-[0.98]"
      :disabled="controlsDisabled"
      @click="quests.addToQueueBehindActive(quest)"
    >
      <ListPlus class="h-4 w-4 shrink-0" aria-hidden="true" />
      {{ t('home.add_to_queue') }}
    </Button>
  </div>
</template>
