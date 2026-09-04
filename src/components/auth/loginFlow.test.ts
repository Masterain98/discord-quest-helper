import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AuthProgress, CdpStatus, DesktopClientInventory } from '@/api/tauri'
import {
  canBeginLogin,
  classifyCdpAvailability,
  installedCdpLaunchTargets,
  presentAuthProgress,
  shouldAskCdpLaunchTarget,
  shouldPollCdp,
  startCdpPolling,
  usesVesktopForCdpLogin,
} from './loginFlow'

function inventory(overrides: Partial<DesktopClientInventory> = {}): DesktopClientInventory {
  return {
    officialInstalled: true,
    vesktopInstalled: true,
    officialRunning: false,
    vesktopRunning: false,
    cdpOwner: 'none',
    stableInstalled: true,
    ptbInstalled: false,
    canaryInstalled: false,
    stableRunning: false,
    ptbRunning: false,
    canaryRunning: false,
    ...overrides,
  }
}

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

describe('CDP login client copy', () => {
  it('uses Discord copy unless Vesktop is connected or the only available client', () => {
    expect(usesVesktopForCdpLogin(null)).toBe(false)
    expect(usesVesktopForCdpLogin(inventory())).toBe(false)
    expect(usesVesktopForCdpLogin(inventory({ officialRunning: true, vesktopRunning: true }))).toBe(false)
    expect(usesVesktopForCdpLogin(inventory({ officialInstalled: false, stableInstalled: false }))).toBe(true)
    expect(usesVesktopForCdpLogin(inventory({ vesktopRunning: true }))).toBe(false)
    expect(usesVesktopForCdpLogin(inventory({ officialInstalled: false, stableInstalled: false, vesktopRunning: true }))).toBe(true)
    expect(usesVesktopForCdpLogin(inventory({ cdpOwner: 'vesktop' }))).toBe(true)
  })
})

describe('CDP launch target selection', () => {
  it('lists only installed official channels and Vesktop', () => {
    expect(installedCdpLaunchTargets(null)).toEqual([])
    expect(installedCdpLaunchTargets(inventory())).toEqual(['stable', 'vesktop'])
    expect(installedCdpLaunchTargets(inventory({
      ptbInstalled: true,
      canaryInstalled: true,
    }))).toEqual(['stable', 'ptb', 'canary', 'vesktop'])
  })

  it('asks only when CDP is down and more than one client is installed', () => {
    expect(shouldAskCdpLaunchTarget(true, ['stable', 'vesktop'])).toBe(false)
    expect(shouldAskCdpLaunchTarget(false, ['stable'])).toBe(false)
    expect(shouldAskCdpLaunchTarget(false, ['stable', 'canary', 'vesktop'])).toBe(true)
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
