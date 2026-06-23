<script setup lang="ts">
import { useToastStore } from '@/stores/toast'
import { CheckCircle2, XCircle, AlertTriangle, Info, X } from 'lucide-vue-next'

const toastStore = useToastStore()

const iconMap = {
  success: CheckCircle2,
  error: XCircle,
  warning: AlertTriangle,
  info: Info,
}

const styleMap = {
  success: 'border-green-500/30 bg-green-500/10 text-green-600 dark:text-green-400',
  error: 'border-destructive/30 bg-destructive/10 text-destructive',
  warning: 'border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400',
  info: 'border-blue-500/30 bg-blue-500/10 text-blue-600 dark:text-blue-400',
}
</script>

<template>
  <Teleport to="body">
    <TransitionGroup
      tag="div"
      class="fixed bottom-4 right-4 z-[100] flex flex-col-reverse gap-2 w-[380px] max-w-[calc(100vw-2rem)] pointer-events-none"
      enter-active-class="transition ease-out duration-300"
      enter-from-class="opacity-0 translate-x-8"
      enter-to-class="opacity-100 translate-x-0"
      leave-active-class="transition ease-in duration-200 absolute"
      leave-from-class="opacity-100 translate-x-0"
      leave-to-class="opacity-0 translate-x-8"
      move-class="transition duration-200"
    >
      <div
        v-for="toast in toastStore.toasts"
        :key="toast.id"
        class="pointer-events-auto rounded-lg border bg-popover/95 backdrop-blur-sm shadow-lg p-3 flex gap-2.5 items-start"
        :class="styleMap[toast.type]"
      >
        <component
          :is="iconMap[toast.type]"
          class="w-5 h-5 shrink-0 mt-0.5"
        />

        <div class="flex-1 min-w-0">
          <p class="text-sm font-medium text-foreground leading-snug">{{ toast.title }}</p>
          <p v-if="toast.description" class="text-xs text-muted-foreground mt-0.5 leading-relaxed break-words">{{ toast.description }}</p>

          <div v-if="toast.actions && toast.actions.length > 0" class="flex gap-2 mt-2">
            <button
              v-for="(action, idx) in toast.actions"
              :key="idx"
              class="text-xs font-medium px-2 py-1 rounded border border-current/20 hover:bg-current/10 transition-colors"
              @click="action.onClick(); toastStore.dismiss(toast.id)"
            >
              {{ action.label }}
            </button>
          </div>
        </div>

        <button
          type="button"
          aria-label="Dismiss notification"
          class="shrink-0 text-muted-foreground/60 hover:text-foreground transition-colors"
          @click="toastStore.dismiss(toast.id)"
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    </TransitionGroup>
  </Teleport>
</template>
