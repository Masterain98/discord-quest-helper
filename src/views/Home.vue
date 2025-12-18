<template>
  <div class="home-view fade-in">
    <!-- Update Available Banner -->
    <div 
      v-if="versionStore.hasUpdate && versionStore.latestRelease" 
      class="mb-6 p-4 bg-primary/10 border border-primary/30 rounded-lg flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3"
    >
      <div class="flex items-center gap-3">
        <div class="w-10 h-10 rounded-full bg-primary/20 flex items-center justify-center">
          <ArrowUpCircle class="w-5 h-5 text-primary" />
        </div>
        <div>
          <p class="font-semibold text-primary">{{ t('version.update_available') }}</p>
          <p class="text-sm text-muted-foreground">
            {{ t('version.update_desc', { version: versionStore.latestRelease.tag_name, current: versionStore.currentVersion }) }}
          </p>
        </div>
      </div>
      <Button @click="openUpdatePage" class="gap-2 shrink-0">
        <ExternalLink class="w-4 h-4" />
        {{ t('version.download') }}
      </Button>
    </div>

    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
      <div class="lg:col-span-2 space-y-6">
        <!-- Toolbar -->
        <div class="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4">
          <h2 class="text-2xl font-bold tracking-tight select-none">{{ t('home.available_quests') }}</h2>
          <div class="flex gap-2 w-full sm:w-auto">
            <Button 
              variant="outline"
              @click="showFilters = !showFilters"
              :class="cn('gap-2 flex-1 sm:flex-none', hasActiveFilters && 'border-primary text-primary')"
            >
              <Filter class="w-4 h-4" />
              <span>{{ t('home.filters') }}</span>
              <Badge v-if="activeFilterCount > 0" variant="secondary" class="ml-1 h-5 px-1.5">
                {{ activeFilterCount }}
              </Badge>
            </Button>
            <Button 
              @click="refreshQuests"
              :disabled="questsStore.loading || !authStore.user"
              class="gap-2 flex-1 sm:flex-none"
            >
              <RotateCw :class="cn('w-4 h-4', questsStore.loading && 'animate-spin')" />
              {{ t('general.refresh') }}
            </Button>
          </div>
        </div>
        
        <!-- Filter Panel -->
        <Card v-if="showFilters" class="animate-in slide-in-from-top-2 duration-200">
          <CardHeader class="pb-3">
            <div class="flex justify-between items-center">
              <CardTitle class="text-base">{{ t('home.filters') }}</CardTitle>
              <Button 
                variant="ghost" 
                size="sm" 
                @click="clearFilters"
                :disabled="!hasActiveFilters"
                class="h-8 text-xs text-muted-foreground hover:text-foreground"
              >
                {{ t('general.clear_all') }}
              </Button>
            </div>
          </CardHeader>
          <CardContent class="space-y-4">
            <!-- Search Input -->
            <div class="relative">
              <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
              <Input 
                v-model="searchQuery" 
                type="text" 
                :placeholder="t('home.search_placeholder')" 
                class="pl-9"
              />
            </div>
            
            <div class="grid gap-6 md:grid-cols-2">
              <!-- Status Filters -->
              <div class="space-y-3">
                <h4 class="text-sm font-medium text-muted-foreground">{{ t('filter.status') }}</h4>
                <div class="flex flex-wrap gap-2">
                  <button 
                    v-for="(label, key) in statusFilterOptions"
                    :key="key"
                    @click="filters.status[key] = !filters.status[key]"
                    :class="cn(
                      'inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full border transition-colors',
                      filters.status[key] 
                        ? 'border-green-600 text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-950/30' 
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    <Check v-if="filters.status[key]" class="h-3.5 w-3.5" />
                    {{ label }}
                  </button>
                </div>
              </div>

              <!-- Type Filters -->
              <div class="space-y-3">
                <h4 class="text-sm font-medium text-muted-foreground">{{ t('filter.type') }}</h4>
                <div class="flex flex-wrap gap-2">
                  <button 
                    @click="filters.questType.watch = !filters.questType.watch"
                    :class="cn(
                      'inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full border transition-colors',
                      filters.questType.watch 
                        ? 'border-green-600 text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-950/30' 
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    <Check v-if="filters.questType.watch" class="h-3.5 w-3.5" />
                    🎬 {{ t('filter.watch') }}
                  </button>
                  <button 
                    @click="filters.questType.play = !filters.questType.play"
                    :class="cn(
                      'inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full border transition-colors',
                      filters.questType.play 
                        ? 'border-green-600 text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-950/30' 
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    <Check v-if="filters.questType.play" class="h-3.5 w-3.5" />
                    🎮 {{ t('filter.play') }}
                  </button>
                  <button 
                    @click="filters.questType.activity = !filters.questType.activity"
                    :class="cn(
                      'inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full border transition-colors',
                      filters.questType.activity 
                        ? 'border-green-600 text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-950/30' 
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    <Check v-if="filters.questType.activity" class="h-3.5 w-3.5" />
                    🎯 {{ t('filter.activity') }}
                  </button>
                </div>
              </div>

              <!-- Reward Filters -->
              <div class="space-y-3 md:col-span-2">
                <h4 class="text-sm font-medium text-muted-foreground">{{ t('filter.reward') }}</h4>
                <div class="flex flex-wrap gap-2">
                  <button 
                    @click="filters.rewards.orbs = !filters.rewards.orbs"
                    :class="cn(
                      'inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full border transition-colors',
                      filters.rewards.orbs 
                        ? 'border-green-600 text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-950/30' 
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    <Check v-if="filters.rewards.orbs" class="h-3.5 w-3.5" />
                    💎 {{ t('filter.orbs') }}
                  </button>
                  <button 
                    @click="filters.rewards.avatarDecoration = !filters.rewards.avatarDecoration"
                    :class="cn(
                      'inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full border transition-colors',
                      filters.rewards.avatarDecoration 
                        ? 'border-green-600 text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-950/30' 
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    <Check v-if="filters.rewards.avatarDecoration" class="h-3.5 w-3.5" />
                    🌟 {{ t('filter.decoration') }}
                  </button>
                  <button 
                    @click="filters.rewards.ingame = !filters.rewards.ingame"
                    :class="cn(
                      'inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full border transition-colors',
                      filters.rewards.ingame 
                        ? 'border-green-600 text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-950/30' 
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    <Check v-if="filters.rewards.ingame" class="h-3.5 w-3.5" />
                    🎁 {{ t('filter.in_game') }}
                  </button>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
        
        <!-- Actions & Queue Status -->
        <div class="flex flex-wrap gap-3 items-center" v-if="!questsStore.loading">
           <Button 
            v-if="unenrolledCount > 0"
            @click="handleAcceptAll"
            class="bg-green-600 hover:bg-green-700 text-white"
          >
            {{ t('home.accept_all') }} ({{ unenrolledCount }})
          </Button>

          <Button 
            v-if="enrolledVideoCount > 0 && !questsStore.isQueueRunning"
            @click="handleCompleteAllVideo"
            variant="secondary"
          >
            {{ t('home.complete_all_video') }} ({{ enrolledVideoCount }})
          </Button>

          <div v-if="questsStore.isQueueRunning" class="flex items-center gap-3 px-4 py-2 bg-secondary/50 rounded-md text-sm border border-secondary">
             <span class="relative flex h-2.5 w-2.5">
              <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-primary opacity-75"></span>
              <span class="relative inline-flex rounded-full h-2.5 w-2.5 bg-primary"></span>
            </span>
             <span class="text-muted-foreground">{{ t('home.processing_queue') }}: {{ questsStore.questQueue.length }} {{ t('home.remaining') }}...</span>
             <Button variant="link" class="h-auto p-0 text-destructive" @click="questsStore.clearQueue">{{ t('general.stop') }}</Button>
          </div>
        </div>
        
        <!-- Content Area -->
        <div v-if="!authStore.user" class="text-center py-12">
           <p class="text-muted-foreground">Please login to view quests</p>
        </div>

        <div v-else-if="questsStore.loading" class="text-center py-12 text-muted-foreground">
          {{ t('general.loading') }}
        </div>
        
        <div v-else-if="questsStore.error" class="p-4 rounded-md bg-destructive/10 text-destructive flex gap-2 items-center">
          <AlertCircle class="w-4 h-4" />
          Error: {{ questsStore.error }}
        </div>
        
        <div v-else-if="filteredQuests.length === 0" class="text-center py-12">
          <p class="text-muted-foreground">No quests match your filters</p>
          <Button variant="link" @click="clearFilters" v-if="hasActiveFilters">Clear Filters</Button>
        </div>
        
        <div v-else class="space-y-4">
          <QuestCard 
            v-for="quest in filteredQuests" 
            :key="quest.id"
            :quest="quest"
            :quest-type="getQuestType(quest)"
          >
            <template #actions>
              <Button 
                v-if="!quest.user_status?.enrolled_at"
                @click="acceptQuest(quest)"
                :disabled="acceptingQuest === quest.id"
              >
                {{ acceptingQuest === quest.id ? 'Accepting...' : 'Accept Quest' }}
              </Button>
              
              <Button 
                v-else-if="questsStore.activeQuestId === quest.id"
                @click="questsStore.stop()"
                variant="destructive"
                :disabled="questsStore.stopping"
              >
                <Loader2 v-if="questsStore.stopping" class="w-4 h-4 mr-2 animate-spin" />
                Stop
              </Button>

              <Button 
                v-else-if="!quest.user_status?.completed_at && canStartQuest(quest)"
                @click="startQuest(quest)"
                variant="default"
                :disabled="questsStore.activeQuestId !== null"
              >
                {{ getStartButtonText(quest) }}
              </Button>
              
              <Button v-else-if="quest.user_status?.completed_at && !quest.user_status?.claimed_at" variant="outline" disabled>
                 Pending Claim
              </Button>
               <span v-else-if="quest.user_status?.completed_at" class="text-sm font-medium text-green-500 self-center px-2">
                ✓ Completed
              </span>
            </template>
          </QuestCard>
        </div>
      </div>
      
      <div class="lg:col-span-1">
        <QuestProgress />
      </div>
    </div>

    <!-- Accept All Confirmation Dialog -->
    <AlertDialog :open="showAcceptAllDialog" @update:open="showAcceptAllDialog = $event">
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{{ t('dialog.accept_quests_title') }}</AlertDialogTitle>
          <AlertDialogDescription>
            {{ t('dialog.accept_quests_desc', { count: pendingAcceptQuests.length }) }}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <AlertDialogAction @click="confirmAcceptAll">{{ t('dialog.accept') }}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <!-- Complete All Confirmation Dialog -->
    <AlertDialog :open="showCompleteAllDialog" @update:open="showCompleteAllDialog = $event">
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{{ t('dialog.complete_quests_title') }}</AlertDialogTitle>
          <AlertDialogDescription>
            {{ t('dialog.complete_quests_desc', { count: pendingCompleteQuests.length }) }}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <AlertDialogAction @click="confirmCompleteAll">{{ t('dialog.start') }}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </div>
