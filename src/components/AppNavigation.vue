<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Bug, Gamepad2, Globe2, LayoutDashboard, Settings2, Terminal } from 'lucide-vue-next'
import IconTooltip from './IconTooltip.vue'

export type AppTab = 'home' | 'global' | 'game' | 'console' | 'settings' | 'debug'
const props = defineProps<{ current: AppTab; debugEnabled: boolean; consoleEnabled: boolean; suspended?: boolean }>()
const emit = defineEmits<{ navigate: [tab: AppTab] }>()
const { t } = useI18n()
const items = computed(() => [
  { key: 'home' as const, label: 'nav.home', icon: LayoutDashboard },
  { key: 'global' as const, label: 'nav.global_quests', icon: Globe2 },
  { key: 'game' as const, label: 'nav.game_simulator', icon: Gamepad2 },
  ...(props.consoleEnabled ? [{ key: 'console' as const, label: 'nav.discord_console', icon: Terminal }] : []),
  ...(props.debugEnabled ? [{ key: 'debug' as const, label: 'nav.debug', icon: Bug }] : []),
])
</script>

<template>
  <nav class="app-rail-navigation" :aria-label="t('general.title')">
    <div class="app-primary-links">
      <div class="app-primary-link-items">
        <IconTooltip v-for="item in items" :key="item.key" :label="t(item.label)" :disabled="suspended">
          <button
            type="button"
            :aria-label="t(item.label)"
            :aria-current="current === item.key ? 'page' : undefined"
            :class="['app-rail-button', current === item.key && 'app-rail-button-active']"
            @click="emit('navigate', item.key)"
          >
            <component :is="item.icon" class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" />
          </button>
        </IconTooltip>
      </div>
      <span class="app-rail-divider app-rail-navigation-divider" aria-hidden="true" />
    </div>
    <div class="app-rail-bottom-tools">
      <slot />
      <IconTooltip :label="t('nav.settings')" :disabled="suspended">
        <button type="button" :aria-label="t('nav.settings')" :aria-current="current === 'settings' ? 'page' : undefined" :class="['app-rail-button', current === 'settings' && 'app-rail-button-active']" @click="emit('navigate', 'settings')">
          <Settings2 class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" />
        </button>
      </IconTooltip>
    </div>
  </nav>
</template>
