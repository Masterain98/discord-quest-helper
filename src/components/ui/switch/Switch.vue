<script setup lang="ts">
import { useId } from 'vue'

const props = defineProps<{
  modelValue: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

const gripId = `switch-grip-${useId()}`

function activate() {
  if (!props.disabled) emit('update:modelValue', !props.modelValue)
}
</script>

<template>
  <button
    type="button"
    role="switch"
    class="tactile-switch"
    :data-state="modelValue ? 'on' : 'off'"
    :aria-checked="modelValue"
    :disabled="disabled"
    @click="activate"
  >
    <svg class="tactile-switch__symbols" viewBox="0 0 48 24" aria-hidden="true">
      <path class="tactile-switch__enabled" d="M10 8v8" />
      <circle class="tactile-switch__idle" cx="36" cy="12" r="3.2" />
    </svg>
    <span class="tactile-switch__knob" aria-hidden="true">
      <svg class="tactile-switch__grip" viewBox="0 0 24 24">
        <defs>
          <pattern :id="gripId" width="4" height="4" patternUnits="userSpaceOnUse">
            <path class="grip-top" d="M0 0h4L2 2Z" />
            <path class="grip-right" d="M4 0v4L2 2Z" />
            <path class="grip-bottom" d="M4 4H0l2-2Z" />
            <path class="grip-left" d="M0 4V0l2 2Z" />
          </pattern>
        </defs>
        <circle cx="12" cy="12" r="12" :fill="`url(#${gripId})`" />
      </svg>
    </span>
  </button>
</template>

<style scoped>
.tactile-switch {
  --knob: 1.375rem;
  --edge: 0.1875rem;
  --travel: 1.5rem;
  --shade: rgb(15 20 42 / 0.26);
  --rim-start: color-mix(in srgb, hsl(var(--muted-foreground)) 55%, hsl(var(--input)));
  --rim-end: hsl(var(--background));
  --well: color-mix(in srgb, hsl(var(--input)) 36%, hsl(var(--muted-foreground)));
  --ceramic: hsl(var(--control-cap));
  position: relative;
  isolation: isolate;
  display: inline-block;
  flex-shrink: 0;
  width: calc(var(--knob) + var(--travel) + var(--edge) * 2);
  height: calc(var(--knob) + var(--edge) * 2);
  padding: 0;
  border: none;
  border-radius: 100vmax;
  background: linear-gradient(to bottom, var(--rim-start), var(--rim-end));
  box-shadow: 0 1px 3px rgb(15 20 42 / 0.16);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.dark .tactile-switch {
  --shade: rgb(0 0 0 / 0.45);
  --rim-start: color-mix(in srgb, hsl(var(--input)) 60%, hsl(var(--muted-foreground)));
  --rim-end: color-mix(in srgb, hsl(var(--input)) 28%, hsl(var(--muted-foreground)));
  --well: hsl(var(--input));
}

.tactile-switch::before {
  content: '';
  position: absolute;
  inset: 0.125rem;
  border-radius: inherit;
  background-color: var(--well);
  background-image: radial-gradient(ellipse at 50% 100%, rgb(255 255 255 / 0.13), transparent 75%);
  box-shadow: inset 0 3px 6px var(--shade), inset 0 -1px 1px rgb(255 255 255 / 0.15);
  transition: background-color 220ms ease-out;
}

.tactile-switch[data-state='on']::before {
  background-color: hsl(var(--primary));
}

.tactile-switch__symbols {
  position: absolute;
  inset: 0.125rem;
  width: calc(100% - 0.25rem);
  height: calc(100% - 0.25rem);
  fill: none;
  stroke: var(--ceramic);
  stroke-width: 1.5;
  stroke-linecap: round;
  filter: drop-shadow(0 1px 0 var(--shade));
  pointer-events: none;
}

.tactile-switch__enabled,
.tactile-switch__idle {
  transition: opacity 160ms ease-out;
}

.tactile-switch__enabled { opacity: 0; }
.tactile-switch__idle { opacity: 0.35; }
.tactile-switch[data-state='on'] .tactile-switch__enabled { opacity: 0.55; }
.tactile-switch[data-state='on'] .tactile-switch__idle { opacity: 0; }

.tactile-switch__knob {
  position: absolute;
  top: var(--edge);
  left: var(--edge);
  width: var(--knob);
  height: var(--knob);
  border-radius: 50%;
  background: linear-gradient(to bottom, var(--ceramic), color-mix(in srgb, var(--ceramic) 77%, hsl(var(--muted-foreground))));
  box-shadow: 0 1px 3px var(--shade), inset 0 1px 0 var(--ceramic);
  pointer-events: none;
  transition: transform 260ms cubic-bezier(0.2, 0.8, 0.25, 1);
}

.tactile-switch__grip {
  position: absolute;
  inset: 14%;
  width: 72%;
  height: 72%;
  border-radius: inherit;
  box-shadow: inset 0 1px 2px rgb(15 20 42 / 0.18);
}

.grip-top { fill: var(--ceramic); }
.grip-right { fill: color-mix(in srgb, var(--ceramic) 87%, hsl(var(--muted-foreground))); }
.grip-bottom { fill: color-mix(in srgb, var(--ceramic) 75%, hsl(var(--muted-foreground))); }
.grip-left { fill: color-mix(in srgb, var(--ceramic) 94%, hsl(var(--muted-foreground))); }

.tactile-switch[data-state='on'] .tactile-switch__knob {
  transform: translateX(var(--travel));
}

.tactile-switch:active:not(:disabled) .tactile-switch__knob {
  transform: scale(0.95);
}

.tactile-switch[data-state='on']:active:not(:disabled) .tactile-switch__knob {
  transform: translateX(var(--travel)) scale(0.95);
}

.tactile-switch:focus-visible {
  outline: 2px solid hsl(var(--ring));
  outline-offset: 3px;
}

.tactile-switch:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

@media (prefers-reduced-motion: reduce) {
  .tactile-switch::before,
  .tactile-switch__knob,
  .tactile-switch__enabled,
  .tactile-switch__idle {
    transition: none;
  }
}
</style>
