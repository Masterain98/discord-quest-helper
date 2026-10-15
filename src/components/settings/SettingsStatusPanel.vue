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
  <div :class="cn('settings-status-panel grid gap-x-2.5 gap-y-3 rounded-xl border px-4 py-3 text-sm leading-relaxed', $slots.actions && 'sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center', settingToneClass[tone].panel)">
    <div class="flex min-w-0 items-start gap-2.5">
      <span v-if="icon" class="settings-status-icon flex h-[1lh] shrink-0 items-center" aria-hidden="true">
        <component :is="icon" class="h-4 w-4" />
      </span>
      <div class="settings-status-content min-w-0 flex-1">
        <slot />
      </div>
    </div>
    <div v-if="$slots.actions" :class="cn('justify-self-start sm:justify-self-end', icon && 'pl-[1.625rem] sm:pl-0')">
      <slot name="actions" />
    </div>
  </div>
</template>
