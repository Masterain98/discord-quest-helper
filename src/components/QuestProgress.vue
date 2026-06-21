<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch, onUnmounted } from 'vue'
import { useQuestsStore } from '@/stores/quests'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { AlertCircle, ChevronUp, ListChecks, X } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const questsStore = useQuestsStore()
const expanded = ref(false)
const floatingRef = ref<HTMLElement | null>(null)
const floatingPosition = ref<{ left: number, top: number } | null>(null)
const draggedDuringPointer = ref(false)
const FLOATING_POSITION_KEY = 'questHelper_progressFloatingPosition'
const EDGE_MARGIN = 12

let dragState: {
  pointerId: number
  startX: number
  startY: number
  originLeft: number
  originTop: number
} | null = null

// Local progress is now managed by the store
const activeQuest = computed(() => {
  if (!questsStore.activeQuestId) return null
  return questsStore.quests.find(quest => quest.id === questsStore.activeQuestId) ?? null
})

const hasFloatingContent = computed(() =>
  !!questsStore.activeQuestId || questsStore.questQueue.length > 0 || !!questsStore.error
)

const queuedUpcoming = computed(() => {
  if (!questsStore.activeQuestId) return questsStore.questQueue
  return questsStore.questQueue.filter(quest => quest.id !== questsStore.activeQuestId)
})

const queuedBehindCount = computed(() => {
  return queuedUpcoming.value.length
})

const floatingTitle = computed(() => {
  if (questsStore.error && !questsStore.activeQuestId) return t('toast.error')
  if (questsStore.activeQuestId) return t('quest.active_progress')
  return `${t('quest.up_next')} (${questsStore.questQueue.length})`
})

const floatingSubtitle = computed(() => {
  if (questsStore.activeQuestId) {
    return activeQuest.value?.config.messages.quest_name ?? t('quest.active_progress')
  }
  if (questsStore.questQueue.length > 0) {
    return questsStore.questQueue[0]?.config.messages.quest_name ?? t('quest.up_next')
  }
  return questsStore.error ?? ''
})

const floatingStyle = computed(() => {
  if (!floatingPosition.value) return {}
  return {
    left: `${floatingPosition.value.left}px`,
    top: `${floatingPosition.value.top}px`,
    right: 'auto',
    bottom: 'auto',
  }
})

function clampPosition(left: number, top: number) {
  const rect = floatingRef.value?.getBoundingClientRect()
  const width = rect?.width ?? 384
  const height = rect?.height ?? 80
  const maxLeft = Math.max(EDGE_MARGIN, window.innerWidth - width - EDGE_MARGIN)
  const maxTop = Math.max(EDGE_MARGIN, window.innerHeight - height - EDGE_MARGIN)

  return {
    left: Math.min(maxLeft, Math.max(EDGE_MARGIN, left)),
    top: Math.min(maxTop, Math.max(EDGE_MARGIN, top)),
  }
}

function saveFloatingPosition() {
  if (!floatingPosition.value) return
  localStorage.setItem(FLOATING_POSITION_KEY, JSON.stringify(floatingPosition.value))
}

async function reconcileFloatingPosition() {
  await nextTick()
  const rect = floatingRef.value?.getBoundingClientRect()
  if (!rect) return
  floatingPosition.value = clampPosition(rect.left, rect.top)
  saveFloatingPosition()
}

function startDrag(event: PointerEvent) {
  if (event.button !== 0) return
  const rect = floatingRef.value?.getBoundingClientRect()
  if (!rect) return

  dragState = {
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    originLeft: rect.left,
    originTop: rect.top,
  }
  draggedDuringPointer.value = false

  if (event.currentTarget instanceof HTMLElement) {
    event.currentTarget.setPointerCapture(event.pointerId)
  }

  window.addEventListener('pointermove', handleDragMove)
  window.addEventListener('pointerup', stopDrag)
}

function handleDragMove(event: PointerEvent) {
  if (!dragState) return
  const dx = event.clientX - dragState.startX
  const dy = event.clientY - dragState.startY

  if (Math.abs(dx) + Math.abs(dy) > 4) {
    draggedDuringPointer.value = true
  }

  floatingPosition.value = clampPosition(dragState.originLeft + dx, dragState.originTop + dy)
}

function stopDrag(event: PointerEvent) {
  if (dragState && event.currentTarget instanceof HTMLElement) {
    try {
      event.currentTarget.releasePointerCapture(dragState.pointerId)
    } catch {
      // Pointer capture may already be released by the browser.
    }
  }

  dragState = null
  saveFloatingPosition()
  window.removeEventListener('pointermove', handleDragMove)
  window.removeEventListener('pointerup', stopDrag)
}

function handleCollapsedClick() {
  if (draggedDuringPointer.value) {
    draggedDuringPointer.value = false
    return
  }
  expanded.value = true
}

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

onMounted(() => {
  const savedPosition = localStorage.getItem(FLOATING_POSITION_KEY)
  if (!savedPosition) return
  try {
    const parsed = JSON.parse(savedPosition) as { left?: unknown, top?: unknown }
    if (typeof parsed.left === 'number' && typeof parsed.top === 'number') {
      floatingPosition.value = clampPosition(parsed.left, parsed.top)
    }
  } catch {
    localStorage.removeItem(FLOATING_POSITION_KEY)
  }
})

watch(hasFloatingContent, (visible) => {
  if (!visible) expanded.value = false
})

watch([expanded, queuedBehindCount], () => {
  if (hasFloatingContent.value) {
    reconcileFloatingPosition()
  }
})

