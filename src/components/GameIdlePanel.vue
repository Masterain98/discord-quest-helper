<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  ChevronDown,
  Clock3,
  Gamepad2,
  Loader2,
  MonitorPlay,
  MoreHorizontal,
  Play,
  RotateCcw,
  Square,
  TimerReset,
  Trash2,
} from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/stores/auth'
import { useGameIdleStore, formatSimulationDuration } from '@/stores/gameIdle'
import { useQuestsStore } from '@/stores/quests'
import { getSimulationExecutables } from '@/utils/executables'
import type { GameIdleItem } from '@/api/tauri'

const { t } = useI18n()
const auth = useAuthStore()
const idle = useGameIdleStore()
const quests = useQuestsStore()
const stage = ref<HTMLElement | null>(null)
const stageWidth = ref(900)
const now = ref(Date.now())
const contextMenu = ref<{ item: GameIdleItem; x: number; y: number } | null>(null)
let clockTimer: ReturnType<typeof setInterval> | null = null
let resizeObserver: ResizeObserver | null = null

const active = computed(() => idle.isActive)
const immersive = computed(() => active.value || idle.loading)
const games = computed(() => quests.detectableGames)
const uniqueGameCount = computed(() => new Set(games.value.map(game => game.id)).size)
const processCandidateCount = computed(() => {
  const capabilities = quests.platformCapabilities
  if (!capabilities) return 0
  return new Set(games.value.filter(game => getSimulationExecutables(
      game.executables,
      capabilities.os,
      capabilities.executableOsPriority
    ).length > 0).map(game => game.id)).size
})
const candidateCount = computed(() => idle.mode === 'cdp' ? uniqueGameCount.value : processCandidateCount.value)

const maxVisibleDistance = computed(() => {
  if (stageWidth.value >= 1050) return 5
  if (stageWidth.value >= 820) return 4
  if (stageWidth.value >= 620) return 3
  return 2
})

const carouselItems = computed(() => {
  const status = idle.status
  if (!status) return []
  const recent = status.recent.slice(-5)
  const currentId = status.current?.id
  const items: Array<{ item: GameIdleItem; offset: number; upcoming: boolean }> = []
  if (recent.length > 0) {
    recent.forEach((item, index) => {
      items.push({ item, offset: index - recent.length + 1, upcoming: false })
    })
  } else if (status.current) {
    items.push({ item: status.current, offset: 0, upcoming: false })
  }
  status.upcoming.forEach((item, index) => {
    if (currentIsQueued.value && index === 0) return
    if (item.id !== currentId || index > 0) {
      items.push({ item, offset: currentIsQueued.value ? index : index + 1, upcoming: true })
    }
  })
  return items.filter(entry => Math.abs(entry.offset) <= maxVisibleDistance.value)
})

const phaseRemainingSeconds = computed(() => {
  const endsAt = idle.status?.phaseEndsAt
  if (!endsAt) return 0
  return Math.max(0, Math.ceil((endsAt - now.value) / 1000))
})

const sessionSeconds = computed(() => {
  const status = idle.status
  if (!status) return 0
  if (status.phase !== 'playing') return status.accumulatedPlayedSeconds
  const elapsed = Math.max(0, Math.floor((now.value - status.phaseStartedAt) / 1000))
  return status.accumulatedPlayedSeconds + elapsed
})

const sessionDuration = computed(() => formatSimulationDuration(sessionSeconds.value))
const configError = computed(() => idle.validateConfig())
const currentIsQueued = computed(() => Boolean(
  idle.status?.current &&
  idle.status.upcoming[0]?.occurrenceId === idle.status.current.occurrenceId
))
const nextGameName = computed(() => idle.status?.upcoming[currentIsQueued.value ? 1 : 0]?.name || t('game_idle.preparing'))

function formatCountdown(seconds: number) {
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const remainder = seconds % 60
  const base = `${String(minutes).padStart(2, '0')}:${String(remainder).padStart(2, '0')}`
  return hours > 0 ? `${hours}:${base}` : base
}

