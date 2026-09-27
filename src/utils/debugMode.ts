export const DEBUG_MODE_STORAGE_KEY = 'debugMode'

type DebugModeStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

export function isDebugModeEnabled(storage: DebugModeStorage = localStorage): boolean {
  return storage.getItem(DEBUG_MODE_STORAGE_KEY) === 'true'
}

export function persistDebugMode(enabled: boolean, storage: DebugModeStorage = localStorage): void {
  if (enabled) {
    storage.setItem(DEBUG_MODE_STORAGE_KEY, 'true')
  } else {
    storage.removeItem(DEBUG_MODE_STORAGE_KEY)
  }
}
