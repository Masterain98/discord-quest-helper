import { describe, expect, it } from 'vitest'
import { DEBUG_MODE_STORAGE_KEY, isDebugModeEnabled, persistDebugMode } from './debugMode'

function createMemoryStorage() {
  const values = new Map<string, string>()
  return {
    getItem: (key: string) => values.get(key) ?? null,
    removeItem: (key: string) => values.delete(key),
    setItem: (key: string, value: string) => values.set(key, value),
  }
}

describe('developer mode persistence', () => {
  it('restores the enabled state from the shared profile storage', () => {
    const storage = createMemoryStorage()

    persistDebugMode(true, storage)

    expect(storage.getItem(DEBUG_MODE_STORAGE_KEY)).toBe('true')
    expect(isDebugModeEnabled(storage)).toBe(true)
  })

  it('removes the persisted state when developer mode is disabled', () => {
    const storage = createMemoryStorage()
    persistDebugMode(true, storage)

    persistDebugMode(false, storage)

    expect(storage.getItem(DEBUG_MODE_STORAGE_KEY)).toBeNull()
    expect(isDebugModeEnabled(storage)).toBe(false)
  })
})
