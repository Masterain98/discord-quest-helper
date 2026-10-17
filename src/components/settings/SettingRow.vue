<script setup lang="ts">
defineProps<{
  label: string
  description?: string
}>()
</script>

<template>
  <div class="settings-row" data-settings-search :data-search-label="label" :data-search-description="description">
    <div class="min-w-0">
      <p class="text-sm font-medium">{{ label }}</p>
      <p v-if="description" class="mt-1 max-w-2xl text-xs leading-relaxed text-muted-foreground">{{ description }}</p>
    </div>
    <div class="settings-row-control">
      <slot />
    </div>
  </div>
</template>
