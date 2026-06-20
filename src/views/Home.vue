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
              :disabled="questsStore.loading || !authStore.user || isBatchAccepting"
              class="gap-2 flex-1 sm:flex-none"
            >
              <RotateCw :class="cn('w-4 h-4', questsStore.loading && 'animate-spin')" />
              {{ t('general.refresh') }}
            </Button>
          </div>
        </div>

        <div
          v-if="
            questsStore.questEnrollmentBlockedUntil &&
            new Date(questsStore.questEnrollmentBlockedUntil).getTime() > Date.now()
          "
          class="flex items-center gap-2 rounded-lg border border-yellow-500/30 bg-yellow-500/10 p-3 text-sm text-yellow-700 dark:text-yellow-300"
        >
          <AlertCircle class="h-4 w-4 shrink-0" />
          <span>{{ t('home.enrollment_blocked_until', { time: formatBlockedUntil(questsStore.questEnrollmentBlockedUntil) }) }}</span>
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
            :disabled="isBatchAccepting"
            class="bg-green-600 hover:bg-green-700 text-white gap-2"
          >
            <Check class="w-4 h-4" />
            {{ t('home.accept_all') }} ({{ unenrolledCount }})
          </Button>

          <div
            v-if="hasAnyCompleteOption && !questsStore.isQueueRunning"
            class="flex flex-wrap gap-2 items-center p-1.5 rounded-xl bg-muted/50 border border-border/60 shadow-sm"
          >
            <Button
              v-if="enrolledAllCount > 0"
              @click="handleCompleteAllTasks"
              :disabled="isBatchAccepting"
              variant="default"
              class="gap-2 shadow-sm"
            >
              <Sparkles class="w-4 h-4" />
              {{ t('home.complete_all_tasks') }} ({{ enrolledAllCount }})
            </Button>

            <Button
              v-if="enrolledVideoCount > 0"
              @click="handleCompleteAllVideo"
              :disabled="isBatchAccepting"
              variant="secondary"
              class="gap-2"
            >
              <MonitorPlay class="w-4 h-4" />
              {{ t('home.complete_all_video') }} ({{ enrolledVideoCount }})
            </Button>

            <Button
              v-if="enrolledGameCount > 0"
              @click="handleCompleteAllGame"
              :disabled="isBatchAccepting"
              variant="secondary"
              class="gap-2"
            >
              <Gamepad2 class="w-4 h-4" />
              {{ t('home.complete_all_game') }} ({{ enrolledGameCount }})
            </Button>
          </div>

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
          <p class="text-muted-foreground">{{ t('home.no_quests_match') }}</p>
          <Button variant="link" @click="clearFilters" v-if="hasActiveFilters">{{ t('home.clear_filters') }}</Button>
        </div>

        <template v-else>
        <!-- Pending Claim Reminder -->
        <div
          v-if="showPendingClaimBanner"
          class="flex items-center justify-between gap-3 p-3 rounded-lg border border-yellow-500/30 bg-yellow-500/10"
        >
          <div class="flex items-center gap-2 text-sm text-yellow-700 dark:text-yellow-300">
            <Gift class="w-4 h-4 shrink-0" />
            <span>{{ t('home.pending_claim_reminder', { count: pendingClaimCount }) }}</span>
          </div>
          <Button size="sm" variant="outline" class="shrink-0 border-yellow-500/40 text-yellow-700 dark:text-yellow-300 hover:bg-yellow-500/10" @click="showPendingClaimFilter">
            {{ t('home.view_pending_claim') }}
          </Button>
        </div>

        <TransitionGroup name="quest-list" tag="div" class="space-y-4">
          <QuestCard 
            v-for="quest in filteredQuests" 
            :key="quest.id"
            :quest="quest"
            :quest-type="getQuestType(quest)"
            :show-developer-details="props.debugModeEnabled"
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

              <Button
                v-else-if="quest.user_status?.completed_at && !quest.user_status?.claimed_at"
                :disabled="claimingQuest === quest.id || isBatchAccepting"
                class="bg-green-600 hover:bg-green-700 text-white gap-1.5"
                @click="claimReward(quest)"
              >
                <Loader2 v-if="claimingQuest === quest.id" class="w-4 h-4 animate-spin" />
                <Gift v-else class="w-4 h-4" />
                {{ t('home.claim_reward') }}
              </Button>
               <span v-else-if="quest.user_status?.completed_at" class="text-sm font-medium text-green-500 self-center px-2">
                {{ t('home.completed') }}
              </span>
            </template>
          </QuestCard>
        </TransitionGroup>
        </template>
      </div>
      
      <div class="lg:col-span-1">
        <QuestProgress />
      </div>
    </div>

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
                    {{ quest.config.expires_at ? new Date(quest.config.expires_at).toLocaleString() : 'No Expiry' }}
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
                      {{ q.config.messages.game_title }} • ID: {{ q.id }}
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
                      {{ q.config.messages.game_title }} • {{ getQuestType(q) === 'video' ? t('filter.video') : t('filter.play') }} • ID: {{ q.id }}
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { RotateCw, Filter, AlertCircle, Loader2, ArrowUpCircle, ExternalLink, Check, Search, Gift, MonitorPlay, Gamepad2, Sparkles } from 'lucide-vue-next'
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

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const versionStore = useVersionStore()