</template>


<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'
import QuestCard from '@/components/QuestCard.vue'
import QuestProgress from '@/components/QuestProgress.vue'
import type { Quest } from '@/api/tauri'
import { acceptQuest as acceptQuestApi } from '@/api/tauri'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { cn } from '@/lib/utils'
import { RotateCw, Filter, AlertCircle, Loader2, ArrowUpCircle, ExternalLink, Check, Search } from 'lucide-vue-next'
import { Input } from '@/components/ui/input'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-shell'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const versionStore = useVersionStore()

// Open update page in browser
async function openUpdatePage() {
  if (versionStore.latestRelease?.html_url) {
    await open(versionStore.latestRelease.html_url)
  }
}

const statusFilterOptions = computed(() => ({
  notAccepted: t('filter.not_accepted'),
  inProgress: t('filter.in_progress'),
  pendingClaim: t('filter.pending_claim'),
  completed: t('filter.claimed')
}))

// Show/hide filter panel
const showFilters = ref(false)

// Search query for quest keyword search
const searchQuery = ref('')

// Accepting quest state
const acceptingQuest = ref<string | null>(null)

// Confirmation dialogs state
const showAcceptAllDialog = ref(false)
const showCompleteAllDialog = ref(false)
const pendingAcceptQuests = ref<Quest[]>([])
const pendingCompleteQuests = ref<Quest[]>([])

