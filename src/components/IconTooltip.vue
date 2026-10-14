<script setup lang="ts">
import { TooltipContent, TooltipPortal, TooltipRoot, TooltipTrigger } from 'radix-vue'
import { ref, watch } from 'vue'
const props = withDefaults(defineProps<{ label: string; side?: 'top' | 'right' | 'bottom' | 'left'; disabled?: boolean }>(), { side: 'right', disabled: false })
const tooltipOpen = ref(false)
watch(() => props.disabled, disabled => { if (disabled) tooltipOpen.value = false })
</script>

<template>
  <TooltipRoot :disabled="disabled" :open="!disabled && tooltipOpen" @update:open="tooltipOpen = !disabled && $event">
    <TooltipTrigger as-child><slot /></TooltipTrigger>
    <TooltipPortal>
      <TooltipContent :side="side" :side-offset="10" :collision-padding="12" class="ui-popover z-[80] max-w-64 rounded-lg border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md">
        {{ label }}
      </TooltipContent>
    </TooltipPortal>
  </TooltipRoot>
</template>
