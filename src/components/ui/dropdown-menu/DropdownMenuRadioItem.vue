<script setup lang="ts">
import { computed, type HTMLAttributes } from 'vue'
import { Check } from 'lucide-vue-next'
import { DropdownMenuItemIndicator, DropdownMenuRadioItem, type DropdownMenuRadioItemProps, useForwardProps } from 'radix-vue'
import { cn } from '@/lib/utils'

const props = defineProps<DropdownMenuRadioItemProps & { class?: HTMLAttributes['class'] }>()
const forwarded = useForwardProps(computed(() => {
  const { class: _, ...delegated } = props
  return delegated
}))
</script>

<template>
  <DropdownMenuRadioItem
    v-bind="forwarded"
    :class="cn('relative flex cursor-default select-none items-center gap-3 rounded-sm px-2 py-1.5 text-sm outline-none transition-colors focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50', props.class)"
  >
    <span class="min-w-0 flex-1 truncate"><slot /></span>
    <span class="flex h-4 w-4 shrink-0 items-center justify-center">
      <DropdownMenuItemIndicator><Check class="h-4 w-4 text-primary" aria-hidden="true" /></DropdownMenuItemIndicator>
    </span>
  </DropdownMenuRadioItem>
</template>
