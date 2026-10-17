import { nextTick, onMounted, onUnmounted, type Ref } from 'vue'
import { isSettingsSection, type SettingsSection } from './useSettingsNavigation'

/** Keep category navigation and the visible section in sync in the workspace scroller. */
export function useSettingsScroll(page: Ref<HTMLElement | null>, options: {
  initialSection: () => SettingsSection
  hasInitialRequest: () => boolean
  onVisible: (section: SettingsSection) => void
}) {
  let container: HTMLElement | null = null
  let observer: ResizeObserver | undefined
  let frame = 0
  let settling: ReturnType<typeof setTimeout> | undefined
  let requested: SettingsSection | null = null
  let disposed = false
  let visible: SettingsSection | null = null
  let highlighted: HTMLElement | null = null
  let highlightTimer: ReturnType<typeof setTimeout> | undefined

  function sections() {
    return Array.from(page.value?.querySelectorAll<HTMLElement>('[data-settings-section]') ?? [])
  }

  function report(section: SettingsSection) {
    if (visible === section) return
    visible = section
    options.onVisible(section)
  }

  function updateVisible() {
    frame = 0
    if (!container || requested) return
    const elements = sections()
    const line = container.getBoundingClientRect().top + 24
    let active = elements[0]
    for (const element of elements) {
      if (element.getBoundingClientRect().top <= line) active = element
      else break
    }
    if (container.scrollTop > 0 && container.scrollTop + container.clientHeight >= container.scrollHeight - 2) {
      active = elements[elements.length - 1]
    }
    const key = active?.dataset.settingsSection
    if (isSettingsSection(key)) report(key)
  }

  function scheduleUpdate() {
    if (!frame) frame = requestAnimationFrame(updateVisible)
  }

  function finishScroll() {
    clearTimeout(settling)
    requested = null
    scheduleUpdate()
  }

  function handleScroll() {
    scheduleUpdate()
    if (requested) {
      clearTimeout(settling)
      settling = setTimeout(finishScroll, 180)
    }
  }

  async function scrollToSection(section: SettingsSection, behavior?: ScrollBehavior, targetId?: string | null) {
    const sectionElement = sections().find(item => item.dataset.settingsSection === section)
    const target = targetId ? document.getElementById(targetId) : null
    const element = target && sectionElement?.contains(target) ? target : sectionElement
    if (!container || !element) return
    const disclosures: HTMLElement[] = []
    for (let ancestor: HTMLElement | null = element; ancestor && ancestor !== sectionElement; ancestor = ancestor.parentElement) {
      if (ancestor.hasAttribute('data-settings-disclosure')) disclosures.unshift(ancestor)
    }
    for (const disclosure of disclosures) {
      disclosure.querySelector<HTMLButtonElement>(':scope > button[aria-expanded="false"]')?.click()
    }
    await nextTick()
    if (disposed || !container) return
    clearTimeout(highlightTimer)
    highlighted?.removeAttribute('data-search-highlight')
    highlighted = target && element === target && element !== sectionElement ? element : null
    if (highlighted) {
      highlighted.setAttribute('data-search-highlight', '')
      highlightTimer = setTimeout(() => highlighted?.removeAttribute('data-search-highlight'), 2400)
    }
    requested = section
    report(section)
    const top = container.scrollTop + element.getBoundingClientRect().top - container.getBoundingClientRect().top - 16
    container.scrollTo({
      top: Math.max(0, top),
      behavior: behavior ?? (window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth'),
    })
    // Covers reselecting an already aligned heading and engines without scrollend.
    clearTimeout(settling)
    settling = setTimeout(finishScroll, 180)
  }

  onMounted(async () => {
    await nextTick()
    if (disposed) return
    container = page.value?.closest<HTMLElement>('.app-workspace-scroll') ?? null
    if (!container || !page.value) return
    container.addEventListener('scroll', handleScroll, { passive: true })
    container.addEventListener('scrollend', finishScroll)
    container.addEventListener('wheel', finishScroll, { passive: true })
    container.addEventListener('touchstart', finishScroll, { passive: true })
    container.addEventListener('keydown', finishScroll)
    observer = new ResizeObserver(scheduleUpdate)
    observer.observe(page.value)
    observer.observe(container)
    const initial = options.initialSection()
    if (initial !== 'account' || options.hasInitialRequest()) scrollToSection(initial, 'auto')
    else {
      container.scrollTo({ top: 0, behavior: 'auto' })
      scheduleUpdate()
    }
  })

  onUnmounted(() => {
    disposed = true
    observer?.disconnect()
    cancelAnimationFrame(frame)
    clearTimeout(settling)
    clearTimeout(highlightTimer)
    highlighted?.removeAttribute('data-search-highlight')
    container?.removeEventListener('scroll', handleScroll)
    container?.removeEventListener('scrollend', finishScroll)
    container?.removeEventListener('wheel', finishScroll)
    container?.removeEventListener('touchstart', finishScroll)
    container?.removeEventListener('keydown', finishScroll)
  })

  return { scrollToSection }
}
