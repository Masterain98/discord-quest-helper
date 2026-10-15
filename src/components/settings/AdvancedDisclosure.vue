<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { SettingsTone } from './settingTones'

const props = withDefaults(defineProps<{
  title: string
  description?: string
  defaultOpen?: boolean
  tone?: SettingsTone
}>(), {
  tone: 'neutral',
})

const open = ref(props.defaultOpen ?? false)
</script>

<template>
  <div class="settings-disclosure overflow-hidden rounded-xl border" data-settings-search data-settings-disclosure :data-search-label="title" :data-search-description="description">
    <Button
      type="button"
      variant="ghost"
      :aria-expanded="open"
      class="h-auto w-full justify-between gap-3 whitespace-normal rounded-none px-4 py-3 text-left hover:bg-muted/40"
      @click="open = !open"
    >
      <span class="min-w-0">
        <span class="block text-sm font-semibold">{{ title }}</span>
        <span v-if="description" class="mt-1 block text-xs font-normal text-muted-foreground">{{ description }}</span>
      </span>
      <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground">
        <ChevronDown :class="cn('h-4 w-4 transition-transform', open && 'rotate-180')" />
      </span>
    </Button>
    <div v-show="open" class="ui-recess border-t border-border/60 px-4 py-4">
      <slot />
    </div>
  </div>
</template>
