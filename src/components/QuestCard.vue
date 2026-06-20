<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from '@/stores/quests'
import { useAuthStore } from '@/stores/auth'
import QuestDeveloperDetails from '@/components/QuestDeveloperDetails.vue'
import QuestTaskBadges from '@/components/QuestTaskBadges.vue'
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  CardFooter,
} from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Clock, Gift, MonitorPlay, Gamepad2, Activity, Copy, Check } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { firstProgressValue, firstTargetTask, formatDuration, getQuestKind } from '@/utils/questTasks'
import { getQuestRewardViews, type QuestRewardView } from '@/utils/questRewards'

const { t } = useI18n()

const props = defineProps<{
  quest: Quest
  questType?: 'video' | 'stream' | 'activity'
  showDeveloperDetails?: boolean
}>()

const questsStore = useQuestsStore()
const authStore = useAuthStore()
const copiedQuestId = ref(false)

// Check if this quest is currently active
const isActiveQuest = computed(() => questsStore.activeQuestId === props.quest.id)

const targetDuration = computed(() => {
  // For active quests, use the store's target duration (includes calculated checkpoint times)
  if (isActiveQuest.value && questsStore.activeQuestTargetDuration > 0) {
    return questsStore.activeQuestTargetDuration
  }
  // For activity quests that haven't started, estimate based on checkpoint settings
  const questKind = getQuestKind(props.quest)
  if (questKind === 'activity') {
    const task = firstTargetTask(props.quest)
    const checkpointCount = task?.target || 3
    const avgCheckpoint = (questsStore.activityCheckpointMin + questsStore.activityCheckpointMax) / 2
    return Math.round(checkpointCount * avgCheckpoint)
  }
  return firstTargetTask(props.quest)?.target || 0
})

const progress = computed(() => {
  if (props.quest.user_status?.completed_at) return 100
  
  // If this quest is active, use real-time progress from store (already a percentage 0-100)
  if (isActiveQuest.value && questsStore.activeQuestId) {
    return Math.min(100, questsStore.activeQuestProgress)
  }
  
  const targetTask = firstTargetTask(props.quest)
  const target = targetTask?.target || targetDuration.value
  if (target > 0) {
    return (firstProgressValue(props.quest, targetTask?.key) / target) * 100
  }
  return 0
})

// Status detection
const isNotAccepted = computed(() => !props.quest.user_status?.enrolled_at)
const isCompleted = computed(() => !!props.quest.user_status?.completed_at)
const isPendingClaim = computed(() => isCompleted.value && !props.quest.user_status?.claimed_at)
const isClaimed = computed(() => isCompleted.value && !!props.quest.user_status?.claimed_at)

const statusLabel = computed(() => {
  if (isNotAccepted.value) return t('filter.not_accepted')
  if (isPendingClaim.value) return t('filter.pending_claim')
  if (isClaimed.value) return t('filter.claimed')
  return t('filter.in_progress')
})

const statusClass = computed(() => {
  if (isNotAccepted.value) return 'border-gray-400/60 bg-gray-500/10 text-gray-600 dark:text-gray-400'
  if (isPendingClaim.value) return 'border-orange-400/60 bg-orange-500/10 text-orange-600 dark:text-orange-400'
  if (isClaimed.value) return 'border-green-500/30 bg-green-500/15 text-green-600 dark:text-green-400'
  return 'border-sky-400/60 bg-sky-500/10 text-sky-600 dark:text-sky-400' // In Progress
})

const rewardViews = computed(() => getQuestRewardViews(props.quest, authStore.user?.premium_type))
const inGameRewards = computed(() => rewardViews.value.filter(reward => reward.kind === 'ingame' && reward.asset))
const discordRewards = computed(() => rewardViews.value.filter(reward => reward.kind !== 'ingame' || !reward.asset))

function formatDate(dateStr: string): string {
  if (!dateStr) return 'N/A'
  const date = new Date(dateStr)
  return date.toLocaleDateString()
}

const shortQuestId = computed(() => {
  const id = props.quest.id
  return id.length > 14 ? `${id.slice(0, 6)}...${id.slice(-4)}` : id
})

async function copyQuestId() {
  await navigator.clipboard.writeText(props.quest.id)
  copiedQuestId.value = true
  setTimeout(() => {
    copiedQuestId.value = false
  }, 1500)
}

function rewardKey(reward: QuestRewardView): string {
  return `${reward.skuId}-${reward.type}-${reward.name}`
}

const activeLocalPercent = computed(() => {
  if (isActiveQuest.value) return Math.min(100, questsStore.localProgress)
  return 0
})

