<script setup lang="ts">
import { computed } from 'vue'
import { Activity, AlertCircle, CheckCircle2, Gift, ListChecks, Loader2, PlayCircle, RefreshCw, Timer } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'

export type NextBestActionState =
  | 'blocked'
  | 'active'
  | 'queue'
  | 'claim'
  | 'run'
  | 'accept'
  | 'empty'
  | 'done'
  | 'error'

const props = defineProps<{
  state: NextBestActionState
  title: string
  description: string
  primaryLabel: string
  secondaryLabel?: string
  primaryDisabled?: boolean
  secondaryDisabled?: boolean
  busy?: boolean
}>()

const emit = defineEmits<{
  primary: []
  secondary: []
}>()

const icon = computed(() => {
  switch (props.state) {
    case 'blocked': return Timer
    case 'active': return Activity
    case 'queue': return ListChecks
    case 'claim': return Gift
    case 'run': return PlayCircle
    case 'accept': return CheckCircle2
    case 'done': return CheckCircle2
    case 'error': return AlertCircle
    default: return RefreshCw
  }
})

const toneClass = computed(() => {
  switch (props.state) {
    case 'blocked': return 'border-amber-500/30 bg-amber-500/10'
    case 'active':
    case 'queue': return 'border-violet-500/30 bg-violet-500/10'
    case 'claim': return 'border-emerald-500/30 bg-emerald-500/10'
    case 'done': return 'border-emerald-500/30 bg-emerald-500/10'
    case 'error': return 'border-destructive/30 bg-destructive/10'
    default: return 'border-primary/30 bg-primary/10'
  }
})

const primaryVariant = computed(() => {
  if (props.state === 'active' || props.state === 'queue') return 'destructive'
  if (props.state === 'blocked' || props.state === 'empty' || props.state === 'done') return 'secondary'
  return 'default'
})
</script>

<template>
  <Card :class="['border shadow-sm', toneClass]">
    <CardContent class="p-5">
      <div class="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div class="flex min-w-0 gap-4">
          <div class="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-background/80">
            <component :is="icon" class="h-5 w-5" />
          </div>
          <div class="min-w-0">
            <p class="text-lg font-semibold leading-tight">{{ title }}</p>
            <p class="mt-1 max-w-2xl text-sm text-muted-foreground">{{ description }}</p>
          </div>
        </div>

        <div class="flex shrink-0 flex-col gap-2 sm:flex-row">
          <Button
            v-if="secondaryLabel"
            variant="outline"
            :disabled="secondaryDisabled || busy"
            @click="emit('secondary')"
          >
            {{ secondaryLabel }}
          </Button>
          <Button
            :variant="primaryVariant"
            :disabled="primaryDisabled || busy"
            class="gap-2"
            @click="emit('primary')"
          >
            <Loader2 v-if="busy" class="h-4 w-4 animate-spin" />
            {{ primaryLabel }}
          </Button>
        </div>
      </div>
    </CardContent>
  </Card>
</template>
