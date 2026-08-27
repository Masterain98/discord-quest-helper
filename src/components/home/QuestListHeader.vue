<script setup lang="ts">
import { Filter, RotateCw, Search } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import BatchActionsMenu from './BatchActionsMenu.vue'

const { t } = useI18n()

defineProps<{
  query: string
  activeFilterCount: number
  showFilters: boolean
  loading?: boolean
  refreshDisabled?: boolean
  batchDisabled?: boolean
  acceptCount: number
  completeAllCount: number
  videoCount: number
  gameCount: number
}>()

const emit = defineEmits<{
  'update:query': [value: string]
  toggleFilters: []
  refresh: []
  acceptAll: []
  completeAll: []
  completeVideo: []
  completeGame: []
}>()
</script>

<template>
  <div class="rounded-xl border border-border/70 bg-card/72 p-3.5 shadow-[0_10px_28px_-28px_hsl(var(--foreground)/0.45)]">
    <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
      <div class="relative min-w-0 flex-1">
        <Search class="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          :model-value="query"
          type="text"
          :placeholder="t('home.search_placeholder')"
          class="pl-9"
          @update:model-value="emit('update:query', String($event))"
        />
      </div>

        <Button
          variant="outline"
          :class="cn('gap-2', (showFilters || activeFilterCount > 0) && 'border-primary text-primary')"
          @click="emit('toggleFilters')"
        >
          <Filter class="h-4 w-4" />
          {{ t('home.advanced_filters') }}
          <Badge v-if="activeFilterCount > 0" variant="secondary" class="ml-1 h-5 px-1.5">
            {{ activeFilterCount }}
          </Badge>
        </Button>

        <Button
          variant="outline"
          class="gap-2"
          :disabled="refreshDisabled"
          @click="emit('refresh')"
        >
          <RotateCw :class="cn('h-4 w-4', loading && 'animate-spin')" />
          {{ t('general.refresh') }}
        </Button>
    </div>

    <BatchActionsMenu
      :accept-count="acceptCount"
      :complete-all-count="completeAllCount"
      :video-count="videoCount"
      :game-count="gameCount"
      :disabled="batchDisabled"
      @accept-all="emit('acceptAll')"
      @complete-all="emit('completeAll')"
      @complete-video="emit('completeVideo')"
      @complete-game="emit('completeGame')"
    />
  </div>
</template>
