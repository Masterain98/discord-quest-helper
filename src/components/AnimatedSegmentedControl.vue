<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

const props = defineProps<{ value: string }>()
const control = ref<HTMLElement | null>(null)
const indicator = ref<HTMLElement | null>(null)
const surface = ref<HTMLElement | null>(null)
let movement: Animation | undefined
let deformation: Animation | undefined
let resizeObserver: ResizeObserver | undefined
let disabledObserver: MutationObserver | undefined
let motionPreference: MediaQueryList | undefined
let positioned = false
let frame = 0

function positionIndicator(animate = false) {
  const root = control.value
  const slider = indicator.value
  const face = surface.value
  if (!root || !slider || !face) return
  const target = Array.from(root.querySelectorAll<HTMLElement>('[data-segment]'))
    .find(element => element.dataset.segment === props.value)
  if (!target || !target.offsetWidth) return

  const rootBox = root.getBoundingClientRect()
  const targetBox = target.getBoundingClientRect()
  const rootStyle = getComputedStyle(root)
  const x = targetBox.left - rootBox.left - parseFloat(rootStyle.borderLeftWidth)
  const y = targetBox.top - rootBox.top - parseFloat(rootStyle.borderTopWidth)
  const previous = getComputedStyle(slider)
  // Capture the rendered position and squash before cancelling an interrupted move.
  const fromTransform = previous.transform
  const fromWidth = previous.width
  const fromDeformation = getComputedStyle(face).transform
  movement?.cancel()
  deformation?.cancel()

  const transform = `translate3d(${x}px, ${y}px, 0)`
  slider.style.transform = transform
  slider.style.width = `${targetBox.width}px`
  slider.style.height = `${targetBox.height}px`
  slider.style.opacity = target.matches(':disabled') ? '0.5' : '1'
  slider.style.visibility = 'visible'

  if (animate && positioned && !motionPreference?.matches) {
    movement = slider.animate([
      { transform: fromTransform, width: fromWidth },
      { transform, width: `${targetBox.width}px` },
    ], { duration: 420, easing: 'cubic-bezier(0.22, 0.75, 0.25, 1)' })
    deformation = face.animate([
      { transform: fromDeformation, offset: 0 },
      { transform: 'scale(1.08, 0.72)', offset: 0.24 },
      { transform: 'scale(1.04, 0.88)', offset: 0.58 },
      { transform: 'scale(0.99, 1.025)', offset: 0.82 },
      { transform: 'scale(1, 1)', offset: 1 },
    ], { duration: 420, easing: 'ease-in-out' })
  }
  positioned = true
}

function schedulePosition() {
  cancelAnimationFrame(frame)
  frame = requestAnimationFrame(() => positionIndicator())
}

watch(() => props.value, () => positionIndicator(true), { flush: 'post' })

onMounted(async () => {
  await nextTick()
  const root = control.value
  if (!root) return
  motionPreference = window.matchMedia('(prefers-reduced-motion: reduce)')
  motionPreference.addEventListener('change', schedulePosition)
  positionIndicator()
  resizeObserver = new ResizeObserver(schedulePosition)
  resizeObserver.observe(root)
  root.querySelectorAll<HTMLElement>('[data-segment]').forEach(element => resizeObserver?.observe(element))
  disabledObserver = new MutationObserver(schedulePosition)
  disabledObserver.observe(root, { subtree: true, attributes: true, attributeFilter: ['disabled'] })
})

onBeforeUnmount(() => {
  cancelAnimationFrame(frame)
  movement?.cancel()
  deformation?.cancel()
  resizeObserver?.disconnect()
  disabledObserver?.disconnect()
  motionPreference?.removeEventListener('change', schedulePosition)
})
</script>

<template>
  <div ref="control" class="animated-segments" role="group">
    <span ref="indicator" class="segment-indicator" aria-hidden="true">
      <span ref="surface" class="segment-surface" />
    </span>
    <slot />
  </div>
</template>

<style scoped>
.animated-segments {
  position: relative;
  isolation: isolate;
}
.segment-indicator {
  position: absolute;
  z-index: 0;
  top: 0;
  left: 0;
  visibility: hidden;
  pointer-events: none;
}
.segment-surface {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: var(--segment-radius, 0.625rem);
  background: hsl(var(--primary));
  box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.2), 0 3px 8px -3px hsl(var(--primary) / 0.35);
  transform-origin: center;
}
.animated-segments :deep([data-segment]) {
  position: relative;
  z-index: 1;
}
.animated-segments :deep([data-segment][aria-pressed='true']) {
  background: transparent;
  color: hsl(var(--primary-foreground));
}
.animated-segments :deep([data-segment]:active:not(:disabled)) {
  transform: none;
}
</style>
