import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { useSettingsSidebar } from './useSettingsSidebar'

const media = vi.hoisted(() => ({ wide: null as unknown as ReturnType<typeof ref<boolean>> }))
vi.mock('@vueuse/core', () => ({ useMediaQuery: () => media.wide }))

let values: Map<string, string>
beforeEach(() => {
  media.wide = ref(true)
  values = new Map()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  })
})
afterEach(() => vi.unstubAllGlobals())

describe('settings sidebar desktop preference', () => {
  it('starts expanded and restores an explicit collapsed preference on the next launch', () => {
    const sidebar = useSettingsSidebar()
    expect(sidebar.expanded.value).toBe(true)
    sidebar.toggle()
    expect(useSettingsSidebar().expanded.value).toBe(false)
  })

  it('keeps the desktop preference through narrow drawer interactions and resizing', () => {
    const sidebar = useSettingsSidebar()
    sidebar.toggle()
    media.wide.value = false
    sidebar.toggle()
    expect(sidebar.drawerOpen.value).toBe(true)
    sidebar.toggle()
    expect(sidebar.drawerOpen.value).toBe(false)
    media.wide.value = true
    expect(sidebar.expanded.value).toBe(false)
    expect(values.get('questHelper_settingsSidebarExpanded')).toBe('false')
  })

  it('does not store a preference merely because the mobile drawer was used', () => {
    media.wide.value = false
    const sidebar = useSettingsSidebar()
    sidebar.toggle()
    expect(values.size).toBe(0)
    expect(sidebar.expanded.value).toBe(true)
  })

  it('continues to toggle when storage is blocked', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => { throw new Error('blocked') },
      setItem: () => { throw new Error('blocked') },
    })
    const sidebar = useSettingsSidebar()
    expect(() => sidebar.toggle()).not.toThrow()
    expect(sidebar.expanded.value).toBe(false)
  })
})