function iconUrl(item: GameIdleItem) {
  return item.icon ? `https://cdn.discordapp.com/app-icons/${item.id}/${item.icon}.png?size=128` : ''
}

async function startIdle() {
  if (configError.value) return
  await idle.start().catch(() => undefined)
}

async function stopIdle() {
  await idle.stop().catch(() => undefined)
}

function openContextMenu(event: MouseEvent | KeyboardEvent, item: GameIdleItem) {
  event.preventDefault()
  const target = event.currentTarget as HTMLElement
  const rect = target.getBoundingClientRect()
  const mouse = event instanceof MouseEvent
  contextMenu.value = {
    item,
    x: Math.min(window.innerWidth - 232, Math.max(8, mouse && event.clientX ? event.clientX : rect.right - 8)),
    y: Math.min(window.innerHeight - 104, Math.max(8, mouse && event.clientY ? event.clientY : rect.bottom - 8)),
  }
  void nextTick(() => document.querySelector<HTMLElement>('[data-idle-context-action]')?.focus())
}

function handleFutureKeydown(event: KeyboardEvent, item: GameIdleItem) {
  if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
    openContextMenu(event, item)
  }
}

async function removeContextItem() {
  const item = contextMenu.value?.item
  contextMenu.value = null
  if (item) await idle.removeUpcoming(item).catch(() => undefined)
}

function closeContextMenu(event?: Event) {
  const target = event?.target as HTMLElement | null
  if (target?.closest('[data-idle-context-menu]')) return
  contextMenu.value = null
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') contextMenu.value = null
}

onMounted(async () => {
  clockTimer = setInterval(() => { now.value = Date.now() }, 1000)
  resizeObserver = new ResizeObserver(entries => {
    stageWidth.value = entries[0]?.contentRect.width ?? stageWidth.value
  })
  if (stage.value) resizeObserver.observe(stage.value)
  document.addEventListener('mousedown', closeContextMenu)
  document.addEventListener('keydown', handleDocumentKeydown)
  await idle.initialize()
  await quests.getDetectableGames().catch(() => undefined)
})

onBeforeUnmount(() => {
  if (clockTimer) clearInterval(clockTimer)
  resizeObserver?.disconnect()
  document.removeEventListener('mousedown', closeContextMenu)
  document.removeEventListener('keydown', handleDocumentKeydown)
})
</script>

