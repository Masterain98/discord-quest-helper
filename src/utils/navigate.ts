import { persistSettingsSection, type SettingsSection } from '@/composables/useSettingsNavigation'

/** Navigate to a tab in the app (dispatches a CustomEvent that App.vue listens for) */
export function navigateToTab(tab: 'home' | 'game' | 'settings' | 'debug', settingsSection?: SettingsSection) {
  if (tab === 'settings' && settingsSection) {
    persistSettingsSection(settingsSection)
  }
  window.dispatchEvent(new CustomEvent('app:navigate', { detail: tab }))
}
