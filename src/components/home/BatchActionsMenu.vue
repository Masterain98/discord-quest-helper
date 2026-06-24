<script setup lang="ts">
import { computed } from 'vue'
import { Check, ChevronDown, Gamepad2, ListChecks, MonitorPlay } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

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
  <DropdownMenu v-if="hasActions">
    <DropdownMenuTrigger as-child>
      <Button variant="outline" class="gap-2" :disabled="disabled">
        <ListChecks class="h-4 w-4" />
        {{ t('home.batch_actions') }}
        <ChevronDown class="h-3.5 w-3.5 opacity-70" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" class="w-64">
      <DropdownMenuItem v-if="acceptCount > 0" class="gap-2" @click="emit('acceptAll')">
        <Check class="h-4 w-4" />
        {{ t('home.accept_all') }} ({{ acceptCount }})
      </DropdownMenuItem>
      <DropdownMenuItem v-if="completeAllCount > 0" class="gap-2" @click="emit('completeAll')">
        <ListChecks class="h-4 w-4" />
        {{ t('home.complete_all_tasks') }} ({{ completeAllCount }})
      </DropdownMenuItem>
      <DropdownMenuItem v-if="videoCount > 0" class="gap-2" @click="emit('completeVideo')">
        <MonitorPlay class="h-4 w-4" />
        {{ t('home.complete_all_video') }} ({{ videoCount }})
      </DropdownMenuItem>
      <DropdownMenuItem v-if="gameCount > 0" class="gap-2" @click="emit('completeGame')">
        <Gamepad2 class="h-4 w-4" />
        {{ t('home.complete_all_game') }} ({{ gameCount }})
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