<template>
  <section class="idle-shell space-y-5" :class="immersive && 'is-immersive'" aria-labelledby="game-idle-heading">
    <div v-if="!immersive" class="idle-controls grid gap-5 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
      <div>
        <div class="flex items-center gap-2 text-primary">
          <RotateCcw class="h-4 w-4" />
          <span class="text-xs font-semibold tracking-[0.16em] uppercase">{{ t('game_idle.eyebrow') }}</span>
        </div>
        <h3 id="game-idle-heading" class="mt-2 text-2xl font-semibold tracking-tight">
          {{ t('game_idle.title') }}
        </h3>
        <p class="mt-1 max-w-2xl text-sm leading-6 text-muted-foreground">
          {{ t('game_idle.description') }}
        </p>
      </div>

      <div class="flex flex-wrap items-end gap-3">
        <div class="space-y-1.5">
          <Label for="idle-play-minutes" class="text-xs">{{ t('game_idle.play_minutes') }}</Label>
          <Input
            id="idle-play-minutes"
            v-model.number="idle.playMinutes"
            type="number"
            inputmode="numeric"
            min="1"
            step="1"
            class="w-28 tabular-nums"
            :disabled="active"
            :aria-invalid="configError === 'play_minutes'"
          />
        </div>
        <div class="space-y-1.5">
          <Label for="idle-rest-minutes" class="text-xs">{{ t('game_idle.rest_minutes') }}</Label>
          <Input
            id="idle-rest-minutes"
            v-model.number="idle.restMinutes"
            type="number"
            inputmode="numeric"
            min="0"
            step="1"
            class="w-28 tabular-nums"
            :disabled="active"
            :aria-invalid="configError === 'rest_minutes'"
          />
        </div>
        <div class="space-y-1.5">
          <Label class="text-xs">{{ t('game_idle.simulation_mode') }}</Label>
          <div class="flex h-10 rounded-md border bg-background p-1">
            <button
              type="button"
              class="idle-mode-button"
              :class="idle.mode === 'process' && 'is-active'"
              :disabled="active"
              @click="idle.mode = 'process'"
            >
              <Gamepad2 class="h-3.5 w-3.5" />
              {{ t('game_idle.process') }}
            </button>
            <button
              type="button"
              class="idle-mode-button"
              :class="idle.mode === 'cdp' && 'is-active'"
              :disabled="active || !quests.cdpAvailable"
              @click="idle.mode = 'cdp'"
            >
              <MonitorPlay class="h-3.5 w-3.5" />
              CDP
            </button>
          </div>
        </div>
        <Button
          v-if="!active"
          class="h-10 min-w-28 gap-2"
          :disabled="idle.loading || !!configError || !auth.user || candidateCount === 0 || !!quests.activeQuestId"
          @click="startIdle"
        >
          <Loader2 v-if="idle.loading" class="h-4 w-4 animate-spin" />
          <Play v-else class="h-4 w-4 fill-current" />
          {{ idle.loading ? t('game_idle.starting') : t('game_idle.start') }}
        </Button>
        <Button v-else variant="destructive" class="h-10 min-w-28 gap-2" :disabled="idle.stopping" @click="stopIdle">
          <Loader2 v-if="idle.stopping" class="h-4 w-4 animate-spin" />
          <Square v-else class="h-4 w-4 fill-current" />
          {{ idle.stopping ? t('game_idle.stopping') : t('game_idle.stop') }}
        </Button>
      </div>
    </div>

    <div v-if="!immersive" class="flex flex-wrap items-center gap-x-5 gap-y-1 text-xs text-muted-foreground">
      <span>{{ t('game_idle.candidate_count', { count: candidateCount }) }}</span>
      <span v-if="!auth.user" class="text-amber-600 dark:text-amber-300">{{ t('game_idle.sign_in_required') }}</span>
      <span v-else-if="configError" class="text-destructive">{{ t(`game_idle.${configError}_error`) }}</span>
      <span v-else-if="idle.mode === 'cdp' && !quests.cdpAvailable" class="text-amber-600 dark:text-amber-300">{{ t('game_idle.cdp_unavailable') }}</span>
    </div>

    <div v-if="idle.error || idle.status?.warning" class="idle-warning" role="status">
      {{ idle.error || idle.status?.warning }}
    </div>

    <div v-if="idle.status?.current || idle.status?.upcoming.length" class="idle-machine">
      <div ref="stage" class="idle-stage" :aria-label="t('game_idle.queue_label')">
        <div class="idle-pointer" aria-hidden="true">
          <span>{{ idle.status.phase === 'resting' ? t('game_idle.resting') : idle.status.phase === 'starting' ? t('game_idle.starting') : t('game_idle.current') }}</span>
          <ChevronDown class="h-6 w-6 fill-current" />
        </div>

        <TransitionGroup name="idle-reel">
          <button
            v-for="entry in carouselItems"
            :key="entry.item.occurrenceId"
            type="button"
            class="idle-reel-item"
            :class="[entry.offset === 0 && 'is-current', entry.upcoming && 'is-upcoming']"
            :style="{
              '--idle-offset': entry.offset,
              '--idle-distance': Math.abs(entry.offset),
            }"
            :aria-label="entry.upcoming && idle.status.phase !== 'starting' ? t('game_idle.upcoming_item', { name: entry.item.name }) : entry.item.name"
            @contextmenu="entry.upcoming && idle.status.phase !== 'starting' && openContextMenu($event, entry.item)"
            @keydown="entry.upcoming && idle.status.phase !== 'starting' && handleFutureKeydown($event, entry.item)"
          >
            <span class="idle-icon-frame">
              <img
                v-if="iconUrl(entry.item)"
                :src="iconUrl(entry.item)"
                :alt="entry.item.name"
                draggable="false"
                @error="($event.target as HTMLImageElement).style.display = 'none'"
              />
              <Gamepad2 class="idle-icon-fallback" aria-hidden="true" />
            </span>
            <span class="idle-icon-name">{{ entry.item.name }}</span>
            <MoreHorizontal v-if="entry.upcoming" class="idle-more h-4 w-4" aria-hidden="true" />
          </button>
        </TransitionGroup>
      </div>

      <div class="idle-readout">
        <div class="idle-primary-readout">
          <span class="idle-readout-label">{{ idle.status.phase === 'resting' ? t('game_idle.resting_after') : idle.status.phase === 'starting' ? t('game_idle.starting') : t('game_idle.now_playing') }}</span>
          <strong>{{ idle.status.current?.name || idle.status.upcoming[0]?.name || t('game_idle.preparing') }}</strong>
          <span class="idle-next">{{ t('game_idle.next') }} · {{ nextGameName }}</span>
        </div>
        <div class="idle-metric">
          <TimerReset class="h-4 w-4" />
          <span>{{ idle.status.phase === 'resting' ? t('game_idle.rest_remaining') : t('game_idle.play_remaining') }}</span>
          <strong>{{ formatCountdown(phaseRemainingSeconds) }}</strong>
          <small>{{ t('game_idle.configured_for', { count: idle.status.playMinutes }) }}</small>
        </div>
        <div class="idle-metric idle-total">
          <Clock3 class="h-4 w-4" />
          <span>{{ t('game_idle.session_total') }}</span>
          <strong>{{ t('game_idle.duration', { hours: sessionDuration.hours, minutes: sessionDuration.minutes }) }}</strong>
          <small>{{ t('game_idle.active_time_only') }}</small>
        </div>
      </div>

      <div v-if="active" class="idle-immersive-footer">
        <div class="idle-immersive-status">
          <span class="idle-live-dot" aria-hidden="true" />
          <span>{{ idle.status?.phase === 'resting' ? t('game_idle.resting') : t('game_idle.now_playing') }}</span>
        </div>
        <Button variant="destructive" class="idle-stop-button h-11 min-w-36 gap-2" :disabled="idle.stopping" @click="stopIdle">
          <Loader2 v-if="idle.stopping" class="h-4 w-4 animate-spin" />
          <Square v-else class="h-4 w-4 fill-current" />
          {{ idle.stopping ? t('game_idle.stopping') : t('game_idle.stop') }}
        </Button>
      </div>
    </div>

    <div v-else-if="immersive" class="idle-starting-surface">
      <div class="idle-empty-icon"><Loader2 class="h-7 w-7 animate-spin" /></div>
      <div class="idle-starting-copy">
        <h4 class="font-medium">{{ t('game_idle.starting') }}</h4>
        <p class="mt-1 text-sm text-muted-foreground">{{ t('game_idle.preparing') }}</p>
      </div>
      <Button variant="destructive" class="idle-stop-button h-11 min-w-32 gap-2" :disabled="idle.stopping" @click="stopIdle">
        <Loader2 v-if="idle.stopping" class="h-4 w-4 animate-spin" />
        <Square v-else class="h-4 w-4 fill-current" />
        {{ idle.stopping ? t('game_idle.stopping') : t('game_idle.stop') }}
      </Button>
    </div>

    <div v-else class="idle-empty">
      <div class="idle-empty-icon">
        <Loader2 v-if="active" class="h-7 w-7 animate-spin" />
        <RotateCcw v-else class="h-7 w-7" />
      </div>
      <div>
        <h4 class="font-medium">{{ immersive ? t('game_idle.starting') : t('game_idle.ready_title') }}</h4>
        <p class="mt-1 text-sm text-muted-foreground">{{ immersive ? t('game_idle.preparing') : t('game_idle.ready_description') }}</p>
      </div>
    </div>

    <Teleport to="body">
      <Transition name="idle-context">
        <div
          v-if="contextMenu"
          data-idle-context-menu
          class="idle-context-menu"
          :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
          role="menu"
        >
          <div class="idle-context-title">{{ contextMenu.item.name }}</div>
          <button data-idle-context-action type="button" role="menuitem" @click="removeContextItem">
            <Trash2 class="h-4 w-4" />
            {{ t('game_idle.remove_from_cycle') }}
          </button>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.idle-shell {
  border: 1px solid hsl(var(--border) / 0.7);
  border-radius: 1.15rem;
  padding: clamp(1rem, 2.4vw, 1.75rem);
  background:
    radial-gradient(circle at 18% 0%, hsl(var(--primary) / 0.11), transparent 32rem),
    hsl(var(--card) / 0.76);
  box-shadow: inset 0 1px 0 hsl(var(--foreground) / 0.04), 0 24px 60px -48px hsl(var(--primary) / 0.7);
  overflow: hidden;
}