// Animate the submitted (blue) progress value so it eases forward instead of jumping
const animatedSubmitted = ref(progress.value)
let _raf: number | null = null
watch(progress, (next) => {
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

// Single-gradient progress bar style: true blue→green color blend, no transparency tricks
const progressBarStyle = computed(() => {
  const local = activeLocalPercent.value
  const submitted = animatedSubmitted.value
  if (local <= 0) return {}
  // Compute gradient stops as % within the bar's own width
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

const activeTimeText = computed(() => {
  if (!isActiveQuest.value) return ''
  const total = targetDuration.value
  const currentSeconds = (questsStore.activeQuestProgress / 100) * total // Use confirmed progress for text? Or pending?
  // Let's match QuestProgress.vue: use confirmed for text 1, total for text 2
  // Format: "MM:ss / MM:ss"
  
  const format = (s: number) => {
     const m = Math.floor(s / 60)
     const sec = Math.floor(s % 60)
     return `${m}:${sec.toString().padStart(2, '0')}`
  }
  return `${format(currentSeconds)} / ${format(total)}`
})
</script>

<template>
  <Card class="mb-4 transition-all hover:shadow-md border-border/50 overflow-hidden">
    <!-- Quest Banner/Hero Image -->
    <div 
      v-if="quest.config.assets?.hero" 
      class="h-24 bg-cover bg-center relative"
      :style="{ backgroundImage: `url(https://cdn.discordapp.com/${quest.config.assets.hero})` }"
    >
      <div class="absolute inset-0 bg-gradient-to-t from-card to-transparent" />
    </div>
    
    <CardHeader class="pb-3">
      <div class="flex justify-between items-start gap-4">
        <div class="flex gap-3 items-start">
          <!-- Application Icon -->
          <img 
            v-if="quest.config.application?.icon"
            :src="`https://cdn.discordapp.com/app-icons/${quest.config.application.id}/${quest.config.application.icon}.png?size=64`"
            :alt="quest.config.application?.name"
            class="w-12 h-12 rounded-lg flex-shrink-0"
          />
          <div class="space-y-1">
            <div class="flex flex-wrap items-center gap-2">
              <Badge
                variant="outline"
                :class="[
                  'mb-1',
                  questType === 'video' && 'border-sky-400/60 bg-sky-500/10 text-sky-600 dark:text-sky-400',
                  questType === 'stream' && 'border-violet-400/60 bg-violet-500/10 text-violet-600 dark:text-violet-400',
                  questType === 'activity' && 'border-amber-400/60 bg-amber-500/10 text-amber-600 dark:text-amber-400',
                ]"
              >
                 <MonitorPlay v-if="questType === 'video'" class="w-3 h-3 mr-1" />
                 <Gamepad2 v-else-if="questType === 'stream'" class="w-3 h-3 mr-1" />
                 <Activity v-else class="w-3 h-3 mr-1" />
                 {{ questType === 'video' ? t('filter.video') : (questType === 'activity' ? t('filter.activity') : t('filter.stream_play')) }}
              </Badge>
              <Badge variant="outline" class="mb-1 max-w-full gap-1 font-mono text-[10px]" :title="quest.id">
                <span class="hidden sm:inline">ID {{ quest.id }}</span>
                <span class="sm:hidden">ID {{ shortQuestId }}</span>
              </Badge>
              <Button
                variant="ghost"
                size="icon"
                class="mb-1 h-6 w-6"
                :title="copiedQuestId ? 'Copied' : 'Copy quest ID'"
                @click="copyQuestId"
              >
                <Check v-if="copiedQuestId" class="h-3 w-3 text-green-500" />
                <Copy v-else class="h-3 w-3" />
              </Button>
            </div>
            <CardTitle class="text-xl text-primary">{{ quest.config.messages.quest_name }}</CardTitle>
            <CardDescription>{{ quest.config.messages.game_title }}</CardDescription>
            <QuestTaskBadges :quest="quest" />
          </div>
        </div>
        <Badge variant="outline" :class="['whitespace-nowrap', statusClass]">
           {{ statusLabel }}
        </Badge>
      </div>
    </CardHeader>
    
    <CardContent class="grid gap-4">
      <div class="space-y-2">
        <div class="flex justify-between text-sm">
          <span class="text-muted-foreground">
            Progress: {{ Math.round(progress) }}%
            <span v-if="isActiveQuest" class="ml-2 font-mono text-xs text-muted-foreground/80">
               ({{ activeTimeText }})
            </span>
          </span>
          <span v-if="targetDuration" class="text-muted-foreground">{{ formatDuration(targetDuration) }} required</span>
        </div>
        
        <!-- Progress Bar for Active Quest: single gradient div, blue→green -->
        <div v-if="isActiveQuest" class="relative h-1.5 w-full rounded-full bg-secondary">
          <div
            class="absolute inset-y-0 left-0 rounded-full transition-all duration-300"
            :style="progressBarStyle"
          ></div>
        </div>
        
        <!-- Standard Progress Bar for others (with glow) -->
        <div v-else class="relative h-1.5 w-full rounded-full bg-secondary">
          <div
            class="absolute inset-y-0 left-0 rounded-full bg-primary transition-all duration-300"
            :style="{
              width: `${progress}%`,
          boxShadow: progress > 0 ? '0 0 4px 1px hsl(var(--primary) / 0.6), 0 0 8px 2px hsl(var(--primary) / 0.25)' : 'none'
            }"
          />
        </div>
      </div>
      
      <!-- In-Game Rewards (with images) -->
      <div v-if="inGameRewards.length > 0" class="space-y-2">
        <p class="text-xs text-muted-foreground font-medium">{{ t('quest.in_game_rewards') }}</p>
        <div 
          v-for="reward in inGameRewards" 
          :key="rewardKey(reward)"
          class="flex items-center gap-3 p-3 rounded-lg bg-gradient-to-r from-muted/40 to-muted/20 border border-border/50"
        >
          <!-- Video asset (.mp4) -->
          <video 
            v-if="reward.asset?.endsWith('.mp4')"
            :src="`https://cdn.discordapp.com/${reward.asset}`"
            class="w-14 h-14 object-contain rounded-md flex-shrink-0"
            autoplay
            loop
            muted
            playsinline
          />
          <!-- Image asset -->
          <img 
            v-else
            :src="`https://cdn.discordapp.com/${reward.asset}`"
            :alt="reward.name"
            class="w-14 h-14 object-contain rounded-md flex-shrink-0"
          />
          <span class="text-sm font-medium">{{ reward.amountText }}</span>
        </div>
      </div>
      
      <!-- Discord Rewards (decorations, orbs etc) -->
      <div v-if="discordRewards.length > 0" class="space-y-2">
        <p class="text-xs text-muted-foreground font-medium">{{ t('quest.discord_rewards') }}</p>
        <div 
          v-for="reward in discordRewards" 
          :key="rewardKey(reward)"
          class="flex items-center gap-3 p-3 rounded-lg bg-gradient-to-r from-muted/40 to-muted/20 border border-border/50"
        >
          <!-- Video asset (Avatar Decoration .mp4) -->
          <video 
            v-if="reward.asset && reward.asset.endsWith('.mp4')"
            :src="`https://cdn.discordapp.com/${reward.asset}`"
            class="w-14 h-14 object-contain rounded-md flex-shrink-0"
            autoplay
            loop
            muted
            playsinline
          />
          <!-- Image asset -->
          <img 
            v-else-if="reward.asset"
            :src="`https://cdn.discordapp.com/${reward.asset}`"
            :alt="reward.name"
            class="w-14 h-14 object-contain rounded-md flex-shrink-0"
          />
          <!-- Orbs reward -->
          <img 
            v-else-if="reward.icon === 'orbs'"
            src="/icons/orbs.png"
            :alt="reward.name"
            class="w-14 h-14 object-contain rounded-md flex-shrink-0"
          />
          <!-- Fallback icon -->
          <Gift v-else class="w-10 h-10 text-pink-400 flex-shrink-0" />
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-sm font-medium">{{ reward.amountText }}</span>
              <Badge v-if="reward.badgeText" variant="secondary" class="text-[10px]">
                {{ reward.badgeText }}
              </Badge>
            </div>
            <div v-if="reward.amountText !== reward.name" class="truncate text-xs text-muted-foreground">
              {{ reward.name }}
            </div>
          </div>
        </div>
      </div>
      
      <div class="grid grid-cols-2 gap-4 text-xs text-muted-foreground">
        <div class="flex items-center gap-1">
          <Clock class="w-3 h-3" />
          Expires: {{ quest.config.expires_at ? formatDate(quest.config.expires_at) : 'N/A' }}
        </div>
         <!-- Target duration handled above -->
      </div>

      <QuestDeveloperDetails v-if="showDeveloperDetails" :quest="quest" />
    </CardContent>

    <CardFooter class="flex gap-2 justify-end pt-2">
      <slot name="actions"></slot>
    </CardFooter>
  </Card>
</template>

