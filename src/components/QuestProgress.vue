<script setup lang="ts">
import { computed } from 'vue'
import { useQuestsStore } from '@/stores/quests'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { AlertCircle } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const questsStore = useQuestsStore()

// Local progress is now managed by the store

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = Math.floor(seconds % 60)
  return `${m}:${s.toString().padStart(2, '0')}`
}

const submittedTimeText = computed(() => {
  const total = questsStore.activeQuestTargetDuration
  const progress = questsStore.activeQuestProgress
  const currentSeconds = (progress / 100) * total
  return `${formatTime(currentSeconds)} / ${formatTime(total)}`
})

async function handleStop() {
  await questsStore.stop()
}
</script>

<template>
  <Card class="sticky top-20 border-border/50">
    <CardHeader>
      <CardTitle class="text-lg">{{ t('quest.active_progress') }}</CardTitle>
    </CardHeader>
    <CardContent>
      <div v-if="questsStore.activeQuestId" class="space-y-4">
        <div class="space-y-2">
          <div class="flex justify-between text-sm items-end">
             <div class="flex flex-col">
               <span class="text-muted-foreground truncate max-w-[150px] text-xs" :title="questsStore.activeQuestId">
                 ID: {{ questsStore.activeQuestId.substring(0, 8) }}...
               </span>
               <span class="font-mono text-xs text-muted-foreground">
                 {{ submittedTimeText }}
               </span>
             </div>
             <span class="font-medium text-lg">{{ Math.floor(questsStore.activeQuestProgress) }}%</span>
          </div>
          
          <!-- Dual Layer Progress Bar -->
          <div class="relative h-3 w-full overflow-hidden rounded-full bg-secondary">
             <!-- Layer 1: Local Accumulated (Green) -->
             <div 
               class="absolute h-full bg-green-500/50 transition-all duration-300 ease-linear"
               :style="{ width: `${questsStore.localProgress}%` }"
             ></div>
             <!-- Layer 2: Submitted (Blue) -->
             <div 
               class="absolute h-full bg-primary transition-all duration-300 ease-in-out"
               :style="{ width: `${questsStore.activeQuestProgress}%` }"
             ></div>
          </div>
          <div class="flex justify-between text-[10px] text-muted-foreground px-1">
             <div class="flex items-center gap-1">
               <div class="w-2 h-2 rounded-full bg-primary"></div>
               <span>{{ t('quest.submitted') }}</span>
             </div>
             <div class="flex items-center gap-1">
               <div class="w-2 h-2 rounded-full bg-green-500/50"></div>
               <span>{{ t('quest.pending') }}</span>
             </div>
          </div>
        </div>
        
        <Button 
          variant="destructive" 
          class="w-full"
          @click="handleStop"
        >
          {{ t('home.stop_quest') }}
        </Button>
      </div>
      
      <div v-else class="text-center py-6 text-muted-foreground">
        {{ t('quest.no_active') }}
      </div>
      
      <div v-if="questsStore.questQueue.length > 0" class="mt-6 pt-4 border-t">
        <div class="flex justify-between items-center mb-2">
          <h4 class="font-semibold text-sm">{{ t('quest.up_next') }} ({{ questsStore.questQueue.length }})</h4>
          <Button 
            variant="ghost" 
            size="sm"
            class="h-6 px-2 text-destructive hover:text-destructive"
            @click="questsStore.clearQueue"
          >
            {{ t('general.clear') }}
          </Button>
        </div>
        
        <div class="space-y-2 max-h-[300px] overflow-y-auto pr-1">
          <div 
            v-for="(quest, index) in questsStore.questQueue" 
            :key="quest.id"
            class="bg-muted/50 p-2 rounded text-sm flex gap-2 items-center"
          >
            <span class="text-muted-foreground font-mono text-xs w-4">{{ index + 1 }}.</span>
            <div class="flex-1 overflow-hidden">
               <div class="truncate font-medium">{{ quest.config.messages.quest_name }}</div>
               <div class="text-xs text-muted-foreground truncate">{{ quest.config.messages.game_title }}</div>
            </div>
          </div>
        </div>
      </div>
  
      <div v-if="questsStore.error" class="mt-4 p-3 bg-red-500/10 border border-red-500/20 rounded text-sm text-red-500 flex items-start gap-2">
        <AlertCircle class="w-4 h-4 mt-0.5 shrink-0" />
        <span class="break-words">{{ questsStore.error }}</span>
      </div>
    </CardContent>
  </Card>
</template>
