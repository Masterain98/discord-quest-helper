<script setup lang="ts">
import { computed } from 'vue'
import { Check, Gamepad2, ListChecks, MonitorPlay } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'

const { t } = useI18n()

const props = defineProps<{
  acceptCount: number
  completeAllCount: number
  videoCount: number
  gameCount: number
  disabled?: boolean
}>()

const emit = defineEmits<{
  acceptAll: []
  completeAll: []
  completeVideo: []
  completeGame: []
}>()

const hasActions = computed(() =>
  props.acceptCount > 0 ||
  props.completeAllCount > 0 ||
  props.videoCount > 0 ||
  props.gameCount > 0
)
</script>

<template>
  <div
    v-if="hasActions"
    class="mt-3 border-t border-border/60 pt-3"
    :aria-label="t('home.batch_actions')"
  >
    <div class="flex max-w-full flex-wrap items-center gap-2">
      <Button
        v-if="acceptCount > 0"
        size="sm"
        variant="secondary"
        class="shrink-0 gap-2"
        :disabled="disabled"
        @click="emit('acceptAll')"
      >
        <Check class="h-4 w-4" />
        {{ t('home.accept_all') }}
        <span class="tabular-nums opacity-70">{{ acceptCount }}</span>
      </Button>
      <Button
        v-if="completeAllCount > 0"
        size="sm"
        class="shrink-0 gap-2"
        :disabled="disabled"
        @click="emit('completeAll')"
      >
        <ListChecks class="h-4 w-4" />
        {{ t('home.complete_all_tasks') }}
        <span class="tabular-nums opacity-80">{{ completeAllCount }}</span>
      </Button>
      <Button
        v-if="videoCount > 0"
        size="sm"
        variant="outline"
        class="shrink-0 gap-2"
        :disabled="disabled"
        @click="emit('completeVideo')"
      >
        <MonitorPlay class="h-4 w-4" />
        {{ t('home.complete_all_video') }}
        <span class="tabular-nums opacity-70">{{ videoCount }}</span>
      </Button>
      <Button
        v-if="gameCount > 0"
        size="sm"
        variant="outline"
        class="shrink-0 gap-2"
        :disabled="disabled"
        @click="emit('completeGame')"
      >
        <Gamepad2 class="h-4 w-4" />
        {{ t('home.complete_all_game') }}
        <span class="tabular-nums opacity-70">{{ gameCount }}</span>
      </Button>
    </div>
  </div>
</template>
