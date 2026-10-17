import { nextTick, onMounted, onUnmounted, ref, watch, type Ref } from 'vue'
import { isSettingsSection, type SettingsSection } from './useSettingsNavigation'

export interface SettingsSearchResult {
  id: string
  section: SettingsSection
  sectionLabel: string
  label: string
  description: string
}

function normalize(text: string) {
  return text.normalize('NFKD').replace(/\p{M}/gu, '').toLocaleLowerCase().trim()
}

/** Index translated labels only; never index account data or input values. */
export function useSettingsSearch(page: Ref<HTMLElement | null>, query: () => string, onResults: (results: SettingsSearchResult[]) => void) {
  const entries = ref<SettingsSearchResult[]>([])
  let observer: MutationObserver | undefined
  let nextId = 0
  let disposed = false

  function index() {
    const results: SettingsSearchResult[] = []
    for (const section of page.value?.querySelectorAll<HTMLElement>('[data-settings-section]') ?? []) {
      const key = section.dataset.settingsSection
      if (!isSettingsSection(key)) continue
      const sectionLabel = section.getAttribute('aria-label') ?? ''
      results.push({ id: section.id, section: key, sectionLabel, label: sectionLabel, description: '' })
      for (const element of section.querySelectorAll<HTMLElement>('[data-settings-search]')) {
        const label = element.dataset.searchLabel
        if (!label) continue
        if (!element.id) element.id = `settings-item-${++nextId}`
        results.push({ id: element.id, section: key, sectionLabel, label, description: element.dataset.searchDescription ?? '' })
      }
    }
    entries.value = results
  }

  watch([entries, query], ([items, text]) => {
    const tokens = normalize(text).split(/\s+/).filter(Boolean)
    onResults(tokens.length ? items.filter(item => {
      const haystack = normalize(`${item.label} ${item.description} ${item.sectionLabel}`)
      return tokens.every(token => haystack.includes(token))
    }) : [])
  }, { immediate: true })

  onMounted(async () => {
    await nextTick()
    if (disposed || !page.value) return
    index()
    observer = new MutationObserver(index)
    observer.observe(page.value, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['data-search-label', 'data-search-description', 'aria-label'],
    })
  })

  onUnmounted(() => {
    disposed = true
    observer?.disconnect()
    onResults([])
  })
}
