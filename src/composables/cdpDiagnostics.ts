import { ref } from 'vue'
import { parseCommandError, type ParsedCommandError } from '@/utils/commandError'

export interface TimestampedCommandError extends ParsedCommandError {
  timestamp: string
}

const lastCdpLaunchError = ref<TimestampedCommandError | null>(null)

export function recordCdpLaunchError(value: unknown): TimestampedCommandError {
  const error = { ...parseCommandError(value), timestamp: new Date().toISOString() }
  lastCdpLaunchError.value = error
  return error
}

export function clearCdpLaunchError() {
  lastCdpLaunchError.value = null
}

export function useCdpDiagnostics() {
  return { lastCdpLaunchError, recordCdpLaunchError, clearCdpLaunchError }
}
