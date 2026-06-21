<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'

const props = defineProps<{
  title: string
  description?: string
  defaultOpen?: boolean
}>()

const open = ref(props.defaultOpen ?? false)
</script>

<template>
  <div class="rounded-lg border border-border">
    <Button
      type="button"
      variant="ghost"
      class="h-auto w-full justify-between gap-3 whitespace-normal px-4 py-3 text-left"
      @click="open = !open"
    >
      <span class="min-w-0">
        <span class="block text-sm font-medium">{{ title }}</span>
        <span v-if="description" class="mt-1 block text-xs font-normal text-muted-foreground">{{ description }}</span>
      </span>
      <ChevronDown :class="['h-4 w-4 shrink-0 transition-transform', open && 'rotate-180']" />
    </Button>
    <div v-if="open" class="border-t border-border/60 px-4 py-4">
      <slot />
    </div>
  </div>
</template>