const props = defineProps<{
  debugModeEnabled?: boolean
}>()

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
const claimingQuest = ref<string | null>(null)

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

// Filtered quests based on filter state
// Quests completed but not yet claimed (across the full store, not filtered)
// Excludes expired quests to align with filteredQuests visibility rules
const pendingClaimCount = computed(() =>
  questsStore.quests.filter(q => {
    if (!q.user_status?.completed_at || q.user_status?.claimed_at) return false
    if (q.config.expires_at && new Date(q.config.expires_at) < new Date()) return false
    return true
  }).length
)

// Show banner only when pending-claim quests exist but aren't visible in the current filtered view
const showPendingClaimBanner = computed(() => {
  if (pendingClaimCount.value === 0) return false
  return !filteredQuests.value.some(q => q.user_status?.completed_at && !q.user_status?.claimed_at)
})

function showPendingClaimFilter() {
  filters.value.status.pendingClaim = true
  showFilters.value = true
}

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
     // Strict check: Must be a VIDEO quest as determined by task config
     // Previous check only looked for stream duration, which let "Play" quests through
     const isVideo = getQuestType(q) === 'video'
     const isEnrolled = !!q.user_status
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted && isVideo
  }).length
})

const enrolledGameCount = computed(() => {
  return filteredQuests.value.filter(q => {
     const questType = getQuestType(q)
     const isGame = questType === 'stream' // stream kind = play/game quests
     const isEnrolled = !!q.user_status
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted && isGame
  }).length
})

const enrolledAllCount = computed(() => {
  return filteredQuests.value.filter(q => {
     const questType = getQuestType(q)
     // Exclude activity (requires manual interaction)
     if (questType === 'activity') return false
     const isEnrolled = !!q.user_status
     const isCompleted = !!q.user_status?.completed_at
     return isEnrolled && !isCompleted
  }).length
})

const isBatchAccepting = computed(() => acceptingAllQuestIds.value.size > 0)

const hasAnyCompleteOption = computed(() => enrolledAllCount.value > 0 || enrolledVideoCount.value > 0 || enrolledGameCount.value > 0)

function handleAcceptAll() {
  const toAccept = filteredQuests.value.filter(q => {
    // Check if not enrolled
    if (q.user_status) return false
    
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
    alert(`Accepted ${successCount} quests, failed ${failCount}`)
  }
}

function handleCompleteAllVideo() {
  const toComplete = filteredQuests.value.filter(q => {
     const isVideo = getQuestType(q) === 'video'
     const isEnrolled = !!q.user_status
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
     const isEnrolled = !!q.user_status
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
     const isEnrolled = !!q.user_status
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
    alert(`Failed to start game simulator: ${e}\n\nPlease try using the Game Simulator tab manually.`)
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
    alert(`Failed to start game simulator: ${e}\n\nPlease try using the Game Simulator tab manually.`)
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
  if (!dateStr) return 'No Expiry'
  const expires = new Date(dateStr)
  const now = new Date()
  const diff = expires.getTime() - now.getTime()
  
  const dateText = expires.toLocaleString()
  
  if (diff < 0) return `${dateText} (EXPIRED)`
  
  const days = Math.floor(diff / (1000 * 60 * 60 * 24))
  const hours = Math.floor((diff % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60))
  
  if (days > 0) return `${dateText} (${days}d ${hours}h left)`
  return `${dateText} (${hours}h left)`
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
        alert(`Failed to start game simulator: ${e}\n\nPlease try using the Game Simulator tab manually.`)
    }
  } else if (isStreamQuest) {
    // Stream quests require actually streaming a game
    const gameName = quest.config.messages.game_title || quest.config.messages.quest_name
    alert(`Stream quests require you to stream "${gameName}" on Discord.\n\nPlease:\n1. Start a stream in a Discord voice channel\n2. Use "Game Simulator" to simulate running the game\n3. Discord will track your streaming progress`)
  } else if (isActivityQuest) {
    // Activity quest - requires CDP mode
    if (!questsStore.cdpAvailable) {
      alert(t('home.activity_cdp_required'))
      return
    }
    // Show the activity launch dialog
    activityLaunchQuest.value = quest
    activityLaunchError.value = null
    showActivityLaunchDialog.value = true
    return // Wait for user to confirm in dialog
  } else {
    // Unknown type - show warning
    alert(`Unknown quest type: ${taskTypes.join(', ') || 'none'}\n\nPlease check the quest requirements in Discord.`)
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
    alert(`Failed to accept quest: ${error}`)
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
    alert(`Failed to start activity quest: ${e}`)
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
    } else {
      // Fallback: try API claim when CDP is not available
      await claimQuestReward(quest.id)
      await questsStore.fetchQuests(true, true)
    }
  } catch (error) {
    console.error('Failed to claim quest reward:', error)
    alert(`Failed to claim reward: ${error}`)
  } finally {
    claimingQuest.value = null
  }
}
</script>

<style scoped>
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