// localStorage key for filters
const FILTERS_STORAGE_KEY = 'questHelper_filters'

// Load saved filters from localStorage
const defaultFilters = {
  rewards: {
    orbs: false,
    avatarDecoration: false,
    ingame: false
  },
  questType: {
    play: false,
    watch: false,
    activity: false
  },
  status: {
    notAccepted: true,
    inProgress: true,
    pendingClaim: false,
    completed: false
  }
}

const savedFiltersRaw = localStorage.getItem(FILTERS_STORAGE_KEY)
const savedFilters = savedFiltersRaw ? JSON.parse(savedFiltersRaw) : null

// Filter state - use saved or default
const filters = ref(savedFilters || defaultFilters)

// Persist filter changes to localStorage
watch(filters, (newFilters) => {
  localStorage.setItem(FILTERS_STORAGE_KEY, JSON.stringify(newFilters))
}, { deep: true })

const hasActiveFilters = computed(() => {
  return filters.value.rewards.orbs || 
         filters.value.rewards.avatarDecoration || 
         filters.value.rewards.ingame ||
         filters.value.questType.play ||
         filters.value.questType.watch ||
         filters.value.questType.activity ||
         filters.value.status.notAccepted ||
         filters.value.status.inProgress ||
         filters.value.status.pendingClaim ||
         filters.value.status.completed
})

