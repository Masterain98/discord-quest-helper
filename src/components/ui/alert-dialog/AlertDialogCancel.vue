<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { AlertDialogCancel } from 'radix-vue'
import { cn } from '@/lib/utils'
import { buttonVariants } from '@/components/ui/button'

defineProps<{
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <AlertDialogCancel data-variant="outline" :class="cn(buttonVariants({ variant: 'outline' }), 'mt-2 sm:mt-0', $props.class)">
    <slot />
  </AlertDialogCancel>
</template>

