<script setup lang="ts">
import type { Component } from 'vue'
import { cn } from '@/lib/utils'
import { settingToneClass, type SettingsTone } from './settingTones'

withDefaults(defineProps<{
  tone?: SettingsTone
  icon?: Component
}>(), {
  tone: 'neutral',
})
</script>

<template>
  <div :class="cn('flex items-start gap-2.5 rounded-lg border px-4 py-3 text-sm', settingToneClass[tone].panel)">
    <component v-if="icon" :is="icon" class="mt-0.5 h-4 w-4 shrink-0" />
    <div class="min-w-0 flex-1">
      <slot />
    </div>
  </div>
</template>