const activeFilterCount = computed(() => {
  let count = 0
  if (filters.value.rewards.orbs) count++
  if (filters.value.rewards.avatarDecoration) count++
  if (filters.value.rewards.ingame) count++
  if (filters.value.questType.play) count++
  if (filters.value.questType.watch) count++
  if (filters.value.questType.activity) count++
  if (filters.value.status.notAccepted) count++
  if (filters.value.status.inProgress) count++
  if (filters.value.status.pendingClaim) count++
  if (filters.value.status.completed) count++
  return count
})

function clearFilters() {
  filters.value.rewards.orbs = false
  filters.value.rewards.avatarDecoration = false
  filters.value.rewards.ingame = false
  filters.value.questType.play = false
  filters.value.questType.watch = false
  filters.value.questType.activity = false
  filters.value.status.notAccepted = false
  filters.value.status.inProgress = false
  filters.value.status.pendingClaim = false
  filters.value.status.completed = false
}

onMounted(() => {
  if (authStore.user) {
    questsStore.fetchQuests()
  }
})

watch(() => authStore.user, (newUser) => {
  if (newUser) {
    questsStore.fetchQuests()
  } else {
    questsStore.quests = []
  }
})


async function refreshQuests() {
  await questsStore.fetchQuests()
}

// Determine quest type based on task_config
function getQuestType(quest: Quest): 'video' | 'stream' | 'activity' {
  const taskConfig = quest.config.task_config_v2 || quest.config.task_config
  if (taskConfig?.tasks) {
    const taskKeys = Object.keys(taskConfig.tasks)
    
    // Activity quests
    if (taskKeys.some(key => key.includes('ACTIVITY') || key.includes('ACHIEVEMENT'))) {
      return 'activity'
    }

    // Check for stream/play task names
    if (taskKeys.some(key => 
      key.includes('STREAM') || 
      key.includes('PLAY')
    )) {
      return 'stream'
    }
  }
  return 'video'
}

// Get button text based on quest type
function getStartButtonText(quest: Quest): string {
  const taskConfig = quest.config.task_config_v2 || quest.config.task_config
  if (!taskConfig?.tasks) return 'Start Quest'
  
  const taskKeys = Object.keys(taskConfig.tasks)
  
  // Video quests
  if (taskKeys.some(key => key.includes('WATCH_VIDEO') || key.includes('VIDEO'))) {
    return '🎬 Start Watching'
  }
  
  // Play quests (game simulation)
  if (taskKeys.some(key => key.includes('PLAY_ON_DESKTOP') || key.includes('PLAY_ON'))) {
    return '🎮 Start Playing'
  }
  
  // Stream quests
  if (taskKeys.some(key => key.includes('STREAM'))) {
    return '📡 Start Streaming'
  }
  
  // Activity quests
  if (taskKeys.some(key => key.includes('ACTIVITY'))) {
    return '🎯 Launch Activity'
  }
  
  return 'Start Quest'
}

// Get reward type for a quest
function getRewardType(quest: Quest): 'orbs' | 'avatar' | 'ingame' {
  const config = quest.config.rewards_config
  if (!config?.rewards) return 'ingame'
  
  const rewardNames = config.rewards.map(r => r.messages?.name?.toLowerCase() || '').join(' ')
  if (rewardNames.includes('orb')) return 'orbs'
  if (rewardNames.includes('avatar') || rewardNames.includes('decoration')) return 'avatar'
  return 'ingame'
}

