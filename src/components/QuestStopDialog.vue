<script setup lang="ts">
import { computed, watch } from 'vue'
import { ListX, Square } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { useQuestsStore } from '@/stores/quests'
import { Button } from '@/components/ui/button'
import {
  AlertDialog, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog'

const { t } = useI18n()
const quests = useQuestsStore()
const pendingCount = computed(() => quests.upcomingQuests.length)

// A task completing while the dialog is open must not turn confirmation for
// that task into a stop request for the next task.
watch(() => quests.activeQuestId, id => {
  if (quests.stopDialogOpen && id !== quests.stopDialogQuestId) quests.stopDialogOpen = false
})

function confirmStop(scope: 'current' | 'all') {
  if (quests.activeQuestId !== quests.stopDialogQuestId) {
    quests.stopDialogOpen = false
    return
  }
  quests.stopDialogOpen = false
  void quests.stop(scope).catch(error => { console.error('Failed to stop quests:', error) })
}
</script>

<template>
  <AlertDialog v-model:open="quests.stopDialogOpen">
    <AlertDialogContent class="max-h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)] max-w-sm gap-3 overflow-y-auto rounded-lg p-4">
      <AlertDialogHeader class="text-left">
        <AlertDialogTitle class="text-base">{{ t('queue.stop_title') }}</AlertDialogTitle>
        <AlertDialogDescription class="sr-only">{{ t('queue.stop_description') }}</AlertDialogDescription>
      </AlertDialogHeader>

      <div class="grid gap-2">
        <Button
          v-if="quests.activeQuestId"
          variant="outline"
          class="h-auto items-start justify-start gap-2.5 whitespace-normal border-amber-600/25 bg-amber-500/5 p-3 text-left hover:border-amber-600/50 hover:bg-amber-500/10 dark:border-amber-400/25"
          :disabled="quests.stopping"
          @click="confirmStop('current')"
        >
          <Square class="mt-0.5 h-4 w-4 shrink-0 text-amber-700 dark:text-amber-300" aria-hidden="true" />
          <span class="min-w-0 space-y-0.5">
            <span class="block font-semibold text-amber-800 dark:text-amber-200">{{ t('queue.stop_current') }}</span>
            <span class="block text-xs font-normal leading-normal text-muted-foreground">
              {{ pendingCount > 0 ? t('queue.stop_current_continue', { count: pendingCount }) : t('queue.stop_current_only') }}
            </span>
          </span>
        </Button>

        <Button
          v-if="pendingCount > 0"
          variant="outline"
          class="h-auto items-start justify-start gap-2.5 whitespace-normal border-red-600/25 bg-red-500/5 p-3 text-left hover:border-red-600/50 hover:bg-red-500/10 dark:border-red-400/25"
          :disabled="quests.stopping"
          @click="confirmStop('all')"
        >
          <ListX class="mt-0.5 h-4 w-4 shrink-0 text-red-700 dark:text-red-300" aria-hidden="true" />
          <span class="min-w-0 space-y-0.5">
            <span class="block font-semibold text-red-800 dark:text-red-200">{{ t('queue.stop_all') }}</span>
            <span class="block text-xs font-normal leading-normal text-muted-foreground">{{ t('queue.stop_all_description', { count: pendingCount }) }}</span>
          </span>
        </Button>
      </div>

      <AlertDialogFooter>
        <AlertDialogCancel class="mt-0 h-8 w-full px-3 text-xs sm:w-auto">{{ t('dialog.cancel') }}</AlertDialogCancel>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
