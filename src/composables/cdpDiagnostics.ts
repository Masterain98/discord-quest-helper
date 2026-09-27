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

export function invalidCdpPortError(port: number) {
  return {
    code: 'invalid_port',
    params: { port },
    message: 'Invalid port number. Must be between 1024 and 65535.',
  }
}

export function useCdpDiagnostics() {
  return { lastCdpLaunchError, recordCdpLaunchError, clearCdpLaunchError }
}
