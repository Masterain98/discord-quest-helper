<script setup lang="ts">
import { ref, computed } from 'vue'
import { Wifi, AlertTriangle, Gamepad2, Check, ArrowUpRight } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import RailPopover from './RailPopover.vue'
import { useQuestsStore } from '@/stores/quests'
import { navigateToTab } from '@/utils/navigate'

const { t } = useI18n()
defineProps<{ suspended?: boolean }>()
const questsStore = useQuestsStore()
const open = ref(false)
const simulateOption = ref<HTMLButtonElement | null>(null)
const cdpOption = ref<HTMLButtonElement | null>(null)

const modeState = computed(() => {
  if (questsStore.gameQuestMode === 'cdp') {
    return {
      tone: questsStore.cdpAvailable ? 'success' : 'warning',
      icon: questsStore.cdpAvailable ? Wifi : AlertTriangle,
      label: t(questsStore.cdpAvailable ? 'header.mode.cdp_connected' : 'header.mode.cdp_disconnected'),
      note: questsStore.cdpAvailable ? null : t('settings.game_mode_cdp_unavailable'),
    }
  }
  if (questsStore.gameQuestMode === 'heartbeat') {
    return {
      tone: 'danger',
      icon: AlertTriangle,
      label: t('header.mode.heartbeat'),
      note: `${t('settings.tooltip_heartbeat_title')} · ${t('settings.tooltip_heartbeat_1')} · ${t('settings.tooltip_heartbeat_3')}`,
    }
  }
  return {
    tone: 'neutral',
    icon: Gamepad2,
    label: t('header.mode.simulate'),
    note: null,
  }
})

function selectMode(mode: 'simulate' | 'cdp') {
  if (mode === 'cdp' && !questsStore.cdpAvailable) {
    open.value = false
    navigateToTab('settings', 'discord_integration')
    return
  }
  questsStore.gameQuestMode = mode
}

function focusSelectedMode(event: Event) {
  const target = questsStore.gameQuestMode === 'cdp' ? cdpOption.value : simulateOption.value
  if (target) {
    event.preventDefault()
    target.focus({ preventScroll: true })
  }
}
</script>

<template>
  <RailPopover v-model:open="open" :suspended="suspended" :label="`${t('sidebar.mode')}: ${modeState.label}`" @open-auto-focus="focusSelectedMode">
    <template #trigger>
      <button type="button" :aria-label="`${t('sidebar.mode')}: ${modeState.label}`" :data-tone="modeState.tone" :class="['app-rail-button rail-tool-button rail-mode-trigger', open && 'rail-tool-button-open']">
        <component :is="modeState.icon" class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" />
      </button>
    </template>
    <div class="rail-panel-heading">
      <span class="rail-panel-glyph ui-icon-tile"><Gamepad2 class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" /></span>
      <h3 class="text-sm font-semibold">{{ t('sidebar.mode') }}</h3>
    </div>
    <p v-if="modeState.note" class="rail-mode-note" :data-tone="modeState.tone" role="status">
      <AlertTriangle class="mt-0.5 h-3.5 w-3.5 shrink-0" :stroke-width="1.7" aria-hidden="true" />
      <span>{{ modeState.note }}</span>
    </p>
    <div class="rail-mode-choices ui-recess" role="group" :aria-label="t('settings.game_quest_mode')">
      <button
        ref="simulateOption"
        type="button"
        class="rail-mode-option"
        :aria-pressed="questsStore.gameQuestMode === 'simulate'"
        @click="selectMode('simulate')"
      >
        <Gamepad2 class="rail-mode-option-icon" :stroke-width="1.7" aria-hidden="true" />
        <span class="rail-mode-option-copy">
          <span class="rail-mode-option-title">{{ t('settings.tooltip_simulate_title') }}</span>
          <span class="rail-mode-option-description">{{ t('settings.tooltip_simulate_1') }}</span>
        </span>
        <span class="rail-mode-selection" aria-hidden="true"><Check v-if="questsStore.gameQuestMode === 'simulate'" /></span>
      </button>
      <button
        ref="cdpOption"
        type="button"
        class="rail-mode-option"
        :aria-pressed="questsStore.gameQuestMode === 'cdp'"
        @click="selectMode('cdp')"
      >
        <Wifi class="rail-mode-option-icon" :stroke-width="1.7" aria-hidden="true" />
        <span class="rail-mode-option-copy">
          <span class="rail-mode-option-title">{{ t('settings.tooltip_cdp_title') }}</span>
          <span class="rail-mode-option-description">{{ t('settings.tooltip_cdp_2') }}</span>
          <span class="rail-mode-connection" :data-connected="questsStore.cdpAvailable">
            <span class="rail-mode-connection-light" aria-hidden="true" />
            {{ questsStore.cdpAvailable ? t('settings.game_mode_cdp_connected') : t('settings.cdp_disconnected_short') }}
            <ArrowUpRight v-if="!questsStore.cdpAvailable" class="h-3 w-3 shrink-0" aria-hidden="true" />
          </span>
        </span>
        <span class="rail-mode-selection" aria-hidden="true"><Check v-if="questsStore.gameQuestMode === 'cdp'" /></span>
      </button>
    </div>
  </RailPopover>
</template>