.idle-shell.is-immersive {
  min-height: calc(100dvh - 2rem);
  display: flex;
  flex-direction: column;
  justify-content: center;
  border: 0;
  border-radius: 0;
  padding: clamp(1rem, 4vw, 4rem);
  background:
    radial-gradient(circle at 50% 38%, hsl(var(--primary) / 0.15), transparent 30rem),
    hsl(var(--background));
  box-shadow: none;
}

.idle-shell.is-immersive .idle-machine {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  min-height: min(42rem, 76dvh);
  background: hsl(var(--card) / 0.52);
  box-shadow: inset 0 1px 0 hsl(var(--foreground) / 0.06), 0 32px 80px -56px hsl(var(--primary) / 0.9);
}

.idle-shell.is-immersive .idle-stage {
  height: clamp(18rem, 48dvh, 32rem);
}

.idle-immersive-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  border-top: 1px solid hsl(var(--border) / 0.55);
  padding: 1rem 1.25rem 1.15rem;
}

.idle-immersive-status {
  display: inline-flex;
  align-items: center;
  gap: 0.55rem;
  color: hsl(var(--muted-foreground));
  font-size: 0.72rem;
  font-weight: 600;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.idle-live-dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 999px;
  background: hsl(var(--destructive));
  box-shadow: 0 0 0 0.25rem hsl(var(--destructive) / 0.12);
}

