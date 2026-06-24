<script setup lang="ts">
defineProps<{
  label: string
  description?: string
}>()
</script>

<template>
  <div class="grid gap-3 border-b border-border/50 py-4 last:border-b-0 md:grid-cols-[1fr_auto] md:items-center">
    <div class="min-w-0">
      <p class="text-sm font-medium">{{ label }}</p>
      <p v-if="description" class="mt-1 max-w-2xl text-xs leading-relaxed text-muted-foreground">{{ description }}</p>
    </div>
    <div class="min-w-0 md:justify-self-end">
      <slot />
    </div>
  </div>
</template>
