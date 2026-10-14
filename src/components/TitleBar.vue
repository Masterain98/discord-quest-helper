<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Minus, Square, X, Copy } from 'lucide-vue-next'
import { useVersionStore } from '@/stores/version'

defineProps<{ brandVisible?: boolean }>()
const appWindow = getCurrentWindow()
const versionStore = useVersionStore()
const isMaximized = ref(false)
let unlisten: (() => void) | null = null

onMounted(async () => {
  isMaximized.value = await appWindow.isMaximized()
  unlisten = await appWindow.listen('tauri://resize', async () => {
    isMaximized.value = await appWindow.isMaximized()
  })
})

onUnmounted(() => {
  if (unlisten) unlisten()
})

async function handleMinimize() {
  await appWindow.minimize()
}

async function handleToggleMaximize() {
  await appWindow.toggleMaximize()
}

async function handleClose() {
  await appWindow.close()
}

// Manual drag implementation per Tauri v2 docs
async function handleDragStart(e: MouseEvent) {
  // Only start dragging on left mouse button (primary)
  if (e.buttons === 1) {
    if (e.detail === 2) {
      // Double click to toggle maximize
      await appWindow.toggleMaximize()
    } else {
      // Start dragging
      await appWindow.startDragging()
    }
  }
}
</script>

<template>
  <div class="app-titlebar fixed left-0 top-0 z-50 flex h-[48px] w-full select-none items-center">
    <div
      class="app-titlebar-brand app-brand-lockup"
      :class="{ 'invisible': brandVisible === false }"
      @mousedown="handleDragStart"
    >
      <span class="app-titlebar-logo-zone" aria-hidden="true">
        <img src="/icons/logo.png" alt="" class="h-9 w-9 rounded-[11px] pointer-events-none" />
      </span>
      <span class="app-titlebar-app-name">
        <span class="font-medium">Discord Quest Helper</span>
        <span class="opacity-70">v{{ versionStore.currentVersion }}</span>
      </span>
    </div>

    <div class="app-titlebar-drag-region" @mousedown="handleDragStart" />

    <div class="app-titlebar-controls flex h-full">
      <button
        type="button"
        @click="handleMinimize"
        class="titlebar-button"
        tabindex="-1"
      >
        <Minus class="w-4 h-4" />
      </button>

      <button
        type="button"
        @click="handleToggleMaximize"
        class="titlebar-button"
        tabindex="-1"
      >
        <Copy v-if="isMaximized" class="w-4 h-4 rotate-180" />
        <Square v-else class="w-3.5 h-3.5" />
      </button>

      <button
        type="button"
        @click="handleClose"
        class="titlebar-button titlebar-button-close"
        tabindex="-1"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
  </div>
  <!-- Spacer to prevent content from going under titlebar -->
  <div class="h-[48px] w-full shrink-0"></div>
</template>

<style scoped>
/* No specific styles needed thanks to tailwind */
</style>