.idle-stop-button {
  transition: transform 280ms cubic-bezier(.32,.72,0,1), box-shadow 280ms cubic-bezier(.32,.72,0,1);
}

.idle-stop-button:hover:not(:disabled) {
  transform: translateY(-2px);
  box-shadow: 0 12px 28px -18px hsl(var(--destructive));
}

.idle-stop-button:active:not(:disabled) { transform: translateY(0) scale(.98); }

.idle-controls { position: relative; z-index: 2; }
.idle-mode-button { display: inline-flex; align-items: center; gap: .35rem; border-radius: .28rem; padding: 0 .7rem; font-size: .75rem; color: hsl(var(--muted-foreground)); transition: color 180ms ease, background 180ms ease, transform 120ms ease; }
.idle-mode-button:hover:not(:disabled) { color: hsl(var(--foreground)); }
.idle-mode-button:active:not(:disabled) { transform: translateY(1px); }
.idle-mode-button:focus-visible { outline: 2px solid hsl(var(--ring)); outline-offset: 2px; }
.idle-mode-button.is-active { background: hsl(var(--primary)); color: hsl(var(--primary-foreground)); box-shadow: 0 5px 16px -10px hsl(var(--primary)); }
.idle-mode-button:disabled { cursor: not-allowed; opacity: .5; }
.idle-warning { border-left: 3px solid hsl(38 92% 55%); background: hsl(38 92% 55% / .09); padding: .75rem 1rem; color: hsl(var(--foreground)); font-size: .8rem; }