// Filtered quests based on filter state
const filteredQuests = computed(() => { 
  // Global filter: Hide expired quests (unless claimed)
  let quests = questsStore.quests.filter(q => {
    // Always show if claimed (history)
    if (q.user_status?.claimed_at) return true
    
    // Check expiry
    if (q.config.expires_at) {
      const expires = new Date(q.config.expires_at)
      if (expires < new Date()) return false
    }
    return true
  })
  
  // Apply search filter if search query is provided
  if (searchQuery.value.trim()) {
    const query = searchQuery.value.toLowerCase().trim()
    quests = quests.filter(q => {
      const questName = q.config.messages?.quest_name?.toLowerCase() || ''
      const gameTitle = q.config.messages?.game_title?.toLowerCase() || ''
      return questName.includes(query) || gameTitle.includes(query)
    })
  }

  // If no filters are active, show all valid quests EXCEPT activity (default hidden)
  if (!hasActiveFilters.value) {
    return quests.filter(q => getQuestType(q) !== 'activity')
  }
  
  return quests.filter(quest => {
    // Quest type filter (if any quest type filter is active)
    const questTypeFiltersActive = filters.value.questType.play || filters.value.questType.watch || filters.value.questType.activity
    if (questTypeFiltersActive) {
      const questType = getQuestType(quest)
      const matchPlay = filters.value.questType.play && questType === 'stream'
      const matchWatch = filters.value.questType.watch && questType === 'video'
      const matchActivity = filters.value.questType.activity && questType === 'activity'
      if (!matchPlay && !matchWatch && !matchActivity) return false
    }
    
    // Reward type filter (if any reward filter is active)
    const rewardFiltersActive = filters.value.rewards.orbs || 
                                 filters.value.rewards.avatarDecoration || 
                                 filters.value.rewards.ingame
    if (rewardFiltersActive) {
      const rewardType = getRewardType(quest)
      const matchOrbs = filters.value.rewards.orbs && rewardType === 'orbs'
      const matchAvatar = filters.value.rewards.avatarDecoration && rewardType === 'avatar'
      const matchIngame = filters.value.rewards.ingame && rewardType === 'ingame'
      if (!matchOrbs && !matchAvatar && !matchIngame) return false
    }
    
    // Status filter (if any status filter is active)
    const statusFiltersActive = filters.value.status.notAccepted || 
                                 filters.value.status.inProgress || 
                                 filters.value.status.pendingClaim ||
                                 filters.value.status.completed
    if (statusFiltersActive) {
      const isNotAccepted = !quest.user_status || !quest.user_status.enrolled_at
      const isCompleted = !!quest.user_status?.completed_at
      const isClaimed = isCompleted && !!quest.user_status?.claimed_at
      const isPendingClaim = isCompleted && !quest.user_status?.claimed_at
      const isInProgress = !isNotAccepted && !isCompleted
      
      const matchNotAccepted = filters.value.status.notAccepted && isNotAccepted
      const matchInProgress = filters.value.status.inProgress && isInProgress
      const matchPendingClaim = filters.value.status.pendingClaim && isPendingClaim
      const matchCompleted = filters.value.status.completed && isClaimed
      if (!matchNotAccepted && !matchInProgress && !matchPendingClaim && !matchCompleted) return false
    }
    
    return true
  })
})

const unenrolledCount = computed(() => {
  return filteredQuests.value.filter(q => !q.user_status).length
})

const enrolledVideoCount = computed(() => {
  return filteredQuests.value.filter(q => {
     const isStream = q.config.stream_duration_requirement_minutes && q.config.stream_duration_requirement_minutes > 0
     const isEnrolled = !!q.user_status
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted && !isStream
  }).length
})

function handleAcceptAll() {
  const toAccept = filteredQuests.value.filter(q => !q.user_status)
  if (toAccept.length === 0) return
  pendingAcceptQuests.value = toAccept
  showAcceptAllDialog.value = true
}

async function confirmAcceptAll() {
  await questsStore.acceptAllQuests(pendingAcceptQuests.value.map(q => q.id))
  showAcceptAllDialog.value = false
  pendingAcceptQuests.value = []
}

function handleCompleteAllVideo() {
  const toComplete = filteredQuests.value.filter(q => {
     const isStream = q.config.stream_duration_requirement_minutes && q.config.stream_duration_requirement_minutes > 0
     const isEnrolled = !!q.user_status
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted && !isStream
  })
  
  if (toComplete.length === 0) return
  pendingCompleteQuests.value = toComplete
  showCompleteAllDialog.value = true
}

