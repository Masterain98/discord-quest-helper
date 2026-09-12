<script setup lang="ts">
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useI18n } from 'vue-i18n'

export type AppTab = 'home' | 'game' | 'settings' | 'debug'

const props = withDefaults(defineProps<{
  current: AppTab
  debugEnabled: boolean
  /**
   * Locks navigation while a long-running exclusive activity (game idle) owns
   * the app. Disabled entries stay visible but cannot navigate away.
   */
  disabled?: boolean
  disabledHint?: string
}>(), {
  disabled: false,
  disabledHint: undefined,
})

const emit = defineEmits<{
  navigate: [tab: AppTab]
}>()

const { t } = useI18n()

const items = [
  { key: 'home' as const, label: 'nav.home' },
  { key: 'game' as const, label: 'nav.game_simulator' },
  { key: 'settings' as const, label: 'nav.settings' },
  { key: 'debug' as const, label: 'nav.debug', debugOnly: true },
]

function onNavigate(tab: AppTab) {
  if (props.disabled) return
  emit('navigate', tab)
}
</script>

<template>
  <nav class="flex min-w-0 items-center gap-1 overflow-x-auto" :aria-label="t('general.title')">
    <Button
      v-for="item in items"
      v-show="!item.debugOnly || props.debugEnabled"
      :key="item.key"
      size="sm"
      variant="ghost"
      :disabled="props.disabled"
      :title="props.disabled ? props.disabledHint : undefined"
      :aria-current="props.current === item.key ? 'page' : undefined"
      :class="cn(
        'relative shrink-0 rounded-md px-3 text-muted-foreground transition-colors',
        'hover:bg-primary/[0.055] hover:text-foreground',
        props.current === item.key && [
          'bg-primary/[0.075] text-primary hover:bg-primary/[0.1] hover:text-primary',
          'shadow-[inset_0_-2px_0_hsl(var(--primary))]',
        ],
      )"
      @click="onNavigate(item.key)"
    >
      {{ t(item.label) }}
    </Button>
  </nav>
</template>
