<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import AnimatedSegmentedControl from '@/components/AnimatedSegmentedControl.vue'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { GameIdleDuration, GameIdleDurationMode } from '@/api/tauri'

const props = defineProps<{
  label: string
  unit: 'minutes' | 'seconds'
  minAllowed: number
  modelValue: GameIdleDuration
  disabled?: boolean
  invalid?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: GameIdleDuration]
}>()

const { t } = useI18n()
const unitLabel = computed(() => t(props.unit === 'minutes' ? 'game_idle.unit_minutes' : 'game_idle.unit_seconds'))

// The Input commits on blur only (passive useVModel emits nothing while
// typing); the owning store normalizes the emitted object into safe bounds.
function commit(field: 'fixed' | 'min' | 'max', event: FocusEvent) {
  const raw = (event.target as HTMLInputElement).value
  emit('update:modelValue', { ...props.modelValue, [field]: raw === '' ? Number.NaN : Number(raw) })
}

function setMode(mode: GameIdleDurationMode) {
  if (!props.disabled && props.modelValue.mode !== mode) {
    emit('update:modelValue', { ...props.modelValue, mode })
  }
}
</script>

<template>
  <div class="space-y-1.5">
    <Label class="text-xs">{{ label }}</Label>
    <div class="flex flex-wrap items-center gap-2">
      <AnimatedSegmentedControl
        :value="modelValue.mode"
        class="flex h-10 rounded-md border bg-background p-1"
        style="--segment-radius: 0.28rem"
        :aria-label="label"
      >
        <button
          type="button"
          class="duration-mode-button"
          data-segment="fixed"
          :aria-pressed="modelValue.mode === 'fixed'"
          :disabled="disabled"
          @click="setMode('fixed')"
        >
          {{ t('game_idle.duration_fixed') }}
        </button>
        <button
          type="button"
          class="duration-mode-button"
          data-segment="random"
          :aria-pressed="modelValue.mode === 'random'"
          :disabled="disabled"
          @click="setMode('random')"
        >
          {{ t('game_idle.duration_random') }}
        </button>
      </AnimatedSegmentedControl>
      <Input
        v-if="modelValue.mode === 'fixed'"
        type="number"
        inputmode="numeric"
        :min="minAllowed"
        step="1"
        :model-value="modelValue.fixed"
        class="w-20 tabular-nums"
        :disabled="disabled"
        :aria-label="label"
        :aria-invalid="invalid || undefined"
        @blur="commit('fixed', $event)"
      />
      <template v-else>
        <Input
          type="number"
          inputmode="numeric"
          :min="minAllowed"
          step="1"
          :model-value="modelValue.min"
          class="w-20 tabular-nums"
          :disabled="disabled"
          :aria-label="t('game_idle.duration_min')"
          :aria-invalid="invalid || undefined"
          @blur="commit('min', $event)"
        />
        <Input
          type="number"
          inputmode="numeric"
          :min="minAllowed"
          step="1"
          :model-value="modelValue.max"
          class="w-20 tabular-nums"
          :disabled="disabled"
          :aria-label="t('game_idle.duration_max')"
          :aria-invalid="invalid || undefined"
          @blur="commit('max', $event)"
        />
      </template>
      <span class="text-xs text-muted-foreground">{{ unitLabel }}</span>
    </div>
  </div>
</template>

<style scoped>
.duration-mode-button { display: inline-flex; align-items: center; gap: .35rem; border-radius: .28rem; padding: 0 .7rem; font-size: .75rem; color: hsl(var(--muted-foreground)); transition: color 180ms ease, background 180ms ease, transform 120ms ease; }
.duration-mode-button:hover:not(:disabled) { color: hsl(var(--foreground)); }
.duration-mode-button:active:not(:disabled) { transform: translateY(1px); }
.duration-mode-button:focus-visible { outline: 2px solid hsl(var(--ring)); outline-offset: 2px; }
.duration-mode-button:disabled { cursor: not-allowed; opacity: .5; }
</style>
