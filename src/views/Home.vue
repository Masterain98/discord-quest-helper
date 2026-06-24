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

    <div class="space-y-6">
      <div class="space-y-6">
        <div class="space-y-2">
          <h2 class="text-2xl font-bold tracking-tight select-none">{{ t('home.dashboard_title') }}</h2>
          <p class="text-sm text-muted-foreground">{{ t('home.dashboard_desc') }}</p>
        </div>

        <HomeStatusSummary
          :to-accept="questBuckets.toAccept.length"
          :ready-to-run="questBuckets.readyToRun.length"
          :running="runningCount"
          :ready-to-claim="questBuckets.readyToClaim.length"
          :attention-needed="questBuckets.attentionNeeded.length"
          @select="selectPreset"
        />

        <NextBestActionPanel
          :state="nextBestAction.state"
          :title="nextBestAction.title"
          :description="nextBestAction.description"
          :primary-label="nextBestAction.primaryLabel"
          :secondary-label="nextBestAction.secondaryLabel"
          :primary-disabled="nextBestAction.primaryDisabled"
          :secondary-disabled="nextBestAction.secondaryDisabled"
          :busy="nextActionBusy"
          @primary="handleNextPrimary"
          @secondary="handleNextSecondary"
        />

        <QuestViewTabs
          :selected="selectedPreset"
          :counts="viewCounts"
          @update:selected="selectPreset"
        />

        <QuestListHeader
          v-model:query="searchQuery"
          :result-count="filteredQuests.length"
          :active-filter-count="activeFilterCount"
          :show-filters="showFilters"
          :loading="questsStore.loading"
          :refresh-disabled="questsStore.loading || !authStore.user || isBatchAccepting"
          :batch-disabled="isBatchAccepting || questsStore.isQueueRunning"
          :accept-count="unenrolledCount"
          :complete-all-count="enrolledAllCount"
          :video-count="enrolledVideoCount"
          :game-count="enrolledGameCount"
          @toggle-filters="showFilters = !showFilters"
          @refresh="refreshQuests"
          @accept-all="handleAcceptAll"
          @complete-all="handleCompleteAllTasks"
          @complete-video="handleCompleteAllVideo"
          @complete-game="handleCompleteAllGame"
        />

        <Card v-if="showFilters" class="animate-in slide-in-from-top-2 duration-200">
          <CardHeader class="pb-3">
            <div class="flex justify-between items-center gap-3">
              <CardTitle class="text-base">{{ t('home.advanced_filters') }}</CardTitle>
              <Button
                variant="ghost"
                size="sm"
                @click="clearFilters"
                :disabled="!hasActiveFilters"
                class="h-8 text-xs text-muted-foreground hover:text-foreground"
              >
                {{ t('home.reset_advanced_filters') }}
              </Button>
            </div>
          </CardHeader>
          <CardContent class="space-y-5">
            <div class="grid gap-6 md:grid-cols-2">
              <div class="space-y-3">
                <h4 class="text-sm font-medium text-muted-foreground">{{ t('filter.type') }}</h4>
                <div class="flex flex-wrap gap-2">
                  <button
                    @click="advancedFilters.types.video = !advancedFilters.types.video"
                    :class="cn(
                      'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors',
                      advancedFilters.types.video
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    🎬 {{ t('filter.video') }}
                  </button>
                  <button
                    @click="advancedFilters.types.play = !advancedFilters.types.play"
                    :class="cn(
                      'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors',
                      advancedFilters.types.play
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    🎮 {{ t('filter.play') }}
                  </button>
                  <button
                    @click="advancedFilters.types.activity = !advancedFilters.types.activity"
                    :class="cn(
                      'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors',
                      advancedFilters.types.activity
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    🎯 {{ t('filter.activity') }}
                  </button>
                </div>
              </div>

              <div class="space-y-3">
                <h4 class="text-sm font-medium text-muted-foreground">{{ t('filter.reward') }}</h4>
                <div class="flex flex-wrap gap-2">
                  <button
                    @click="advancedFilters.rewards.orbs = !advancedFilters.rewards.orbs"
                    :class="cn(
                      'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors',
                      advancedFilters.rewards.orbs
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    💎 {{ t('filter.orbs') }}
                  </button>
                  <button
                    @click="advancedFilters.rewards.avatarDecoration = !advancedFilters.rewards.avatarDecoration"
                    :class="cn(
                      'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors',
                      advancedFilters.rewards.avatarDecoration
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    🌟 {{ t('filter.decoration') }}
                  </button>
                  <button
                    @click="advancedFilters.rewards.ingame = !advancedFilters.rewards.ingame"
                    :class="cn(
                      'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors',
                      advancedFilters.rewards.ingame
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-background hover:bg-accent hover:text-accent-foreground'
                    )"
                  >
                    🎁 {{ t('filter.in_game') }}
                  </button>
                </div>
              </div>
            </div>

            <label class="flex items-start gap-3 rounded-lg border border-border p-3">
              <input
                v-model="advancedFilters.includeExpired"
                type="checkbox"
                class="mt-1 h-4 w-4 accent-primary"
              />
              <span class="min-w-0 flex-1">
                <span class="text-sm font-medium">{{ t('home.include_expired') }}</span>
                <span class="mt-1 block text-xs text-muted-foreground">
                  {{ t('home.include_expired_desc') }}
                </span>
              </span>
            </label>
          </CardContent>
        </Card>

        <div v-if="!authStore.user" class="text-center py-12">
           <p class="text-muted-foreground">{{ t('general.login_prompt') }}</p>
        </div>

        <div v-else-if="questsStore.loading" class="text-center py-12 text-muted-foreground">
          {{ t('general.loading') }}
        </div>
        
        <div v-else-if="filteredQuests.length === 0" class="rounded-lg border border-dashed p-8 text-center">
          <p class="font-medium">{{ emptyStateText }}</p>
          <div class="mt-3 flex justify-center gap-2">
            <Button v-if="hasActiveFilters" variant="outline" @click="clearFilters">
              {{ t('home.reset_filters') }}
            </Button>
            <Button v-else-if="selectedPreset !== 'recommended'" variant="outline" @click="backToRecommended">
              {{ t('home.back_to_recommended') }}
            </Button>
            <Button variant="ghost" @click="refreshQuests">
              {{ t('general.refresh') }}
            </Button>
          </div>
        </div>

        <template v-else>
          <div
            v-if="showPendingClaimBanner"
            class="flex items-center justify-between gap-3 rounded-lg border border-yellow-500/30 bg-yellow-500/10 p-3"
          >
            <div class="flex items-center gap-2 text-sm text-yellow-700 dark:text-yellow-300">
              <Gift class="w-4 h-4 shrink-0" />
              <span>{{ t('home.pending_claim_reminder', { count: pendingClaimCount }) }}</span>
            </div>
            <Button size="sm" variant="outline" class="shrink-0 border-yellow-500/40 text-yellow-700 hover:bg-yellow-500/10 dark:text-yellow-300" @click="showPendingClaimFilter">
              {{ t('home.view_pending_claim') }}
            </Button>
          </div>

          <TransitionGroup name="quest-list" tag="div" class="space-y-3">
            <QuestCard
              v-for="quest in filteredQuests"
              :key="quest.id"
              :quest="quest"
              :quest-type="getQuestType(quest)"
              :show-developer-details="props.debugModeEnabled"
              density="compact"
            >
              <template #actions>
                <Button
                  v-if="!quest.user_status?.enrolled_at"
                  @click="acceptQuest(quest)"
                  :disabled="acceptingQuest === quest.id || acceptingAllQuestIds.has(quest.id)"
                >
                  {{ (acceptingQuest === quest.id || acceptingAllQuestIds.has(quest.id)) ? t('home.accepting') : t('home.accept_quest') }}
                </Button>

                <Button
                  v-else-if="questsStore.activeQuestId === quest.id"
                  @click="questsStore.stop()"
                  variant="destructive"
                  :disabled="questsStore.stopping || isBatchAccepting"
                >
                  <Loader2 v-if="questsStore.stopping" class="w-4 h-4 mr-2 animate-spin" />
                  {{ t('home.stop') }}
                </Button>

                <Button
                  v-else-if="!quest.user_status?.completed_at && canStartQuest(quest)"
                  @click="startQuest(quest)"
                  variant="default"
                  :disabled="questsStore.activeQuestId !== null || startingQuestId !== null || isBatchAccepting"
                >
                  <Loader2 v-if="startingQuestId === quest.id" class="w-4 h-4 mr-2 animate-spin" />
                  {{ getStartButtonText(quest) }}
                </Button>

                <div v-else-if="quest.user_status?.completed_at && !quest.user_status?.claimed_at" class="relative -mt-1 mb-1">
                  <Button
                    :disabled="claimingQuest === quest.id || isBatchAccepting"
                    class="bg-green-600 hover:bg-green-700 text-white gap-1.5"
                    @click="claimReward(quest)"
                  >
                    <Loader2 v-if="claimingQuest === quest.id" class="w-4 h-4 animate-spin" />
                    <Gift v-else class="w-4 h-4" />
                    {{ t('home.claim_reward') }}
                  </Button>
                  <span
                    v-if="claimedNoticeQuestId === quest.id"
                    class="absolute top-full right-0 mt-1.5 whitespace-nowrap text-[11px] text-green-600 dark:text-green-400 notice-fade"
                  >
                    {{ t('home.claim_navigated') }}
                  </span>
                </div>
                 <span v-else-if="quest.user_status?.completed_at" class="self-center px-2 text-sm font-medium text-green-500">
                  {{ t('home.completed') }}
                </span>
              </template>
            </QuestCard>
          </TransitionGroup>
        </template>
      </div>
    </div>

    <QuestProgress />

    <!-- Accept All Confirmation Dialog -->
    <AlertDialog :open="showAcceptAllDialog" @update:open="showAcceptAllDialog = $event">
      <AlertDialogContent class="max-w-[600px]">
        <AlertDialogHeader>
          <AlertDialogTitle>{{ t('dialog.accept_quests_title') }}</AlertDialogTitle>
          <AlertDialogDescription>
            <div class="space-y-4 my-4">
              <p>{{ t('dialog.accept_quests_desc', { count: pendingAcceptQuests.length }) }}</p>
              <div class="border rounded-md p-3 bg-secondary/20 max-h-[300px] overflow-y-auto space-y-2 text-xs">
                <div v-for="quest in pendingAcceptQuests" :key="quest.id" class="flex justify-between items-center gap-4">
                  <span class="font-medium truncate flex-1">{{ quest.config.messages.quest_name }}</span>
                  <span class="text-muted-foreground whitespace-nowrap font-mono">
                    {{ quest.config.expires_at ? new Date(quest.config.expires_at).toLocaleString() : t('quest.no_expiry') }}
                  </span>
                </div>
              </div>
            </div>
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
      <AlertDialogContent class="max-w-[600px]">
        <AlertDialogHeader>
          <AlertDialogTitle>{{ t('dialog.complete_quests_title') }}</AlertDialogTitle>
          <AlertDialogDescription>
            <div class="space-y-4 my-4">
              <p>{{ t('dialog.complete_quests_desc', { count: pendingCompleteQuests.length }) }}</p>
              <div class="border rounded-md p-3 bg-secondary/20 max-h-[300px] overflow-y-auto space-y-2 text-xs">
                 <div v-for="q in pendingCompleteQuests" :key="q.id" class="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1">
                    <span class="font-medium truncate text-foreground">{{ q.config.messages.quest_name }}</span>
                    <span :class="cn('font-mono', getExpiryColor(q.config.expires_at))">
                      {{ getExpiryText(q.config.expires_at) }}
                    </span>
                    <span class="text-xs text-muted-foreground col-span-2 truncate">
                      {{ q.config.messages.game_title }}
                      <template v-if="props.debugModeEnabled"> • ID: {{ q.id }}</template>
                    </span>
                 </div>
              </div>
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <AlertDialogAction @click="confirmCompleteAll">{{ t('dialog.start') }}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <!-- Exe Selection Dialog for Play Quests with multiple win32 executables -->
    <Dialog :open="showExeSelectDialog" @update:open="showExeSelectDialog = $event">
      <DialogContent class="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{{ t('home.exe_select_title') }}</DialogTitle>
          <DialogDescription>
            {{ t('home.exe_select_desc', { game: exeSelectGameName }) }}
          </DialogDescription>
        </DialogHeader>
        <div class="flex flex-col gap-2 max-h-64 overflow-y-auto py-2">
          <Button
            v-for="exe in exeSelectOptions"
            :key="exe"
            variant="outline"
            class="justify-start font-mono text-sm h-auto py-3 px-4"
            @click="selectExeAndStartPlay(exe)"
          >
            {{ exe }}
          </Button>
        </div>
        <DialogFooter>
          <Button variant="ghost" @click="showExeSelectDialog = false">{{ t('dialog.cancel') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- Custom Exe Input Dialog for Play Quests with NO known executables -->
    <Dialog :open="showCustomExeDialog" @update:open="showCustomExeDialog = $event">
      <DialogContent class="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{{ t('home.custom_exe_title') }}</DialogTitle>
          <DialogDescription>
            {{ t('home.custom_exe_desc', { game: customExeGameName }) }}
          </DialogDescription>
        </DialogHeader>
        <div class="space-y-4 py-2">
          <div class="p-3 bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 rounded-md text-sm border border-yellow-500/20 space-y-1">
            <p>{{ t('game_sim.no_exe_hint') }}</p>
            <p>{{ t('game_sim.no_exe_custom_warning') }}</p>
          </div>
          <div class="space-y-2">
            <Input
              v-model="customExeInput"
              :placeholder="t('game_sim.custom_exe_placeholder')"
              @keyup.enter="submitCustomExeAndStartPlay"
            />
            <p class="text-xs text-muted-foreground">{{ t('game_sim.custom_exe_hint') }}</p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" @click="cancelCustomExeDialog">{{ t('dialog.cancel') }}</Button>
          <Button @click="submitCustomExeAndStartPlay" :disabled="!customExeInput.trim()">{{ t('dialog.confirm') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- Activity Quest Launch Dialog -->
    <Dialog :open="showActivityLaunchDialog" @update:open="showActivityLaunchDialog = $event">
      <DialogContent class="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{{ t('home.activity_launch_title') }}</DialogTitle>
          <DialogDescription>
            {{ t('home.activity_launch_desc') }}
          </DialogDescription>
        </DialogHeader>
        <div class="space-y-4 py-2">
          <div class="space-y-3 text-sm">
            <div class="flex items-start gap-3">
              <div class="flex-shrink-0 w-6 h-6 rounded-full bg-primary/20 text-primary flex items-center justify-center text-xs font-medium">1</div>
              <p>{{ t('home.activity_launch_step1') }}</p>
            </div>
            <div class="flex items-start gap-3">
              <div class="flex-shrink-0 w-6 h-6 rounded-full bg-primary/20 text-primary flex items-center justify-center text-xs font-medium">2</div>
              <p>{{ t('home.activity_launch_step2') }}</p>
            </div>
            <div class="flex items-start gap-3">
              <div class="flex-shrink-0 w-6 h-6 rounded-full bg-primary/20 text-primary flex items-center justify-center text-xs font-medium">3</div>
              <p>{{ t('home.activity_launch_step3') }}</p>
            </div>
            <div class="flex items-start gap-3">
              <div class="flex-shrink-0 w-6 h-6 rounded-full bg-primary/20 text-primary flex items-center justify-center text-xs font-medium">4</div>
              <p>{{ t('home.activity_launch_step4') }}</p>
            </div>
          </div>
          <div v-if="activityLaunchError" class="p-3 bg-destructive/10 text-destructive rounded-md text-sm border border-destructive/20">
            {{ activityLaunchError }}
          </div>
        </div>
        <DialogFooter class="flex gap-2">
          <Button
            variant="outline"
            @click="navigateActivityQuestInDiscord"
            :disabled="activityNavigatingToDiscord"
          >
            <Loader2 v-if="activityNavigatingToDiscord" class="mr-2 h-4 w-4 animate-spin" />
            <ExternalLink v-else class="mr-2 h-4 w-4" />
            {{ t('home.activity_launch_navigate') }}
          </Button>
          <Button variant="ghost" @click="cancelActivityLaunch">{{ t('home.activity_launch_cancel') }}</Button>
          <Button @click="confirmActivityLaunch">
            {{ t('home.activity_launch_start') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- Batch Complete Confirmation Dialog -->
    <AlertDialog :open="showBatchCompleteDialog" @update:open="showBatchCompleteDialog = $event">
      <AlertDialogContent class="max-w-[600px]">
        <AlertDialogHeader>
          <AlertDialogTitle>
            {{ batchCompleteType === 'game' ? t('dialog.complete_game_title') : t('dialog.complete_all_title') }}
          </AlertDialogTitle>
          <AlertDialogDescription>
            <div class="space-y-4 my-4">
              <p>
                {{ batchCompleteType === 'game'
                  ? t('dialog.complete_game_desc', { count: pendingBatchCompleteQuests.length })
                  : t('dialog.complete_all_desc', { count: pendingBatchCompleteQuests.length })
                }}
              </p>
              <p v-if="batchCompleteType === 'all'" class="text-xs text-muted-foreground italic">
                {{ t('dialog.activity_excluded_notice') }}
              </p>
              <div class="border rounded-md p-3 bg-secondary/20 max-h-[300px] overflow-y-auto space-y-2 text-xs">
                 <div v-for="q in pendingBatchCompleteQuests" :key="q.id" class="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1">
                    <span class="font-medium truncate text-foreground">{{ q.config.messages.quest_name }}</span>
                    <span :class="cn('font-mono', getExpiryColor(q.config.expires_at))">
                      {{ getExpiryText(q.config.expires_at) }}
                    </span>
                    <span class="text-xs text-muted-foreground col-span-2 truncate">
                      {{ q.config.messages.game_title }} • {{ getQuestType(q) === 'video' ? t('filter.video') : t('filter.play') }}
                      <template v-if="props.debugModeEnabled"> • ID: {{ q.id }}</template>
                    </span>
                 </div>
              </div>
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <AlertDialogAction @click="confirmBatchComplete">{{ t('dialog.start') }}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <!-- Batch Exe Selection Dialog -->
    <Dialog :open="showBatchExeSelectDialog" @update:open="cancelBatchExeSelect">
      <DialogContent class="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{{ t('home.exe_select_title') }}</DialogTitle>
          <DialogDescription>
            {{ t('home.exe_select_desc', { game: batchExeSelectGameName }) }}
          </DialogDescription>
        </DialogHeader>
        <div class="flex flex-col gap-2 max-h-64 overflow-y-auto py-2">
          <Button
            v-for="exe in batchExeSelectOptions"
            :key="exe"
            variant="outline"
            class="justify-start font-mono text-sm"
            @click="selectBatchExe(exe)"
          >
            {{ exe }}
          </Button>
        </div>
        <DialogFooter>
          <Button variant="ghost" @click="cancelBatchExeSelect">{{ t('dialog.cancel') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>


<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'
import HomeStatusSummary from '@/components/home/HomeStatusSummary.vue'
import NextBestActionPanel, { type NextBestActionState } from '@/components/home/NextBestActionPanel.vue'
import QuestListHeader from '@/components/home/QuestListHeader.vue'
import QuestViewTabs from '@/components/home/QuestViewTabs.vue'
import QuestCard from '@/components/QuestCard.vue'
import QuestProgress from '@/components/QuestProgress.vue'
import type { Quest } from '@/api/tauri'
import {
  acceptQuest as acceptQuestApi,
  claimQuestReward,
  navigateDiscordSpa,
} from '@/api/tauri'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { ArrowUpCircle, ExternalLink, Gift, Loader2 } from 'lucide-vue-next'
import { Input } from '@/components/ui/input'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-shell'
import {
  firstProgressValue,
  firstStartableTask,
  getQuestKind,
  getQuestTasks,
  isActivityTask,
  isDesktopPlayTask,
  isStreamTask,
  isVideoTask,
} from '@/utils/questTasks'
import { getQuestRewardCategory } from '@/utils/questRewards'
import { useToastStore } from '@/stores/toast'
import { navigateToTab } from '@/utils/navigate'
import {
  emptyAdvancedQuestFilters,
  getRecommendedQuests,
  isQuestExpired,
  type AdvancedQuestFilters,
  type QuestViewPreset,
  useHomeQuestState,
} from '@/composables/useHomeQuestState'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const versionStore = useVersionStore()
const toast = useToastStore()

const props = defineProps<{
  debugModeEnabled?: boolean
}>()

// Open update page in browser
async function openUpdatePage() {
  if (versionStore.latestRelease?.html_url) {
    await open(versionStore.latestRelease.html_url)
  }
}

const VIEW_PRESET_STORAGE_KEY = 'questHelper_viewPreset'
const ADVANCED_FILTERS_STORAGE_KEY = 'questHelper_advancedFilters'
const viewPresetOptions: QuestViewPreset[] = ['recommended', 'to_accept', 'ready_to_run', 'ready_to_claim', 'completed', 'all']

function readSavedPreset(): QuestViewPreset {
  const savedPreset = localStorage.getItem(VIEW_PRESET_STORAGE_KEY)
  return viewPresetOptions.includes(savedPreset as QuestViewPreset) ? savedPreset as QuestViewPreset : 'recommended'
}

function readSavedAdvancedFilters(): AdvancedQuestFilters {
  const defaults = emptyAdvancedQuestFilters()
  const savedRaw = localStorage.getItem(ADVANCED_FILTERS_STORAGE_KEY)
  if (!savedRaw) return defaults

  try {
    const saved = JSON.parse(savedRaw) as Partial<AdvancedQuestFilters>
    return {
      query: typeof saved.query === 'string' ? saved.query : defaults.query,
      types: { ...defaults.types, ...saved.types },
      rewards: { ...defaults.rewards, ...saved.rewards },
      includeExpired: saved.includeExpired ?? defaults.includeExpired,
    }
  } catch {
    return defaults
  }
}

const selectedPreset = ref<QuestViewPreset>(readSavedPreset())
const advancedFilters = ref<AdvancedQuestFilters>(readSavedAdvancedFilters())
const showFilters = ref(false)

const searchQuery = computed({
  get: () => advancedFilters.value.query,
  set: (value: string) => {
    advancedFilters.value.query = value
  },
})

watch(selectedPreset, (preset) => {
  localStorage.setItem(VIEW_PRESET_STORAGE_KEY, preset)
})

watch(advancedFilters, (newFilters) => {
  localStorage.setItem(ADVANCED_FILTERS_STORAGE_KEY, JSON.stringify(newFilters))
}, { deep: true })

// Accepting quest state
const acceptingQuest = ref<string | null>(null)
const claimingQuest = ref<string | null>(null)
const claimedNoticeQuestId = ref<string | null>(null)
let claimedNoticeTimer: ReturnType<typeof setTimeout> | null = null

// Batch accept state — tracks IDs of quests being accepted in "Accept All" flow
const acceptingAllQuestIds = ref<Set<string>>(new Set())

// Loading state for the Start button (fetching detectable games, etc.)
const startingQuestId = ref<string | null>(null)

// Confirmation dialogs state
const showAcceptAllDialog = ref(false)
const showCompleteAllDialog = ref(false)
const pendingAcceptQuests = ref<Quest[]>([])
const pendingCompleteQuests = ref<Quest[]>([])

// Batch complete dialog state (parameterized for video/game/all)
const showBatchCompleteDialog = ref(false)
const batchCompleteType = ref<'video' | 'game' | 'all'>('all')
const pendingBatchCompleteQuests = ref<Quest[]>([])

// Exe pre-selection state for batch game quests
const batchExeSelections = ref<Map<string, string>>(new Map())
const showBatchExeSelectDialog = ref(false)
const batchExeSelectOptions = ref<string[]>([])
const batchExeSelectGameName = ref('')
const batchExeSelectResolve = ref<((exe: string | null) => void) | null>(null)

// Exe selection dialog state (for play quests with multiple win32 executables)
const showExeSelectDialog = ref(false)
const exeSelectOptions = ref<string[]>([])
const exeSelectGameName = ref('')
const pendingPlayQuest = ref<{ quest: Quest, secondsNeeded: number, initialProgress: number } | null>(null)

// Custom exe input dialog state (for play quests with NO known executables)
const showCustomExeDialog = ref(false)
const customExeGameName = ref('')
const customExeInput = ref('')

// Activity quest launch dialog state
const showActivityLaunchDialog = ref(false)
const activityLaunchQuest = ref<Quest | null>(null)
const activityLaunchError = ref<string | null>(null)
const activityNavigatingToDiscord = ref(false)

const activeFilterCount = computed(() => {
  let count = 0
  if (advancedFilters.value.query.trim()) count++
  if (advancedFilters.value.types.video) count++
  if (advancedFilters.value.types.play) count++
  if (advancedFilters.value.types.activity) count++
  if (advancedFilters.value.rewards.orbs) count++
  if (advancedFilters.value.rewards.avatarDecoration) count++
  if (advancedFilters.value.rewards.ingame) count++
  if (advancedFilters.value.includeExpired) count++
  return count
})

const hasActiveFilters = computed(() => activeFilterCount.value > 0)

function clearFilters() {
  advancedFilters.value = emptyAdvancedQuestFilters()
}

function backToRecommended() {
  selectedPreset.value = 'recommended'
  clearFilters()
}

const { buckets: questBuckets, recommendedQuests } = useHomeQuestState(
  computed(() => questsStore.quests),
  {
    activeQuestId: computed(() => questsStore.activeQuestId),
    questQueue: computed(() => questsStore.questQueue),
    blockedUntil: computed(() => questsStore.questEnrollmentBlockedUntil),
    cdpAvailable: computed(() => questsStore.cdpAvailable),
  }
)

const runningCount = computed(() => (questBuckets.value.active ? 1 : 0) + questBuckets.value.queued.length)

function selectPreset(preset: QuestViewPreset) {
  selectedPreset.value = preset
}

onMounted(() => {
  if (authStore.user) {
    questsStore.fetchQuests()
    questsStore.fetchOrbsBalance().catch(err => {
      console.warn('Background Orbs balance fetch failed:', err)
    })
  }
})

watch(() => authStore.user, (newUser) => {
  if (newUser) {
    questsStore.fetchQuests()
    questsStore.fetchOrbsBalance().catch(err => {
      console.warn('Background Orbs balance fetch failed:', err)
    })
  } else {
    questsStore.quests = []
  }
})


async function refreshQuests() {
  await questsStore.fetchQuests(false, true)
}


function formatBlockedUntil(value: string): string {
  return new Date(value).toLocaleString()
}

// Determine quest type based on task_config
function getQuestType(quest: Quest): 'video' | 'stream' | 'activity' {
  return getQuestKind(quest)
}

// Get button text based on quest type
function getStartButtonText(quest: Quest): string {
  const task = firstStartableTask(quest)
  if (!task) return t('home.start_quest')
  if (isVideoTask(task)) return t('home.start_watching')
  if (isDesktopPlayTask(task)) return t('home.start_playing')
  if (isStreamTask(task)) return t('home.start_streaming')
  if (isActivityTask(task)) return t('home.launch_activity')
  return t('home.start_quest')
}

// Get reward type for a quest
function getRewardType(quest: Quest): 'orbs' | 'avatar' | 'ingame' {
  return getQuestRewardCategory(quest)
}

// Quests completed but not yet claimed (across the full store, not filtered)
// Excludes expired quests to align with filteredQuests visibility rules
const pendingClaimCount = computed(() =>
  questBuckets.value.readyToClaim.length
)

// Show banner only when pending-claim quests exist but aren't visible in the current filtered view
const showPendingClaimBanner = computed(() => {
  if (pendingClaimCount.value === 0) return false
  return !filteredQuests.value.some(q => q.user_status?.completed_at && !q.user_status?.claimed_at)
})

function showPendingClaimFilter() {
  selectedPreset.value = 'ready_to_claim'
}

function visibleQuest(quest: Quest): boolean {
  if (advancedFilters.value.includeExpired) return true
  return !isQuestExpired(quest) || !!quest.user_status?.claimed_at
}

function sortByExpiry(a: Quest, b: Quest): number {
  const aTime = a.config.expires_at ? new Date(a.config.expires_at).getTime() : Number.MAX_SAFE_INTEGER
  const bTime = b.config.expires_at ? new Date(b.config.expires_at).getTime() : Number.MAX_SAFE_INTEGER
  return aTime - bTime
}

function uniqueQuests(quests: Quest[]): Quest[] {
  const seen = new Set<string>()
  return quests.filter(quest => {
    if (seen.has(quest.id)) return false
    seen.add(quest.id)
    return true
  })
}

function questsForPreset(preset: QuestViewPreset): Quest[] {
  switch (preset) {
    case 'to_accept':
      return questBuckets.value.toAccept
    case 'ready_to_run':
      return questBuckets.value.readyToRun
    case 'ready_to_claim':
      return questBuckets.value.readyToClaim
    case 'completed':
      return questBuckets.value.completed
    case 'all':
      return questsStore.quests
    case 'recommended':
    default:
      return recommendedQuests.value
  }
}

const viewCounts = computed<Record<QuestViewPreset, number>>(() => ({
  recommended: getRecommendedQuests(questBuckets.value).filter(visibleQuest).length,
  to_accept: questBuckets.value.toAccept.filter(visibleQuest).length,
  ready_to_run: questBuckets.value.readyToRun.filter(visibleQuest).length,
  ready_to_claim: questBuckets.value.readyToClaim.filter(visibleQuest).length,
  completed: questBuckets.value.completed.filter(visibleQuest).length,
  all: questsStore.quests.filter(visibleQuest).length,
}))

const filteredQuests = computed(() => {
  let quests = uniqueQuests(questsForPreset(selectedPreset.value)).filter(visibleQuest)

  const query = advancedFilters.value.query.toLowerCase().trim()
  if (query) {
    quests = quests.filter(q => {
      const questName = q.config.messages?.quest_name?.toLowerCase() || ''
      const gameTitle = q.config.messages?.game_title?.toLowerCase() || ''
      return questName.includes(query) || gameTitle.includes(query)
    })
  }

  const typeFiltersActive = advancedFilters.value.types.video || advancedFilters.value.types.play || advancedFilters.value.types.activity
  if (typeFiltersActive) {
    quests = quests.filter(quest => {
      const questType = getQuestType(quest)
      return (advancedFilters.value.types.video && questType === 'video')
        || (advancedFilters.value.types.play && questType === 'stream')
        || (advancedFilters.value.types.activity && questType === 'activity')
    })
  }

  const rewardFiltersActive = advancedFilters.value.rewards.orbs ||
    advancedFilters.value.rewards.avatarDecoration ||
    advancedFilters.value.rewards.ingame
  if (rewardFiltersActive) {
    quests = quests.filter(quest => {
      const rewardType = getRewardType(quest)
      return (advancedFilters.value.rewards.orbs && rewardType === 'orbs')
        || (advancedFilters.value.rewards.avatarDecoration && rewardType === 'avatar')
        || (advancedFilters.value.rewards.ingame && rewardType === 'ingame')
    })
  }

  return quests.sort(sortByExpiry)
})

const unenrolledCount = computed(() => {
  return filteredQuests.value.filter(q => !q.user_status?.enrolled_at && !isQuestExpired(q)).length
})

const enrolledVideoCount = computed(() => {
  return filteredQuests.value.filter(q => {
     // Strict check: Must be a VIDEO quest as determined by task config
     // Previous check only looked for stream duration, which let "Play" quests through
     const isVideo = getQuestType(q) === 'video'
     const isEnrolled = !!q.user_status?.enrolled_at
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted && isVideo && !isQuestExpired(q)
  }).length
})

const enrolledGameCount = computed(() => {
  return filteredQuests.value.filter(q => {
     const questType = getQuestType(q)
     const isGame = questType === 'stream' // stream kind = play/game quests
     const isEnrolled = !!q.user_status?.enrolled_at
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted && isGame && !isQuestExpired(q)
  }).length
})

const enrolledAllCount = computed(() => {
  return filteredQuests.value.filter(q => {
     const questType = getQuestType(q)
     // Exclude activity (requires manual interaction)
     if (questType === 'activity') return false
     const isEnrolled = !!q.user_status?.enrolled_at
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted && !isQuestExpired(q)
  }).length
})

const isBatchAccepting = computed(() => acceptingAllQuestIds.value.size > 0)

type HomeNextAction = {
  state: NextBestActionState
  title: string
  description: string
  primaryLabel: string
  secondaryLabel?: string
  primaryDisabled?: boolean
  secondaryDisabled?: boolean
  targetQuest?: Quest
}

const recommendedRunQuest = computed(() => questBuckets.value.readyToRun[0] ?? null)
const recommendedAcceptQuest = computed(() => questBuckets.value.toAccept[0] ?? null)
const recommendedClaimQuest = computed(() => questBuckets.value.readyToClaim[0] ?? null)

const nextBestAction = computed<HomeNextAction>(() => {
  if (questsStore.error) {
    return {
      state: 'error',
      title: t('home.next_error_title'),
      description: questsStore.error,
      primaryLabel: t('general.refresh'),
    }
  }

  if (questBuckets.value.blockedUntil) {
    return {
      state: 'blocked',
      title: t('home.next_blocked_title'),
      description: t('home.next_blocked_desc', { time: formatBlockedUntil(questBuckets.value.blockedUntil) }),
      primaryLabel: t('general.refresh'),
      secondaryLabel: t('home.view_current_quests'),
      primaryDisabled: questsStore.loading,
    }
  }

  if (questBuckets.value.active) {
    return {
      state: 'active',
      title: t('home.next_active_title', { name: questBuckets.value.active.config.messages.quest_name }),
      description: t('home.next_active_desc'),
      primaryLabel: t('home.stop'),
      primaryDisabled: questsStore.stopping,
      targetQuest: questBuckets.value.active,
    }
  }

  if (questsStore.isQueueRunning || questBuckets.value.queued.length > 0) {
    return {
      state: 'queue',
      title: t('home.next_queue_title', { count: questBuckets.value.queued.length }),
      description: t('home.next_queue_desc'),
      primaryLabel: t('home.stop_queue'),
    }
  }

  if (recommendedClaimQuest.value) {
    return {
      state: 'claim',
      title: t('home.next_claim_title', { count: questBuckets.value.readyToClaim.length }),
      description: t('home.next_claim_desc', { name: recommendedClaimQuest.value.config.messages.quest_name }),
      primaryLabel: t('home.claim_reward'),
      secondaryLabel: t('home.view_all_ready_to_claim'),
      primaryDisabled: claimingQuest.value === recommendedClaimQuest.value.id,
      targetQuest: recommendedClaimQuest.value,
    }
  }

  if (recommendedRunQuest.value) {
    return {
      state: 'run',
      title: t('home.next_run_title', { count: questBuckets.value.readyToRun.length }),
      description: t('home.next_run_desc', { name: recommendedRunQuest.value.config.messages.quest_name }),
      primaryLabel: getStartButtonText(recommendedRunQuest.value),
      secondaryLabel: enrolledAllCount.value > 1 ? t('home.complete_all_recommended') : t('home.view_all_ready'),
      primaryDisabled: questsStore.activeQuestId !== null || startingQuestId.value !== null || isBatchAccepting.value,
      targetQuest: recommendedRunQuest.value,
    }
  }

  if (recommendedAcceptQuest.value) {
    return {
      state: 'accept',
      title: t('home.next_accept_title', { count: questBuckets.value.toAccept.length }),
      description: t('home.next_accept_desc', { name: recommendedAcceptQuest.value.config.messages.quest_name }),
      primaryLabel: questBuckets.value.toAccept.length > 1 ? t('home.accept_all_quests') : t('home.accept_quest'),
      primaryDisabled: isBatchAccepting.value || acceptingQuest.value === recommendedAcceptQuest.value.id,
      targetQuest: recommendedAcceptQuest.value,
    }
  }

  if (questsStore.quests.length === 0) {
    return {
      state: 'empty',
      title: t('home.next_empty_title'),
      description: t('home.next_empty_desc'),
      primaryLabel: t('general.refresh'),
      primaryDisabled: questsStore.loading,
    }
  }

  return {
    state: 'done',
    title: t('home.next_done_title'),
    description: t('home.next_done_desc'),
    primaryLabel: t('general.refresh'),
    secondaryLabel: selectedPreset.value === 'all' ? undefined : t('home.view_all'),
    primaryDisabled: questsStore.loading,
  }
})

const nextActionBusy = computed(() => {
  const quest = nextBestAction.value.targetQuest
  if (!quest) return questsStore.loading || questsStore.stopping
  return startingQuestId.value === quest.id ||
    acceptingQuest.value === quest.id ||
    acceptingAllQuestIds.value.has(quest.id) ||
    claimingQuest.value === quest.id ||
    questsStore.stopping
})

function handleNextPrimary() {
  const action = nextBestAction.value

  if (action.state === 'active') {
    questsStore.stop()
    return
  }

  if (action.state === 'queue') {
    questsStore.clearQueue()
    return
  }

  if (action.state === 'claim' && action.targetQuest) {
    claimReward(action.targetQuest)
    return
  }

  if (action.state === 'run' && action.targetQuest) {
    startQuest(action.targetQuest)
    return
  }

  if (action.state === 'accept') {
    if (questBuckets.value.toAccept.length > 1) handleAcceptAll()
    else if (action.targetQuest) acceptQuest(action.targetQuest)
    return
  }

  refreshQuests()
}

function handleNextSecondary() {
  const action = nextBestAction.value
  if (action.state === 'claim') {
    selectedPreset.value = 'ready_to_claim'
    return
  }

  if (action.state === 'run') {
    if (enrolledAllCount.value > 1) handleCompleteAllTasks()
    else selectedPreset.value = 'ready_to_run'
    return
  }

  if (action.state === 'accept') {
    selectedPreset.value = 'to_accept'
    return
  }

  selectedPreset.value = 'all'
}

const emptyStateText = computed(() => {
  if (hasActiveFilters.value) return t('home.empty_filtered')
  switch (selectedPreset.value) {
    case 'recommended':
      return t('home.empty_recommended')
    case 'to_accept':
      return t('home.empty_to_accept')
    case 'ready_to_run':
      return t('home.empty_ready_to_run')
    case 'ready_to_claim':
      return t('home.empty_ready_to_claim')
    case 'completed':
      return t('home.empty_completed')
    default:
      return t('home.no_quests_match')
  }
})

function handleAcceptAll() {
  const toAccept = filteredQuests.value.filter(q => {
    // Check if not enrolled
    if (q.user_status?.enrolled_at) return false
    
    // Check expiration explicitly
    if (q.config.expires_at) {
       const expires = new Date(q.config.expires_at)
       if (expires < new Date()) return false
    }
    
    return true
  })
  if (toAccept.length === 0) return
  pendingAcceptQuests.value = toAccept
  showAcceptAllDialog.value = true
}

async function confirmAcceptAll() {
  const questIds = pendingAcceptQuests.value.map(q => q.id)
  showAcceptAllDialog.value = false
  pendingAcceptQuests.value = []

  // Track accepting state per-quest instead of global loading
  for (const id of questIds) {
    acceptingAllQuestIds.value.add(id)
  }
  acceptingAllQuestIds.value = new Set(acceptingAllQuestIds.value) // trigger reactivity

  let successCount = 0
  let failCount = 0
  for (const id of questIds) {
    try {
      await acceptQuestApi(id)
      questsStore.updateQuestEnrollment(id, new Date().toISOString())
      successCount++
    } catch (e) {
      console.error(`Failed to accept quest ${id}:`, e)
      failCount++
    } finally {
      acceptingAllQuestIds.value.delete(id)
      acceptingAllQuestIds.value = new Set(acceptingAllQuestIds.value) // trigger reactivity
    }
    // Small delay to be nice to API
    await new Promise(r => setTimeout(r, 500))
  }

  if (failCount > 0) {
    toast.error({ title: t('toast.failed_accept_batch', { success: successCount, fail: failCount }) })
  }
}

function handleCompleteAllVideo() {
  const toComplete = filteredQuests.value.filter(q => {
     const isVideo = getQuestType(q) === 'video'
     const isEnrolled = !!q.user_status?.enrolled_at
     const isCompleted = !!q.user_status?.completed_at
     
     // Check expiration explicitly
     if (q.config.expires_at) {
       const expires = new Date(q.config.expires_at)
       if (expires < new Date()) return false
     }
     
     return isEnrolled && !isCompleted && isVideo
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

function handleCompleteAllGame() {
  const toComplete = filteredQuests.value.filter(q => {
     const questType = getQuestType(q)
     const isGame = questType === 'stream'
     const isEnrolled = !!q.user_status?.enrolled_at
     const isCompleted = !!q.user_status?.completed_at
     if (q.config.expires_at) {
       const expires = new Date(q.config.expires_at)
       if (expires < new Date()) return false
     }
     return isEnrolled && !isCompleted && isGame
  })
  if (toComplete.length === 0) return
  pendingBatchCompleteQuests.value = toComplete
  batchCompleteType.value = 'game'
  showBatchCompleteDialog.value = true
}

function handleCompleteAllTasks() {
  const toComplete = filteredQuests.value.filter(q => {
     const questType = getQuestType(q)
     if (questType === 'activity') return false
     const isEnrolled = !!q.user_status?.enrolled_at
     const isCompleted = !!q.user_status?.completed_at
     if (q.config.expires_at) {
       const expires = new Date(q.config.expires_at)
       if (expires < new Date()) return false
     }
     return isEnrolled && !isCompleted
  })
  if (toComplete.length === 0) return
  pendingBatchCompleteQuests.value = toComplete
  batchCompleteType.value = 'all'
  showBatchCompleteDialog.value = true
}

async function confirmBatchComplete() {
  const quests = [...pendingBatchCompleteQuests.value]
  showBatchCompleteDialog.value = false
  pendingBatchCompleteQuests.value = []

  // Separate game quests that need exe pre-selection (simulate mode only)
  const gameQuests = quests.filter(q => getQuestType(q) === 'stream')
  const needExePreselection = questsStore.gameQuestMode === 'simulate' && gameQuests.length > 0

  if (needExePreselection) {
    try {
      await preselectExesForGameQuests(gameQuests)
    } catch {
      // User cancelled or error — abort batch
      return
    }
  }

  // Add all quests to queue with pre-selected exe names
  quests.forEach(q => {
    const exeName = batchExeSelections.value.get(q.id)
    questsStore.addToQueue(q, exeName)
  })
  questsStore.startQueue()
  batchExeSelections.value = new Map()
}

/**
 * Pre-select executables for all game quests in a batch.
 * Shows selection dialogs sequentially for games that have multiple win32 executables.
 * For games with no known executables, shows a custom input dialog.
 * Results are stored in batchExeSelections map.
 * Resolves when all selections are done, rejects if user cancels any.
 */
async function preselectExesForGameQuests(gameQuests: Quest[]): Promise<void> {
  const gamesList = await questsStore.getDetectableGames()

  for (const quest of gameQuests) {
    const appId = quest.config.application?.id
    if (!appId) continue

    const game = gamesList.find(g => g.id === appId)
    if (!game) continue

    const winExes = game.executables.filter(e => e.os === 'win32')

    if (winExes.length > 1) {
      // Multiple executables — show selection dialog
      const selected = await showBatchExeSelectDialogAsync(winExes.map(e => e.name), game.name)
      if (selected === null) throw new Error('User cancelled')
      batchExeSelections.value.set(quest.id, selected)
    } else if (winExes.length === 0) {
      // No known executables — show custom input dialog
      const custom = await showBatchCustomExeDialogAsync(game.name)
      if (custom === null) throw new Error('User cancelled')
      batchExeSelections.value.set(quest.id, custom)
    } else {
      // Exactly one executable — auto-select
      batchExeSelections.value.set(quest.id, winExes[0].name)
    }
  }
}

/** Show exe selection dialog and return selected exe name, or null if cancelled */
function showBatchExeSelectDialogAsync(options: string[], gameName: string): Promise<string | null> {
  return new Promise(resolve => {
    batchExeSelectOptions.value = options
    batchExeSelectGameName.value = gameName
    batchExeSelectResolve.value = resolve
    showBatchExeSelectDialog.value = true
  })
}

/** Show custom exe input dialog and return entered name, or null if cancelled */
function showBatchCustomExeDialogAsync(gameName: string): Promise<string | null> {
  return new Promise(resolve => {
    customExeGameName.value = gameName
    customExeInput.value = ''
    // Reuse existing custom exe dialog with a different resolve
    batchExeSelectResolve.value = resolve
    showCustomExeDialog.value = true
  })
}

function selectBatchExe(exeName: string) {
  showBatchExeSelectDialog.value = false
  batchExeSelectResolve.value?.(exeName)
  batchExeSelectResolve.value = null
}

function cancelBatchExeSelect() {
  showBatchExeSelectDialog.value = false
  batchExeSelectResolve.value?.(null)
  batchExeSelectResolve.value = null
}

function cancelCustomExeDialog() {
  showCustomExeDialog.value = false
  // If in batch pre-selection mode, resolve with null to cancel
  if (batchExeSelectResolve.value) {
    batchExeSelectResolve.value(null)
    batchExeSelectResolve.value = null
  }
}

async function selectExeAndStartPlay(exeName: string) {
  showExeSelectDialog.value = false
  if (!pendingPlayQuest.value) return
  const { quest, secondsNeeded, initialProgress } = pendingPlayQuest.value
  pendingPlayQuest.value = null
  try {
    await questsStore.startPlay(quest, secondsNeeded, initialProgress, exeName)
  } catch (e) {
    toast.error({
      title: t('toast.failed_start_game'),
      description: String(e),
      actions: [{ label: t('toast.open_settings'), onClick: () => navigateToTab('settings', 'quest_behavior') }],
    })
  }
}

async function submitCustomExeAndStartPlay() {
  const exeName = customExeInput.value.trim()
  if (!exeName) return
  showCustomExeDialog.value = false

  // Batch mode — resolve the pre-selection promise
  if (batchExeSelectResolve.value) {
    batchExeSelectResolve.value(exeName)
    batchExeSelectResolve.value = null
    return
  }

  // Single quest mode
  if (!pendingPlayQuest.value) return
  const { quest, secondsNeeded, initialProgress } = pendingPlayQuest.value
  pendingPlayQuest.value = null
  try {
    await questsStore.startPlay(quest, secondsNeeded, initialProgress, exeName)
  } catch (e) {
    toast.error({
      title: t('toast.failed_start_game'),
      description: String(e),
      actions: [{ label: t('toast.open_settings'), onClick: () => navigateToTab('settings', 'quest_behavior') }],
    })
  }
}

function getExpiryColor(dateStr: string | null | undefined): string {
  if (!dateStr) return 'text-muted-foreground'
  const expires = new Date(dateStr)
  const now = new Date()
  const diff = expires.getTime() - now.getTime()
  
  if (diff < 0) return 'text-destructive font-bold' // Expired
  if (diff < 1000 * 60 * 60 * 24) return 'text-orange-500' // < 24h
  return 'text-green-600'
}

function getExpiryText(dateStr: string | null | undefined): string {
  if (!dateStr) return t('quest.no_expiry')
  const expires = new Date(dateStr)
  const now = new Date()
  const diff = expires.getTime() - now.getTime()

  const dateText = expires.toLocaleString()

  if (diff < 0) return `${dateText} (${t('quest.expired')})`

  const days = Math.floor(diff / (1000 * 60 * 60 * 24))
  const hours = Math.floor((diff % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60))

  if (days > 0) return t('quest.time_left_days', { date: dateText, days, hours })
  return t('quest.time_left_hours', { date: dateText, hours })
}

function canStartQuest(quest: Quest): boolean {
  // Check if quest is already completed
  if (quest.user_status?.completed_at) return false
  return firstStartableTask(quest) !== null
}

async function startQuest(quest: Quest) {
  // Blur the focused button before DOM mutation to prevent scroll-to-top
  // when the button is removed from the DOM by the v-else-if condition change.
  if (document.activeElement instanceof HTMLElement) {
    document.activeElement.blur()
  }

  if (questsStore.activeQuestId) return
  
  startingQuestId.value = quest.id
  try {
  const task = firstStartableTask(quest)
  if (!task?.target) return
  
  const secondsNeeded = task.target
  const initialProgress = firstProgressValue(quest, task.key)
  const taskTypes = getQuestTasks(quest).map(item => item.type)
  const isVideoQuest = isVideoTask(task)
  const isPlayQuest = isDesktopPlayTask(task)
  const isStreamQuest = isStreamTask(task)
  const isActivityQuest = isActivityTask(task)
  
  console.log(`Starting quest: type=${task.type}, target=${secondsNeeded}s, progress=${initialProgress}s`)
  
  if (isVideoQuest) {
    // Video quest - uses video-progress API with timestamp
    await questsStore.startVideo(quest.id, secondsNeeded, initialProgress)
  } else if (isPlayQuest) {
    // Play quests - use Game Simulator logic (one-click)
    const gameName = quest.config.messages.game_title || quest.config.messages.quest_name
    console.log(`Starting play quest for ${gameName}`)
    try {
        // Check if there are multiple win32 executables — let user choose
        // Skip exe selection entirely for CDP mode (doesn't need executables)
        if (questsStore.gameQuestMode === 'simulate') {
          const appId = quest.config.application?.id
          if (appId) {
            const gamesList = await questsStore.getDetectableGames()
            const game = gamesList.find(g => g.id === appId)
            if (game) {
              const winExes = game.executables.filter(e => e.os === 'win32')
              if (winExes.length > 1) {
                // Multiple win32 executables — show selection dialog
                exeSelectOptions.value = winExes.map(e => e.name)
                exeSelectGameName.value = game.name
                pendingPlayQuest.value = { quest, secondsNeeded, initialProgress }
                showExeSelectDialog.value = true
                return // Wait for user selection
              } else if (winExes.length === 0) {
                // No known executables — show custom input dialog
                customExeGameName.value = game.name
                customExeInput.value = ''
                pendingPlayQuest.value = { quest, secondsNeeded, initialProgress }
                showCustomExeDialog.value = true
                return // Wait for user input
              }
            }
          }
        }
        await questsStore.startPlay(quest, secondsNeeded, initialProgress)
    } catch (e) {
        toast.error({
          title: t('toast.failed_start_game'),
          description: String(e),
          actions: [{ label: t('toast.open_settings'), onClick: () => navigateToTab('settings', 'quest_behavior') }],
        })
    }
  } else if (isStreamQuest) {
    // Stream quests require actually streaming a game
    toast.info({
      title: t('toast.stream_quest_title'),
      description: t('toast.stream_quest_desc'),
      duration: 0,
      actions: [{ label: t('toast.known'), onClick: () => {} }],
    })
  } else if (isActivityQuest) {
    // Activity quest - requires CDP mode
    if (!questsStore.cdpAvailable) {
      toast.warning({
        title: t('toast.activity_cdp_required'),
        actions: [{ label: t('toast.open_settings'), onClick: () => navigateToTab('settings', 'discord_integration') }],
      })
      return
    }
    // Show the activity launch dialog
    activityLaunchQuest.value = quest
    activityLaunchError.value = null
    showActivityLaunchDialog.value = true
    return // Wait for user to confirm in dialog
  } else {
    // Unknown type - show warning
    toast.error({ title: t('toast.unknown_quest_type', { types: taskTypes.join(', ') || 'none' }) })
  }
  } finally {
    startingQuestId.value = null
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
    toast.error({
      title: t('toast.failed_accept'),
      description: String(error),
      actions: [{ label: t('toast.retry'), onClick: () => acceptQuest(quest) }],
    })
  } finally {
    acceptingQuest.value = null
  }
}

// Activity quest launch dialog handlers
async function navigateActivityQuestInDiscord() {
  const quest = activityLaunchQuest.value
  if (!quest || activityNavigatingToDiscord.value) return

  const questPath = `/quest-home#${encodeURIComponent(quest.id)}`
  activityLaunchError.value = null
  activityNavigatingToDiscord.value = true

  try {
    await navigateDiscordSpa(questPath, questsStore.cdpPort)
  } catch (error) {
    console.error('Failed to navigate Discord to quest page:', error)
    activityLaunchError.value = t('home.activity_navigate_error')
  } finally {
    activityNavigatingToDiscord.value = false
  }
}

async function confirmActivityLaunch() {
  const quest = activityLaunchQuest.value
  if (!quest) return

  showActivityLaunchDialog.value = false
  startingQuestId.value = quest.id
  try {
    await questsStore.startActivity(quest)
  } catch (e) {
    toast.error({ title: t('toast.failed_start_activity'), description: String(e) })
  } finally {
    startingQuestId.value = null
    activityLaunchQuest.value = null
  }
}

function cancelActivityLaunch() {
  showActivityLaunchDialog.value = false
  activityLaunchQuest.value = null
  activityLaunchError.value = null
  activityNavigatingToDiscord.value = false
}

async function claimReward(quest: Quest) {
  if (claimingQuest.value) return
  claimingQuest.value = quest.id

  try {
    if (questsStore.cdpAvailable) {
      // Navigate to the quest page in Discord client so user can claim there
      const questPath = `/quest-home#${encodeURIComponent(quest.id)}`
      await navigateDiscordSpa(questPath, questsStore.cdpPort)
      // Show brief inline notice near the button
      if (claimedNoticeTimer) clearTimeout(claimedNoticeTimer)
      claimedNoticeQuestId.value = quest.id
      claimedNoticeTimer = setTimeout(() => { claimedNoticeQuestId.value = null }, 4000)
    } else {
      // Fallback: try API claim when CDP is not available
      await claimQuestReward(quest.id)
      await questsStore.fetchQuests(true, true)
    }
  } catch (error) {
    console.error('Failed to claim quest reward:', error)
    toast.error({ title: t('toast.failed_claim'), description: String(error) })
  } finally {
    claimingQuest.value = null
  }
}
</script>

<style scoped>
/* Claim notice: fade in, stay, then fade out */
@keyframes noticeLifecycle {
  0% { opacity: 0; transform: translateY(-4px); }
  10% { opacity: 1; transform: translateY(0); }
  75% { opacity: 1; transform: translateY(0); }
  100% { opacity: 0; transform: translateY(-2px); }
}

.notice-fade {
  animation: noticeLifecycle 4s ease forwards;
}

/* Quest list leave animation */
.quest-list-leave-active {
  transition: opacity 0.35s ease, transform 0.35s ease;
  pointer-events: none;
}
.quest-list-leave-to {
  opacity: 0;
  transform: translateY(-6px) scale(0.98);
}
</style>
