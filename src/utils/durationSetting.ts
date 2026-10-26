import type { GameIdleDuration } from '@/api/tauri'

function isGameIdleDuration(value: unknown): value is GameIdleDuration {
  if (typeof value !== 'object' || value === null) return false
  const candidate = value as Partial<Record<keyof GameIdleDuration, unknown>>
  return (candidate.mode === 'fixed' || candidate.mode === 'random')
    && typeof candidate.fixed === 'number' && Number.isSafeInteger(candidate.fixed)
    && typeof candidate.min === 'number' && Number.isSafeInteger(candidate.min)
    && typeof candidate.max === 'number' && Number.isSafeInteger(candidate.max)
}

/**
 * Loads a persisted duration. Values saved before randomization existed are
 * plain numbers, so JSON objects and bare numbers are both accepted; anything
 * unreadable falls back to the default.
 */
export function loadGameIdleDuration(key: string, fallback: GameIdleDuration, minimum: number): GameIdleDuration {
  const raw = localStorage.getItem(key)
  if (raw === null) return { ...fallback }
  try {
    const parsed: unknown = JSON.parse(raw)
    if (isGameIdleDuration(parsed)) return clampGameIdleDuration(parsed, minimum)
  } catch {
    // Malformed values fall through to the legacy numeric path.
  }
  const legacy = Number(raw)
  return Number.isSafeInteger(legacy) && legacy >= minimum
    ? { mode: 'fixed', fixed: legacy, min: Math.max(minimum, fallback.min), max: Math.max(minimum, fallback.max) }
    : { ...fallback }
}

/** Mirrors the backend validation: fixed >= minimum, or minimum <= min <= max. */
export function isGameIdleDurationValid(value: GameIdleDuration, minimum: number): boolean {
  return value.mode === 'fixed'
    ? value.fixed >= minimum
    : value.min >= minimum && value.min <= value.max
}

/** Replaces invalid fields with safe bounds so a bad edit can never reach the backend. */
export function clampGameIdleDuration(value: GameIdleDuration, minimum: number): GameIdleDuration {
  const round = (raw: number, floor: number): number => {
    const rounded = Math.round(raw)
    return Number.isSafeInteger(rounded) && rounded >= floor ? rounded : floor
  }
  const fixed = round(value.fixed, minimum)
  const min = round(value.min, minimum)
  const max = round(value.max, min)
  // Returning the original reference when nothing changed lets sync watchers
  // compare by identity instead of re-triggering on a fresh clone.
  if (fixed === value.fixed && min === value.min && max === value.max) return value
  return { ...value, fixed, min, max }
}

export function resolveGameIdleDuration(value: GameIdleDuration): number {
  if (value.mode === 'fixed') return value.fixed
  const min = Math.min(value.min, value.max)
  const max = Math.max(value.min, value.max)
  return Math.floor(Math.random() * (max - min + 1)) + min
}
