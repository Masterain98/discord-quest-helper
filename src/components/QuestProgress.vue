<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue'
import { useQuestsStore } from '@/stores/quests'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { AlertCircle, RotateCw } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'

const { t } = useI18n()
const questsStore = useQuestsStore()
const authStore = useAuthStore()

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

// Animate the submitted (blue) progress value so it eases forward instead of jumping
const animatedSubmitted = ref(questsStore.activeQuestProgress)
let _raf: number | null = null
watch(() => questsStore.activeQuestProgress, (next) => {
  if (_raf !== null) cancelAnimationFrame(_raf)
  const from = animatedSubmitted.value
  const to = next
  const duration = 450
  const t0 = performance.now()
  const step = (now: number) => {
    const t = Math.min((now - t0) / duration, 1)
    const eased = 1 - Math.pow(1 - t, 3) // ease-out cubic
    animatedSubmitted.value = from + (to - from) * eased
    if (t < 1) _raf = requestAnimationFrame(step)
    else { animatedSubmitted.value = to; _raf = null }
  }
  _raf = requestAnimationFrame(step)
})
onUnmounted(() => { if (_raf !== null) cancelAnimationFrame(_raf) })

// Single-gradient progress bar style: true blue→green color blend
const progressBarStyle = computed(() => {
  const local = questsStore.localProgress
  const submitted = animatedSubmitted.value
  if (local <= 0) return {}
  const junctionPct = Math.round((submitted / local) * 100)
  const stop1 = Math.max(0, junctionPct - 2)
  const stop2 = Math.min(100, junctionPct + 8)
  const hasPending = local > submitted + 0.5
  const bg = !hasPending
    ? 'hsl(var(--primary))'
    : `linear-gradient(to right, hsl(var(--primary)) ${stop1}%, rgb(74,222,128) ${stop2}%, rgb(74,222,128) 100%)`
  return {
    width: `${local}%`,
    background: bg,
    boxShadow: hasPending
      ? '0 0 4px 1px hsl(var(--primary) / 0.6), 0 0 8px 2px hsl(var(--primary) / 0.25), 2px 0 6px 1px rgb(74 222 128 / 0.35)'
      : '0 0 4px 1px hsl(var(--primary) / 0.6), 0 0 8px 2px hsl(var(--primary) / 0.25)',
  }
})
</script>

<template>
  <Card class="sticky top-20 border-border/50">
    <!-- Current Orbs -->
    <div
      v-if="questsStore.showOrbsBalance"
      class="grid grid-cols-[auto_1fr_auto] gap-x-3 items-center px-6 py-3 border-b border-border/50"
    >
      <img src="/icons/orbs.png" alt="" class="h-8 w-8 object-contain row-span-2" />
      <span class="text-xs text-muted-foreground leading-none">{{ t('home.current_orbs') }}</span>
      <Button
        variant="ghost"
        size="icon"
        class="h-7 w-7 shrink-0 row-span-2"
        @click="questsStore.fetchOrbsBalance(true)"
        :disabled="questsStore.orbsBalanceLoading || !authStore.user"
      >
        <RotateCw :class="cn('h-3.5 w-3.5', questsStore.orbsBalanceLoading && 'animate-spin')" />
      </Button>
      <span class="text-base font-bold leading-tight">
        {{ questsStore.orbsBalance == null ? t('home.orbs_not_loaded') : questsStore.orbsBalance.toLocaleString() }}
      </span>
    </div>

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
          
          <!-- Progress Bar: single gradient div, blue→green -->
          <div class="relative h-1.5 w-full rounded-full bg-secondary">
            <div
              class="absolute inset-y-0 left-0 rounded-full transition-all duration-300"
              :style="progressBarStyle"
            ></div>
          </div>
          <div class="flex justify-between text-[10px] text-muted-foreground px-1">
             <div class="flex items-center gap-1">
               <div class="w-2 h-2 rounded-full bg-primary"></div>
               <span>{{ t('quest.submitted') }}</span>
             </div>
             <div class="flex items-center gap-1">
               <div class="w-2 h-2 rounded-full bg-green-400"></div>
               <span>{{ t('quest.pending') }}</span>
             </div>
          </div>
        </div>
        
        <Button 
          variant="destructive" 
          class="w-full"
          @click="handleStop"
        >
          {{ t('home.stop') }}
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
