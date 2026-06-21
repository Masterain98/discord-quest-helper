/** Navigate to a tab in the app (dispatches a CustomEvent that App.vue listens for) */
export function navigateToTab(tab: 'home' | 'game' | 'settings' | 'debug') {
  window.dispatchEvent(new CustomEvent('app:navigate', { detail: tab }))
}
