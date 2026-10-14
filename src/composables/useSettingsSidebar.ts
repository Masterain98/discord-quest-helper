import { ref } from 'vue'
import { useMediaQuery } from '@vueuse/core'

const STORAGE_KEY = 'questHelper_settingsSidebarExpanded'

export function useSettingsSidebar() {
  const wide = useMediaQuery('(min-width: 1024px)')
  // Only explicit desktop toggles persist. Resizing and closing a mobile
  // drawer must never change the user's desktop preference.
  let saved: string | null = null
  try { saved = localStorage.getItem(STORAGE_KEY) } catch { /* Storage may be unavailable. */ }
  const expanded = ref(saved !== 'false')
  const drawerOpen = ref(false)

  function toggle() {
    if (!wide.value) {
      drawerOpen.value = !drawerOpen.value
      return
    }
    expanded.value = !expanded.value
    try { localStorage.setItem(STORAGE_KEY, String(expanded.value)) } catch { /* Keep the in-memory preference. */ }
  }

  return { wide, expanded, drawerOpen, toggle }
}
