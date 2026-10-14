<script setup lang="ts">
import { PopoverContent, PopoverPortal, PopoverRoot, PopoverTrigger, TooltipContent, TooltipPortal, TooltipRoot, TooltipTrigger } from 'radix-vue'
import { ref, watch } from 'vue'

const props = defineProps<{ label: string; suspended?: boolean }>()
const open = defineModel<boolean>('open', { default: false })
const tooltipOpen = ref(false)
watch(() => props.suspended, suspended => {
  if (suspended) { open.value = false; tooltipOpen.value = false }
})
</script>

<template>
  <TooltipRoot :disabled="suspended" :open="!suspended && tooltipOpen" @update:open="tooltipOpen = !suspended && $event">
    <PopoverRoot v-model:open="open">
      <PopoverTrigger as-child>
        <TooltipTrigger as-child><slot name="trigger" /></TooltipTrigger>
      </PopoverTrigger>
      <PopoverPortal>
        <PopoverContent side="right" align="end" :side-offset="16" :collision-padding="{ top: 44, right: 12, bottom: 12, left: 12 }" :aria-label="label" class="rail-utility-panel">
          <slot />
        </PopoverContent>
      </PopoverPortal>
      <TooltipPortal>
        <TooltipContent side="right" :side-offset="10" :collision-padding="{ top: 44, right: 12, bottom: 12, left: 12 }" class="ui-popover z-[80] max-w-64 rounded-lg border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md">
          {{ label }}
        </TooltipContent>
      </TooltipPortal>
    </PopoverRoot>
  </TooltipRoot>
</template>