.idle-machine { border-radius: 1rem; background: hsl(var(--background) / .48); box-shadow: inset 0 0 0 1px hsl(var(--border) / .5); overflow: hidden; }
.idle-stage { position: relative; height: clamp(14rem, 28vw, 18.5rem); isolation: isolate; overflow: hidden; -webkit-mask-image: linear-gradient(90deg, transparent, black 11%, black 89%, transparent); mask-image: linear-gradient(90deg, transparent, black 11%, black 89%, transparent); }
.idle-stage::before { content: ''; position: absolute; inset: 20% 0 0; background: radial-gradient(ellipse at 50% 45%, hsl(var(--primary) / .13), transparent 36%); pointer-events: none; }
.idle-pointer { position: absolute; z-index: 4; top: clamp(2.1rem, 14%, 4.2rem); left: 50%; display: flex; flex-direction: column; align-items: center; color: hsl(var(--primary)); transform: translateX(-50%); }
.idle-pointer span { font-size: clamp(.82rem, 1.4vw, 1.05rem); font-weight: 750; letter-spacing: .12em; text-transform: uppercase; }

.idle-reel-item { --item-gap: clamp(9rem, 16vw, 13rem); position: absolute; z-index: calc(5 - var(--idle-distance)); top: 50%; left: 50%; display: grid; justify-items: center; gap: .55rem; width: clamp(5.3rem, 9vw, 7.5rem); color: hsl(var(--foreground)); opacity: calc(1 - var(--idle-distance) * .16); transform: translate3d(calc(-50% + var(--idle-offset) * var(--item-gap)), -43%, 0) scale(calc(1 - var(--idle-distance) * .1)); filter: saturate(calc(1 - var(--idle-distance) * .1)); transition: transform 440ms cubic-bezier(.22,1,.36,1), opacity 360ms ease, filter 360ms ease; }
.idle-reel-item:focus-visible { outline: none; }
.idle-reel-item:focus-visible .idle-icon-frame { box-shadow: 0 0 0 3px hsl(var(--ring)); }
.idle-icon-frame { position: relative; display: grid; place-items: center; width: 100%; aspect-ratio: 1; border-radius: clamp(.8rem, 1.5vw, 1.3rem); overflow: hidden; background: linear-gradient(145deg, hsl(var(--secondary)), hsl(var(--muted))); box-shadow: inset 0 1px 0 hsl(var(--foreground) / .12), 0 20px 32px -24px hsl(var(--foreground) / .65); transition: box-shadow 220ms ease, transform 220ms ease; }
.idle-icon-frame img { position: relative; z-index: 1; width: 100%; height: 100%; object-fit: cover; }
.idle-icon-fallback { position: absolute; width: 38%; height: 38%; color: hsl(var(--muted-foreground) / .58); }
.idle-icon-name { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: .72rem; font-weight: 600; }
.idle-reel-item.is-current { z-index: 3; opacity: 1; filter: none; transform: translate3d(-50%, -40%, 0) scale(1.22); }
.idle-reel-item.is-current .idle-icon-frame { box-shadow: 0 0 0 1px hsl(var(--primary) / .55), 0 25px 45px -26px hsl(var(--primary)); }
.idle-reel-item.is-upcoming:hover .idle-icon-frame { transform: translateY(-3px); box-shadow: inset 0 1px 0 hsl(var(--foreground) / .14), 0 24px 36px -24px hsl(var(--primary)); }
.idle-more { position: absolute; right: .2rem; top: -.1rem; opacity: 0; color: hsl(var(--muted-foreground)); transition: opacity 180ms ease; }
.idle-reel-item.is-upcoming:hover .idle-more, .idle-reel-item.is-upcoming:focus-visible .idle-more { opacity: 1; }
.idle-reel-enter-active, .idle-reel-leave-active { transition: opacity 260ms ease, transform 440ms cubic-bezier(.22,1,.36,1), filter 440ms cubic-bezier(.22,1,.36,1); }
.idle-reel-enter-from { opacity: 0; transform: translate3d(calc(-50% + (var(--idle-offset) + 1) * var(--item-gap)), -43%, 0) scale(.65); }
.idle-reel-leave-to { opacity: 0; filter: blur(4px); transform: translate3d(calc(-50% + var(--idle-offset) * var(--item-gap)), -43%, 0) scale(.3); }

