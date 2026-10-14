<script setup lang="ts">
import { DropdownMenuRoot, DropdownMenuTrigger, TooltipContent, TooltipPortal, TooltipRoot, TooltipTrigger } from 'radix-vue'
import { DropdownMenuContent } from '@/components/ui/dropdown-menu'
import { ref, watch } from 'vue'

const props = withDefaults(defineProps<{ label: string; side?: 'right' | 'bottom'; contentClass?: string; suspended?: boolean }>(), { side: 'bottom', suspended: false })
const open = defineModel<boolean>('open', { default: false })
const tooltipOpen = ref(false)
watch(() => props.suspended, suspended => {
  if (suspended) { open.value = false; tooltipOpen.value = false }
})
</script>

<template>
  <TooltipRoot :disabled="suspended" :open="!suspended && tooltipOpen" @update:open="tooltipOpen = !suspended && $event">
    <DropdownMenuRoot v-model:open="open">
      <!-- Both anchors and both portaled contents share the inner Popper context. -->
      <DropdownMenuTrigger as-child>
        <TooltipTrigger as-child><slot name="trigger" /></TooltipTrigger>
      </DropdownMenuTrigger>
      <DropdownMenuContent :side="side" align="end" :side-offset="side === 'right' ? 16 : 8" :collision-padding="{ top: 44, right: 12, bottom: 12, left: 12 }" :class="contentClass">
        <slot />
      </DropdownMenuContent>
      <TooltipPortal>
        <TooltipContent :side="side" :side-offset="10" :collision-padding="12" class="ui-popover z-[80] max-w-64 rounded-lg border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md">
          {{ label }}
        </TooltipContent>
      </TooltipPortal>
    </DropdownMenuRoot>
  </TooltipRoot>
</template>
