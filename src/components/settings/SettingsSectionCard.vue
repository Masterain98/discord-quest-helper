<script setup lang="ts">
import type { Component, HTMLAttributes } from 'vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import { settingToneClass, type SettingsTone } from './settingTones'

const props = withDefaults(defineProps<{
  title: string
  description?: string
  icon?: Component
  tone?: SettingsTone
  contentClass?: HTMLAttributes['class']
}>(), {
  tone: 'neutral',
})
</script>

<template>
  <Card :class="cn('overflow-hidden transition-shadow hover:shadow-sm', settingToneClass[tone].card)">
    <CardHeader class="pb-4">
      <div class="flex items-start gap-3">
        <div
          v-if="icon"
          :class="cn('mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-md', settingToneClass[tone].icon)"
        >
          <component :is="icon" class="h-5 w-5" />
        </div>
        <div class="min-w-0">
          <CardTitle>{{ title }}</CardTitle>
          <CardDescription v-if="description" class="mt-1">
            {{ description }}
          </CardDescription>
        </div>
      </div>
    </CardHeader>
    <CardContent :class="cn('space-y-4', props.contentClass)">
      <slot />
    </CardContent>
  </Card>
</template>