.idle-readout { display: grid; grid-template-columns: minmax(0, 1.5fr) repeat(2, minmax(10rem, .75fr)); border-top: 1px solid hsl(var(--border) / .55); background: hsl(var(--card) / .65); }
.idle-primary-readout, .idle-metric { min-width: 0; padding: 1rem 1.2rem 1.15rem; }
.idle-primary-readout { display: flex; flex-direction: column; }
.idle-readout-label, .idle-metric span { color: hsl(var(--muted-foreground)); font-size: .68rem; font-weight: 600; letter-spacing: .08em; text-transform: uppercase; }
.idle-primary-readout strong { margin-top: .2rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: clamp(1.2rem, 2vw, 1.75rem); letter-spacing: -.03em; }
.idle-next { margin-top: .3rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: hsl(var(--muted-foreground)); font-size: .75rem; }
.idle-metric { display: grid; grid-template-columns: auto 1fr; align-items: center; column-gap: .45rem; border-left: 1px solid hsl(var(--border) / .55); }
.idle-metric > svg { color: hsl(var(--primary)); }
.idle-metric strong, .idle-metric small { grid-column: 1 / -1; }
.idle-metric strong { margin-top: .42rem; font-variant-numeric: tabular-nums; font-size: 1.32rem; letter-spacing: -.025em; }
.idle-metric small { margin-top: .12rem; color: hsl(var(--muted-foreground)); font-size: .68rem; }
.idle-empty { display: flex; min-height: 12rem; align-items: center; justify-content: center; gap: 1rem; border: 1px dashed hsl(var(--border)); border-radius: 1rem; background: hsl(var(--muted) / .18); }
.idle-empty-icon { display: grid; place-items: center; width: 3.4rem; height: 3.4rem; border-radius: 1rem; background: hsl(var(--primary) / .11); color: hsl(var(--primary)); }
.idle-starting-surface { display: flex; min-height: 12rem; align-items: center; justify-content: center; gap: 1rem; border-radius: 1rem; background: hsl(var(--card) / .52); }
.idle-starting-copy { min-width: 0; }

.idle-context-menu { position: fixed; z-index: 80; width: 14rem; overflow: hidden; border: 1px solid hsl(var(--border)); border-radius: .65rem; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); box-shadow: 0 18px 50px -20px hsl(var(--foreground) / .45); transform: translateY(3px); }
.idle-context-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border-bottom: 1px solid hsl(var(--border) / .65); padding: .65rem .75rem; color: hsl(var(--muted-foreground)); font-size: .7rem; }
.idle-context-menu button { display: flex; width: 100%; align-items: center; gap: .55rem; padding: .65rem .75rem; font-size: .8rem; transition: background 150ms ease, color 150ms ease; }
.idle-context-menu button:hover, .idle-context-menu button:focus-visible { outline: none; background: hsl(var(--destructive) / .12); color: hsl(var(--destructive)); }
.idle-context-enter-active, .idle-context-leave-active { transition: opacity 150ms ease, transform 180ms cubic-bezier(.22,1,.36,1); }
.idle-context-enter-from, .idle-context-leave-to { opacity: 0; transform: translateY(-2px) scale(.97); }

@media (max-width: 860px) {
  .idle-readout { grid-template-columns: 1fr 1fr; }
  .idle-primary-readout { grid-column: 1 / -1; border-bottom: 1px solid hsl(var(--border) / .55); }
  .idle-metric:first-of-type { border-left: 0; }

  .idle-shell.is-immersive { min-height: calc(100dvh - 1rem); padding: 0.75rem; }
  .idle-shell.is-immersive .idle-machine { min-height: min(38rem, 78dvh); }
}

@media (max-width: 560px) {
  .idle-immersive-footer { align-items: stretch; flex-direction: column; }
  .idle-stop-button { width: 100%; }
  .idle-starting-surface { align-items: stretch; flex-direction: column; padding: 2rem 1rem; }
}

@media (prefers-reduced-motion: reduce) {
  .idle-reel-item, .idle-reel-enter-active, .idle-reel-leave-active, .idle-context-enter-active, .idle-context-leave-active { transition-duration: 1ms !important; }
}
</style>
