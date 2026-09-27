import { afterEach, describe, expect, it } from 'vitest'
import {
  clearCdpLaunchError,
  invalidCdpPortError,
  recordCdpLaunchError,
  useCdpDiagnostics,
} from './cdpDiagnostics'

afterEach(() => clearCdpLaunchError())

describe('CDP launch diagnostics', () => {
  it('records and clears structured launch failures', () => {
    const recorded = recordCdpLaunchError({
      code: 'launch_failed',
      params: { port: 9223 },
      message: 'Launch failed',
    })

    expect(recorded.code).toBe('launch_failed')
    expect(useCdpDiagnostics().lastCdpLaunchError.value).toEqual(recorded)
    clearCdpLaunchError()
    expect(useCdpDiagnostics().lastCdpLaunchError.value).toBeNull()
  })

  it('creates a structured invalid-port failure', () => {
    expect(invalidCdpPortError(0)).toEqual({
      code: 'invalid_port',
      params: { port: 0 },
      message: 'Invalid port number. Must be between 1024 and 65535.',
    })
  })
})
