<script setup lang="ts">
import { ref, computed } from 'vue'
import RailPopover from './RailPopover.vue'
import { useQuestsStore } from '@/stores/quests'
import { Wifi, AlertTriangle, Gamepad2, Settings, ArrowUpRight } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
defineProps<{ suspended?: boolean }>()
const questsStore = useQuestsStore()
const emit = defineEmits<{ 'open-settings': [] }>()

const open = ref(false)


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



function handleAction() {
  open.value = false
  emit('open-settings')
}

</script>

<template>
  <RailPopover v-model:open="open" :suspended="suspended" :label="`${t('sidebar.mode')}: ${modeState.label}`">
    <template #trigger>
      <button type="button" :aria-label="`${t('sidebar.mode')}: ${modeState.label}`" :data-tone="modeState.tone" :class="['app-rail-button rail-tool-button rail-mode-trigger', open && 'rail-tool-button-open']">
        <component :is="modeState.icon" class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" />
        <span class="rail-mode-light" aria-hidden="true" />
      </button>
    </template>
    <div class="rail-panel-heading">
      <span class="rail-panel-glyph"><component :is="modeState.icon" class="h-4 w-4" aria-hidden="true" /></span>
      <h3 class="text-sm font-semibold">{{ t('sidebar.mode') }}</h3>
    </div>
    <div class="rail-mode-summary" :data-tone="modeState.tone" role="status">
      <span class="rail-mode-light" aria-hidden="true" />
      <span class="text-sm font-semibold">{{ modeState.label }}</span>
    </div>
    <p class="mt-4 text-xs font-medium">{{ modeState.title }}</p>
    <ul class="mt-2 space-y-2 text-xs leading-relaxed text-muted-foreground">
      <li v-for="item in modeState.bullets" :key="item" class="flex gap-2"><span class="mt-[7px] h-1 w-1 shrink-0 rounded-full bg-primary/45" aria-hidden="true" /><span>{{ item }}</span></li>
    </ul>
    <button type="button" class="rail-panel-link" @click="handleAction">
      <Settings class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" />{{ modeState.actionLabel }}
      <ArrowUpRight class="ml-auto h-3.5 w-3.5 shrink-0" aria-hidden="true" />
    </button>
  </RailPopover>
</template>