function confirmCompleteAll() {
  // Add to queue
  pendingCompleteQuests.value.forEach(q => questsStore.addToQueue(q))
  questsStore.startQueue()
  showCompleteAllDialog.value = false
  pendingCompleteQuests.value = []
}

function canStartQuest(quest: Quest): boolean {
  // Check if quest is already completed
  if (quest.user_status?.completed_at) return false
  
  // Can start video or stream quests - check task_config for duration
  const taskConfig = quest.config.task_config_v2 || quest.config.task_config
  if (taskConfig?.tasks) {
    // Check if any task has a target (duration requirement)
    return Object.values(taskConfig.tasks).some(task => task.target !== undefined && task.target > 0)
  }
  return false
}

async function startQuest(quest: Quest) {
  if (questsStore.activeQuestId) return
  
  // Get task config and determine task type
  const taskConfig = quest.config.task_config_v2 || quest.config.task_config
  if (!taskConfig?.tasks) return
  
  const taskKeys = Object.keys(taskConfig.tasks)
  const firstTaskKey = taskKeys[0]
  const firstTask = taskConfig.tasks[firstTaskKey]
  
  if (!firstTask?.target) return
  
  const secondsNeeded = firstTask.target
  
  // Get initial progress from user_status.progress[TASK_KEY].value
  let initialProgress = 0
  const progressObj = quest.user_status?.progress
  if (progressObj && typeof progressObj === 'object') {
    const progressValues = Object.values(progressObj as Record<string, { value?: number }>)
    if (progressValues.length > 0 && progressValues[0]?.value !== undefined) {
      initialProgress = progressValues[0].value
    }
  }
  
  // Detect quest type based on task key
  const isVideoQuest = taskKeys.some(key => 
    key.includes('WATCH_VIDEO') || key.includes('VIDEO')
  )
  const isPlayQuest = taskKeys.some(key => 
    key.includes('PLAY_ON_DESKTOP') || key.includes('PLAY_ON')
  )
  const isStreamQuest = taskKeys.some(key => 
    key.includes('STREAM')
  )
  const isActivityQuest = taskKeys.some(key => 
    key.includes('ACTIVITY') || key.includes('ACHIEVEMENT')
  )
  
  console.log(`Starting quest: type=${firstTaskKey}, target=${secondsNeeded}s, progress=${initialProgress}s`)
  
  if (isVideoQuest) {
    // Video quest - uses video-progress API with timestamp
    await questsStore.startVideo(quest.id, secondsNeeded, initialProgress)
  } else if (isPlayQuest) {
    // Play quests - use Game Simulator logic (one-click)
    const gameName = quest.config.messages.game_title || quest.config.messages.quest_name
    console.log(`Starting play quest for ${gameName}`)
    try {
        await questsStore.startPlay(quest, secondsNeeded, initialProgress)
    } catch (e) {
        alert(`Failed to start game simulator: ${e}\n\nPlease try using the Game Simulator tab manually.`)
    }
  } else if (isStreamQuest) {
    // Stream quests require actually streaming a game
    const gameName = quest.config.messages.game_title || quest.config.messages.quest_name
    alert(`Stream quests require you to stream "${gameName}" on Discord.\n\nPlease:\n1. Start a stream in a Discord voice channel\n2. Use "Game Simulator" to simulate running the game\n3. Discord will track your streaming progress`)
  } else if (isActivityQuest) {
    // Activity quest - needs special handling (Discord Activity)
    alert('Activity quests require launching a Discord Activity. Please complete in Discord client.')
  } else {
    // Unknown type - show warning
    alert(`Unknown quest type: ${firstTaskKey}\n\nPlease check the quest requirements in Discord.`)
  }
}

// Accept Quest handler
async function acceptQuest(quest: Quest) {
  if (acceptingQuest.value) return
  
  try {
    acceptingQuest.value = quest.id
    await acceptQuestApi(quest.id)
    // Update the quest locally without refreshing the entire list
    const now = new Date().toISOString()
    questsStore.updateQuestEnrollment(quest.id, now)
  } catch (error) {
    console.error('Failed to accept quest:', error)
    alert(`Failed to accept quest: ${error}`)
  } finally {
    acceptingQuest.value = null
  }
}
</script>
