import { onMounted, onUnmounted, ref, watch } from 'vue'

const SETTINGS_SECTION_STORAGE_KEY = 'questHelper_lastSettingsSection'
export const settingsSections = [
  'account',
  'quest_behavior',
  'discord_integration',
  'translation',
  'live_tunnel',
  'appearance',
  'diagnostics',
  'advanced',
  'about',
] as const

export type SettingsSection = (typeof settingsSections)[number]

export function isSettingsSection(value: unknown): value is SettingsSection {
  return typeof value === 'string' && (settingsSections as readonly string[]).includes(value)
}

function getStorage(): Storage | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage
  } catch {
    return null
  }
}

function readInitialSection(): SettingsSection {
  const saved = getStorage()?.getItem(SETTINGS_SECTION_STORAGE_KEY)
  return isSettingsSection(saved) ? saved : 'account'
}

export function persistSettingsSection(section: SettingsSection) {
  getStorage()?.setItem(SETTINGS_SECTION_STORAGE_KEY, section)
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('app:open-settings-section', { detail: section }))
  }
}

export function useSettingsNavigation() {
  const selectedSection = ref<SettingsSection>(readInitialSection())
  // A request is distinct from scroll-driven selection, including reselecting
  // the current category while its heading has scrolled out of view.
  const navigationRequest = ref(0)
  const navigationTarget = ref<string | null>(null)

  function setSection(section: SettingsSection, target: string | null = null) {
    selectedSection.value = section
    navigationTarget.value = target
    navigationRequest.value++
  }

  function handleDeepLink(event: Event) {
    const section = (event as CustomEvent<unknown>).detail
    if (isSettingsSection(section)) {
      setSection(section)
    }
  }

  watch(selectedSection, (section) => {
    getStorage()?.setItem(SETTINGS_SECTION_STORAGE_KEY, section)
  })

  onMounted(() => {
    window.addEventListener('app:open-settings-section', handleDeepLink)
  })

  onUnmounted(() => {
    window.removeEventListener('app:open-settings-section', handleDeepLink)
  })

  return {
    selectedSection,
    setSection,
    navigationRequest,
    navigationTarget,
  }
}
