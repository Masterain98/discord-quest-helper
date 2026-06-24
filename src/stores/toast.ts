import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface ToastAction {
  label: string
  onClick: () => void
}

export type ToastType = 'success' | 'error' | 'warning' | 'info'

export interface ToastOptions {
  title: string
  description?: string
  type?: ToastType
  actions?: ToastAction[]
  duration?: number
}

interface ToastItem extends Required<Omit<ToastOptions, 'description' | 'actions'>> {
  id: number
  description?: string
  actions?: ToastAction[]
}

export const useToastStore = defineStore('toast', () => {
  const toasts = ref<ToastItem[]>([])
  let nextId = 0
  const timers = new Map<number, ReturnType<typeof setTimeout>>()

  function dismiss(id: number) {
    const timer = timers.get(id)
    if (timer) {
      clearTimeout(timer)
      timers.delete(id)
    }
    const idx = toasts.value.findIndex(t => t.id === id)
    if (idx !== -1) toasts.value.splice(idx, 1)
  }

  function show(options: ToastOptions): number {
    const id = ++nextId
    const duration = options.duration ?? 5000
    const item: ToastItem = {
      id,
      title: options.title,
      type: options.type ?? 'info',
      duration,
      description: options.description,
      actions: options.actions,
    }
    toasts.value.push(item)

    if (duration > 0) {
      timers.set(id, setTimeout(() => dismiss(id), duration))
    }
    return id
  }

  function success(options: ToastOptions): number {
    return show({ ...options, type: 'success' })
  }

  function error(options: ToastOptions): number {
    return show({ ...options, type: 'error', duration: options.duration ?? 7000 })
  }

  function warning(options: ToastOptions): number {
    return show({ ...options, type: 'warning', duration: options.duration ?? 6000 })
  }

  function info(options: ToastOptions): number {
    return show({ ...options, type: 'info' })
  }

  function clear() {
    timers.forEach(t => clearTimeout(t))
    timers.clear()
    toasts.value = []
  }

  return { toasts, show, success, error, warning, info, dismiss, clear }
})
