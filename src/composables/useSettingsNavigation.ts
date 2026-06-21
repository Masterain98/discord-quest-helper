import { onMounted, onUnmounted, ref, watch } from 'vue'

export type SettingsSection =
  | 'account'
  | 'quest_behavior'
  | 'discord_integration'
  | 'appearance'
  | 'diagnostics'
  | 'advanced'
  | 'about'

const SETTINGS_SECTION_STORAGE_KEY = 'questHelper_lastSettingsSection'
const settingsSections: SettingsSection[] = [
  'account',
  'quest_behavior',
  'discord_integration',
  'appearance',
  'diagnostics',
  'advanced',
  'about',
]

export function isSettingsSection(value: unknown): value is SettingsSection {
  return typeof value === 'string' && settingsSections.includes(value as SettingsSection)
}

function readInitialSection(): SettingsSection {
  const saved = localStorage.getItem(SETTINGS_SECTION_STORAGE_KEY)
  return isSettingsSection(saved) ? saved : 'account'
}

export function persistSettingsSection(section: SettingsSection) {
  localStorage.setItem(SETTINGS_SECTION_STORAGE_KEY, section)
  window.dispatchEvent(new CustomEvent('app:open-settings-section', { detail: section }))
}

export function useSettingsNavigation() {
  const selectedSection = ref<SettingsSection>(readInitialSection())

  function setSection(section: SettingsSection) {
    selectedSection.value = section
  }

  function handleDeepLink(event: Event) {
    const section = (event as CustomEvent<unknown>).detail
    if (isSettingsSection(section)) {
      selectedSection.value = section
    }
  }

  watch(selectedSection, (section) => {
    localStorage.setItem(SETTINGS_SECTION_STORAGE_KEY, section)
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
  }
}
