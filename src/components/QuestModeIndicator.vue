<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useQuestsStore } from '@/stores/quests'
import { Wifi, AlertTriangle, Gamepad2, ChevronDown, Settings } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const questsStore = useQuestsStore()
const emit = defineEmits<{ 'open-settings': [] }>()

const open = ref(false)
const containerRef = ref<HTMLElement | null>(null)

interface ModeState {
  key: string
  tone: 'success' | 'warning' | 'danger' | 'neutral'
  icon: typeof Wifi
  label: string
  title: string
  bullets: string[]
  actionLabel: string
}

const modeState = computed<ModeState>(() => {
  if (questsStore.gameQuestMode === 'cdp') {
    return questsStore.cdpAvailable
      ? {
          key: 'cdp-connected',
          tone: 'success',
          icon: Wifi,
          label: t('header.mode.cdp_connected'),
          title: t('settings.tooltip_cdp_title'),
          bullets: [t('settings.tooltip_cdp_1'), t('settings.tooltip_cdp_2'), t('settings.tooltip_cdp_3')],
          actionLabel: t('header.mode.open_settings'),
        }
      : {
          key: 'cdp-disconnected',
          tone: 'warning',
          icon: AlertTriangle,
          label: t('header.mode.cdp_disconnected'),
          title: t('settings.tooltip_cdp_title'),
          bullets: [t('settings.game_mode_cdp_unavailable'), t('settings.cdp_shortcut_desc')],
          actionLabel: t('header.mode.fix_in_settings'),
        }
  }

  if (questsStore.gameQuestMode === 'heartbeat') {
    return {
      key: 'heartbeat',
      tone: 'danger',
      icon: AlertTriangle,
      label: t('header.mode.heartbeat'),
      title: t('settings.tooltip_heartbeat_title'),
      bullets: [t('settings.tooltip_heartbeat_1'), t('settings.tooltip_heartbeat_2'), t('settings.tooltip_heartbeat_3')],
      actionLabel: t('header.mode.change_mode'),
    }
  }

  return {
    key: 'simulate',
    tone: 'neutral',
    icon: Gamepad2,
    label: t('header.mode.simulate'),
    title: t('settings.tooltip_simulate_title'),
    bullets: [t('settings.tooltip_simulate_1'), t('settings.tooltip_simulate_2'), t('settings.tooltip_simulate_3')],
    actionLabel: t('header.mode.open_settings'),
  }
})

const toneClass = computed(() => {
  switch (modeState.value.tone) {
    case 'success': return 'text-green-600 dark:text-green-400 bg-green-500/10 border-green-500/20 hover:bg-green-500/15'
    case 'warning': return 'text-amber-600 dark:text-amber-400 bg-amber-500/10 border-amber-500/20 hover:bg-amber-500/15'
    case 'danger': return 'text-destructive bg-destructive/10 border-destructive/20 hover:bg-destructive/15'
    default: return 'text-muted-foreground bg-muted/40 border-border hover:bg-muted/60'
  }
})

function handleClickOutside(e: MouseEvent) {
  if (containerRef.value && !containerRef.value.contains(e.target as Node)) {
    open.value = false
  }
}

function handleAction() {
  open.value = false
  emit('open-settings')
}

onMounted(() => document.addEventListener('mousedown', handleClickOutside))
onUnmounted(() => document.removeEventListener('mousedown', handleClickOutside))
</script>

<template>
  <div ref="containerRef" class="relative">
    <button
      class="h-8 px-2.5 rounded-md border inline-flex items-center gap-1.5 text-xs font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring"
      :class="toneClass"
      @click="open = !open"
    >
      <component :is="modeState.icon" class="w-3.5 h-3.5 shrink-0" />
      <span class="hidden sm:inline">{{ modeState.label }}</span>
      <ChevronDown class="w-3 h-3 opacity-60 transition-transform" :class="open && 'rotate-180'" />
    </button>

    <Transition
      enter-active-class="transition ease-out duration-150"
      enter-from-class="opacity-0 -translate-y-1"
      enter-to-class="opacity-100 translate-y-0"
      leave-active-class="transition ease-in duration-100"
      leave-from-class="opacity-100 translate-y-0"
      leave-to-class="opacity-0 -translate-y-1"
    >
      <div
        v-if="open"
        class="absolute right-0 top-full mt-2 z-50 w-72 rounded-lg border bg-popover text-popover-foreground shadow-md p-3 text-xs"
      >
        <p class="text-sm font-semibold mb-1">{{ modeState.title }}</p>

        <ul class="mt-2 space-y-1.5 text-muted-foreground leading-5">
          <li v-for="item in modeState.bullets" :key="item" class="flex gap-1.5">
            <span class="shrink-0">•</span>
            <span>{{ item }}</span>
          </li>
        </ul>

        <button
          class="mt-3 w-full inline-flex items-center justify-center gap-2 h-8 rounded-md border bg-background text-sm font-medium hover:bg-accent hover:text-accent-foreground transition-colors"
          @click="handleAction"
        >
          <Settings class="w-3.5 h-3.5" />
          {{ modeState.actionLabel }}
        </button>
      </div>
    </Transition>
  </div>
</template>
