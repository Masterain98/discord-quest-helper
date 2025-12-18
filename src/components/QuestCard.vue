<script setup lang="ts">
import { computed } from 'vue'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from '@/stores/quests'
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  CardFooter,
} from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { Clock, Gift, MonitorPlay, Gamepad2, Activity } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const props = defineProps<{
  quest: Quest
  questType?: 'video' | 'stream' | 'activity'
}>()

const questsStore = useQuestsStore()

// Check if this quest is currently active
const isActiveQuest = computed(() => questsStore.activeQuestId === props.quest.id)

const targetDuration = computed(() => {
  const taskConfig = props.quest.config.task_config_v2 || props.quest.config.task_config
  if (taskConfig?.tasks) {
    const firstTask = Object.values(taskConfig.tasks)[0]
    return firstTask?.target || 0
  }
  return 0
})

const progress = computed(() => {
  if (props.quest.user_status?.completed_at) return 100
  
  // If this quest is active, use real-time progress from store (already a percentage 0-100)
  if (isActiveQuest.value && questsStore.activeQuestId) {
    return Math.min(100, questsStore.activeQuestProgress)
  }
  
  // Progress is stored as progress[TASK_KEY].value
  const progressObj = props.quest.user_status?.progress
  if (!progressObj || typeof progressObj !== 'object') return 0
  
  // Get the first task's progress value
  const progressValues = Object.values(progressObj as Record<string, { value?: number }>)
  if (progressValues.length > 0 && progressValues[0]?.value !== undefined) {
    const target = targetDuration.value
    if (target > 0) {
      return (progressValues[0].value / target) * 100
    }
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

const statusVariant = computed(() => {
  if (isNotAccepted.value) return 'secondary'
  if (isPendingClaim.value) return 'destructive' // Orange-ish usually, but destructive stands out
  if (isClaimed.value) return 'outline' // Done
  return 'default' // In Progress
})

// In-game rewards with images (rewards that have asset field, but not Discord items)
const inGameRewards = computed(() => {
  const config = props.quest.config.rewards_config
  if (!config || !config.rewards) return []
  
  return config.rewards
    .filter(r => {
      if (!r.asset) return false
      // Exclude Discord items (decorations, orbs, profile effects, etc.)
      const name = (r.messages?.name || '').toLowerCase()
      const isDiscordItem = name.includes('decoration') || 
                            name.includes('avatar') ||
                            name.includes('orb') || 
                            name.includes('profile')
      return !isDiscordItem
    })
    .map(r => ({
      name: r.messages?.name || 'Reward',
      asset: r.asset,
      questId: props.quest.id
    }))
})

// Discord rewards (orbs, decorations, profile effects - with or without assets)
const discordRewards = computed(() => {
  const config = props.quest.config.rewards_config
  if (!config || !config.rewards) return []
  
  return config.rewards
    .filter(r => {
      const name = (r.messages?.name || '').toLowerCase()
      const isDiscordItem = name.includes('decoration') || 
                            name.includes('avatar') ||
                            name.includes('orb') || 
                            name.includes('profile')
      // Include if it's a Discord item, or if it has no asset but has a name
      return isDiscordItem || (!r.asset && r.messages?.name)
    })
    .map(r => ({
      name: r.messages?.name || 'Reward',
      asset: r.asset || null
    }))
})

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60)
  if (minutes >= 60) {
    const hours = Math.floor(minutes / 60)
    const mins = minutes % 60
    return `${hours}h ${mins}m`
  }
  return `${minutes}m`
}

function formatDate(dateStr: string): string {
  if (!dateStr) return 'N/A'
  const date = new Date(dateStr)
  return date.toLocaleDateString()
}
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
            <div class="flex items-center gap-2">
              <Badge :variant="questType === 'video' ? 'default' : 'secondary'" class="mb-1">
                 <MonitorPlay v-if="questType === 'video'" class="w-3 h-3 mr-1" />
                 <Gamepad2 v-else-if="questType === 'stream'" class="w-3 h-3 mr-1" />
                 <Activity v-else class="w-3 h-3 mr-1" />
                 {{ questType === 'video' ? 'Video' : (questType === 'activity' ? 'Activity' : 'Stream/Play') }}
              </Badge>
            </div>
            <CardTitle class="text-xl text-primary">{{ quest.config.messages.quest_name }}</CardTitle>
            <CardDescription>{{ quest.config.messages.game_title }}</CardDescription>
          </div>
        </div>
        <Badge :variant="statusVariant" class="whitespace-nowrap">
           {{ statusLabel }}
        </Badge>
      </div>
    </CardHeader>
    
    <CardContent class="grid gap-4">
      <div class="space-y-2">
        <div class="flex justify-between text-sm">
          <span class="text-muted-foreground">Progress: {{ Math.round(progress) }}%</span>
          <span v-if="targetDuration" class="text-muted-foreground">{{ formatDuration(targetDuration) }} required</span>
        </div>
        <Progress :model-value="progress" class="h-2" />
      </div>
      
      <!-- In-Game Rewards (with images) -->
      <div v-if="inGameRewards.length > 0" class="space-y-2">
        <p class="text-xs text-muted-foreground font-medium">{{ t('quest.in_game_rewards') }}</p>
        <div 
          v-for="reward in inGameRewards" 
          :key="reward.asset"
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
          <span class="text-sm font-medium">{{ reward.name }}</span>
        </div>
      </div>
      
      <!-- Discord Rewards (decorations, orbs etc) -->
      <div v-if="discordRewards.length > 0" class="space-y-2">
        <p class="text-xs text-muted-foreground font-medium">{{ t('quest.discord_rewards') }}</p>
        <div 
          v-for="reward in discordRewards" 
          :key="reward.name"
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
            v-else-if="reward.name.toLowerCase().includes('orb')"
            src="/icons/orbs.png"
            :alt="reward.name"
            class="w-14 h-14 object-contain rounded-md flex-shrink-0"
          />
          <!-- Fallback icon -->
          <Gift v-else class="w-10 h-10 text-pink-400 flex-shrink-0" />
          <span class="text-sm font-medium">{{ reward.name }}</span>
        </div>
      </div>
      
      <div class="grid grid-cols-2 gap-4 text-xs text-muted-foreground">
        <div class="flex items-center gap-1">
          <Clock class="w-3 h-3" />
          Expires: {{ quest.config.expires_at ? formatDate(quest.config.expires_at) : 'N/A' }}
        </div>
         <!-- Target duration handled above -->
      </div>
    </CardContent>

    <CardFooter class="flex gap-2 justify-end pt-2">
      <slot name="actions"></slot>
    </CardFooter>
  </Card>
</template>

