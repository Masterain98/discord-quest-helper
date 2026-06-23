<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { settingToneClass, type SettingsTone } from './settingTones'

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
  <div :class="cn('overflow-hidden rounded-lg border bg-card/60', settingToneClass[tone].card)">
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
      <span :class="cn('flex h-7 w-7 shrink-0 items-center justify-center rounded-md', settingToneClass[tone].icon)">
        <ChevronDown :class="cn('h-4 w-4 transition-transform', open && 'rotate-180')" />
      </span>
    </Button>
    <div v-if="open" class="border-t border-border/60 bg-background/40 px-4 py-4">
      <slot />
    </div>
  </div>
</template>
