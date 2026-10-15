<script setup lang="ts">
import type { Component, HTMLAttributes } from 'vue'
import { cn } from '@/lib/utils'
import type { SettingsTone } from './settingTones'

const props = defineProps<{
  title: string
  description?: string
  icon?: Component
  tone?: SettingsTone
  contentClass?: HTMLAttributes['class']
}>()
</script>

<template>
  <div class="settings-section-block">
    <header class="settings-section-heading">
      <span v-if="icon" class="settings-section-glyph ui-icon-tile">
        <component :is="icon" class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" />
      </span>
      <div class="min-w-0">
        <h2 class="settings-section-title">{{ title }}</h2>
        <p v-if="description" class="settings-section-description">{{ description }}</p>
      </div>
    </header>
    <div :class="cn('settings-section-surface ui-panel space-y-5', props.contentClass)">
      <slot />
    </div>
  </div>
</template>