onUnmounted(() => {
  window.removeEventListener('pointermove', handleDragMove)
  window.removeEventListener('pointerup', stopDrag)
})

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
  <div
    ref="floatingRef"
    v-if="hasFloatingContent"
    class="fixed bottom-5 right-5 z-50 w-[calc(100vw-2rem)] max-w-sm"
    :style="floatingStyle"
  >
    <button
      v-if="!expanded"
      type="button"
      class="w-full cursor-grab rounded-lg border bg-card px-4 py-3 text-left shadow-lg transition-all hover:-translate-y-0.5 hover:shadow-xl active:cursor-grabbing focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      @pointerdown="startDrag"
      @click="handleCollapsedClick"
    >
      <div class="flex items-center gap-3">
        <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
          <AlertCircle v-if="questsStore.error && !questsStore.activeQuestId" class="h-4 w-4" />
          <ListChecks v-else class="h-4 w-4" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center justify-between gap-3">
            <p class="truncate text-sm font-semibold">{{ floatingTitle }}</p>
            <div class="flex shrink-0 items-center gap-2">
              <span v-if="queuedBehindCount > 0" class="rounded-full bg-secondary px-2 py-0.5 text-[11px] font-medium text-secondary-foreground">
                {{ t('home.queue_count', { count: queuedBehindCount }) }}
              </span>
              <span v-if="questsStore.activeQuestId" class="text-sm font-semibold">
                {{ Math.floor(questsStore.activeQuestProgress) }}%
              </span>
            </div>
          </div>
          <p class="mt-0.5 truncate text-xs text-muted-foreground">{{ floatingSubtitle }}</p>
          <div v-if="questsStore.activeQuestId" class="mt-2 h-1.5 rounded-full bg-secondary">
            <div
              class="h-full rounded-full transition-all duration-300"
              :style="progressBarStyle"
            />
          </div>
        </div>
        <ChevronUp class="h-4 w-4 shrink-0 text-muted-foreground" />
      </div>
    </button>

    <Card v-else class="border-border/50 shadow-xl">
      <CardHeader
        class="flex cursor-grab flex-row items-center justify-between space-y-0 pb-3 active:cursor-grabbing"
        @pointerdown="startDrag"
      >
        <CardTitle class="min-w-0 truncate text-base">
          {{ floatingTitle }}
          <span v-if="queuedBehindCount > 0" class="ml-2 rounded-full bg-secondary px-2 py-0.5 text-xs font-medium text-secondary-foreground">
            {{ t('home.queue_count', { count: queuedBehindCount }) }}
          </span>
        </CardTitle>
        <Button variant="ghost" size="icon" class="h-8 w-8 shrink-0" @pointerdown.stop @click="expanded = false">
          <X class="h-4 w-4" />
        </Button>
      </CardHeader>
      <CardContent>
        <div v-if="questsStore.activeQuestId" class="space-y-4">
          <div class="space-y-2">
            <div class="flex items-end justify-between gap-4 text-sm">
              <div class="min-w-0">
                <div class="truncate font-medium">{{ activeQuest?.config.messages.quest_name ?? t('quest.active_progress') }}</div>
                <div class="truncate text-xs text-muted-foreground">{{ activeQuest?.config.messages.game_title }}</div>
                <span class="font-mono text-xs text-muted-foreground">
                  {{ submittedTimeText }}
                </span>
              </div>
              <span class="shrink-0 text-lg font-medium">{{ Math.floor(questsStore.activeQuestProgress) }}%</span>
            </div>

            <div class="relative h-1.5 w-full rounded-full bg-secondary">
              <div
                class="absolute inset-y-0 left-0 rounded-full transition-all duration-300"
                :style="progressBarStyle"
              />
            </div>
            <div class="flex justify-between px-1 text-[10px] text-muted-foreground">
              <div class="flex items-center gap-1">
                <div class="h-2 w-2 rounded-full bg-primary"></div>
                <span>{{ t('quest.submitted') }}</span>
              </div>
              <div class="flex items-center gap-1">
                <div class="h-2 w-2 rounded-full bg-green-400"></div>
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

        <div v-if="queuedUpcoming.length > 0" :class="questsStore.activeQuestId && 'mt-6 border-t pt-4'">
          <div class="mb-2 flex items-center justify-between">
            <h4 class="text-sm font-semibold">{{ t('quest.up_next') }} ({{ queuedUpcoming.length }})</h4>
            <Button
              variant="ghost"
              size="sm"
              class="h-6 px-2 text-destructive hover:text-destructive"
              @click="questsStore.clearQueue"
            >
              {{ t('general.clear') }}
            </Button>
          </div>

          <div class="max-h-[260px] space-y-2 overflow-y-auto pr-1">
            <div
              v-for="(quest, index) in queuedUpcoming"
              :key="quest.id"
              class="flex items-center gap-2 rounded bg-muted/50 p-2 text-sm"
            >
              <span class="w-4 shrink-0 text-xs text-muted-foreground">{{ index + 1 }}.</span>
              <div class="min-w-0 flex-1">
                <div class="truncate font-medium">{{ quest.config.messages.quest_name }}</div>
                <div class="truncate text-xs text-muted-foreground">{{ quest.config.messages.game_title }}</div>
              </div>
            </div>
          </div>
        </div>

        <div v-if="questsStore.error" class="mt-4 flex items-start gap-2 rounded border border-red-500/20 bg-red-500/10 p-3 text-sm text-red-500">
          <AlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
          <span class="break-words">{{ questsStore.error }}</span>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
