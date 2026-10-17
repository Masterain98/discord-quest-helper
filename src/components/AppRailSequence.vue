<script setup lang="ts">
import { onUnmounted, ref } from 'vue'

withDefaults(defineProps<{ tag?: string }>(), { tag: 'div' })

type InlinePosition = {
  position: string
  left: string
  top: string
  width: string
  height: string
}
type HeldSiblingMove = {
  element: HTMLElement
  x: number
  y: number
  transform: string
  transition: string
}

const sequence = ref<{ $el: HTMLElement } | null>(null)
const leaveAnimations = new Map<HTMLElement, Animation>()
const leavePositionStyles = new Map<HTMLElement, InlinePosition>()
const leaveSiblingMoves = new Map<HTMLElement, HeldSiblingMove[]>()
const siblingAnimations = new Map<HTMLElement, Animation>()
let leaving = 0

function syncPhase() {
  const element = sequence.value?.$el
  if (!element) return
  element.classList.toggle('app-rail-sequence--leaving', leaving > 0)
}

function withOffset(transform: string, x: number, y: number) {
  const offset = `translate(${x}px, ${y}px)`
  return transform ? `${transform} ${offset}` : offset
}

function beforeLeave(element: Element) {
  leaving += 1
  syncPhase()

  if (!(element instanceof HTMLElement)) return
  const root = sequence.value?.$el
  if (!root) return

  const rect = element.getBoundingClientRect()
  const rootRect = root.getBoundingClientRect()
  const siblingPositions = Array.from(root.children)
    .filter((child): child is HTMLElement => child instanceof HTMLElement
      && child !== element
      && !child.classList.contains('app-rail-sequence-leave-active'))
    .map((sibling) => ({ element: sibling, rect: sibling.getBoundingClientRect() }))

  leavePositionStyles.set(element, {
    position: element.style.position,
    left: element.style.left,
    top: element.style.top,
    width: element.style.width,
    height: element.style.height,
  })
  Object.assign(element.style, {
    position: 'absolute',
    left: `${rect.left - rootRect.left - root.clientLeft + root.scrollLeft}px`,
    top: `${rect.top - rootRect.top - root.clientTop + root.scrollTop}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
  })

  const heldMoves = siblingPositions.flatMap(({ element: sibling, rect: previous }) => {
    const next = sibling.getBoundingClientRect()
    const x = previous.left - next.left
    const y = previous.top - next.top
    if (Math.abs(x) < 0.5 && Math.abs(y) < 0.5) return []

    const move = {
      element: sibling,
      x,
      y,
      transform: sibling.style.transform,
      transition: sibling.style.transition,
    }
    sibling.style.transition = 'none'
    sibling.style.transform = withOffset(move.transform, x, y)
    return [move]
  })
  leaveSiblingMoves.set(element, heldMoves)
  void root.offsetHeight
}

function leave(element: Element, done: () => void) {
  if (!(element instanceof HTMLElement) || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    done()
    return
  }

  const animation = element.animate(
    [
      { opacity: 1, transform: 'scale(1)' },
      { opacity: 0, transform: 'scale(.88)' },
    ],
    { duration: 220, easing: 'ease-out', fill: 'forwards' },
  )
  leaveAnimations.set(element, animation)
  animation.onfinish = () => {
    if (leaveAnimations.get(element) === animation) leaveAnimations.delete(element)
    done()
  }
}

function cancelLeave(element: Element) {
  if (element instanceof HTMLElement) {
    leaveAnimations.get(element)?.cancel()
    leaveAnimations.delete(element)
  }
  finishLeave(element, false)
}

function finishLeave(element?: Element, animateSiblings = true) {
  if (element instanceof HTMLElement) {
    const styles = leavePositionStyles.get(element)
    if (styles) {
      Object.assign(element.style, styles)
      leavePositionStyles.delete(element)
    }
    releaseSiblingMoves(element, animateSiblings)
  }
  leaving = Math.max(0, leaving - 1)
  syncPhase()
}

function releaseSiblingMoves(leavingElement: HTMLElement, animate: boolean) {
  const moves = leaveSiblingMoves.get(leavingElement)
  if (!moves) return
  leaveSiblingMoves.delete(leavingElement)

  for (const move of moves) {
    const { element, x, y } = move
    siblingAnimations.get(element)?.cancel()
    siblingAnimations.delete(element)

    if (!animate || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      element.style.transform = move.transform
      element.style.transition = move.transition
      continue
    }

    const startTransform = withOffset(move.transform, x, y)
    const endTransform = move.transform || 'translate(0, 0)'
    element.style.transform = move.transform
    element.style.transition = 'none'
    const animation = element.animate(
      [{ transform: startTransform }, { transform: endTransform }],
      { duration: 360, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' },
    )
    siblingAnimations.set(element, animation)
    animation.onfinish = () => {
      if (siblingAnimations.get(element) !== animation) return
      siblingAnimations.delete(element)
      element.style.transform = move.transform
      element.style.transition = move.transition
    }
  }
}

onUnmounted(() => {
  for (const animation of leaveAnimations.values()) animation.cancel()
  leaveAnimations.clear()
  for (const animation of siblingAnimations.values()) animation.cancel()
  siblingAnimations.clear()
  leavePositionStyles.clear()
  leaveSiblingMoves.clear()
})
</script>

<template>
  <TransitionGroup
    ref="sequence"
    :tag="tag"
    name="app-rail-sequence"
    move-class="app-rail-sequence-move"
    class="app-rail-sequence"
    @before-leave="beforeLeave"
    @leave="leave"
    @after-leave="finishLeave"
    @leave-cancelled="cancelLeave"
  >
    <slot />
  </TransitionGroup>
</template>
