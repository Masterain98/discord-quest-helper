import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AuthProgress, CdpStatus } from '@/api/tauri'
import {
  canBeginLogin,
  classifyCdpAvailability,
  presentAuthProgress,
  shouldPollCdp,
  startCdpPolling,
} from './loginFlow'

function progress(overrides: Partial<AuthProgress>): AuthProgress {
  return {
    phase: 'extracting_tokens',
    current: null,
    total: null,
    valid_accounts: null,
    ...overrides,
  }
}

const offline: CdpStatus = {
  available: false,
  connected: false,
  target_title: null,
  error: 'connection refused',
}

describe('login progress presentation', () => {
  it('preserves real token validation counts', () => {
    expect(presentAuthProgress(progress({
      phase: 'validating_tokens',
      current: 3,
      total: 7,
    }))).toEqual({
      key: 'auth.progress.validating_tokens',
      params: { current: 3, total: 7 },
      state: 'running',
    })
  })

  it('marks account discovery and completion as successful terminal states', () => {
    expect(presentAuthProgress(progress({
      phase: 'accounts_found',
      current: 4,
      total: 4,
      valid_accounts: 2,
    })).state).toBe('success')
    expect(presentAuthProgress(progress({ phase: 'complete' })).state).toBe('success')
  })

  it('marks an empty scan as an error state', () => {
    expect(presentAuthProgress(progress({
      phase: 'accounts_found',
      current: 0,
      total: 0,
      valid_accounts: 0,
    })).state).toBe('error')
  })
})

describe('CDP status and polling', () => {
  afterEach(() => vi.useRealTimers())

  it('distinguishes ready, starting, offline, and probe failure states', () => {
    expect(classifyCdpAvailability(true, null, false)).toBe('checking')
    expect(classifyCdpAvailability(false, { ...offline, available: true }, false)).toBe('starting')
    expect(classifyCdpAvailability(false, { ...offline, available: true, connected: true }, false)).toBe('ready')
    expect(classifyCdpAvailability(false, offline, false)).toBe('offline')
    expect(classifyCdpAvailability(false, null, true)).toBe('error')
  })

  it('pauses polling while busy, authenticated, or hidden', () => {
    expect(shouldPollCdp({ busy: false, authenticated: false, visible: true })).toBe(true)
    expect(shouldPollCdp({ busy: true, authenticated: false, visible: true })).toBe(false)
    expect(shouldPollCdp({ busy: false, authenticated: true, visible: true })).toBe(false)
    expect(shouldPollCdp({ busy: false, authenticated: false, visible: false })).toBe(false)
  })

  it('runs every five seconds and can be disposed', () => {
    vi.useFakeTimers()
    const callback = vi.fn()
    const stop = startCdpPolling(callback)

    vi.advanceTimersByTime(15_000)
    expect(callback).toHaveBeenCalledTimes(3)

    stop()
    vi.advanceTimersByTime(5_000)
    expect(callback).toHaveBeenCalledTimes(3)
  })
})

describe('login operation gate', () => {
  it('rejects repeated actions until the active operation is released', () => {
    expect(canBeginLogin(null, false)).toBe(true)
    expect(canBeginLogin('local', false)).toBe(false)
    expect(canBeginLogin(null, true)).toBe(false)
    expect(canBeginLogin(null, false)).toBe(true)
  })
})
