import { describe, expect, it, vi } from 'vitest'
import { createDiscordConsoleLifecycle } from './discordConsoleLifecycle'
import { consoleFallbackTab, consoleTabEnabled } from '@/utils/discordConsole'

function setup() {
  const snapshot = { mode: 'cdp', port: 9223, account: 'one' as string | null }
  const console = { opened: false, connected: false, connect: vi.fn().mockResolvedValue(undefined), disconnect: vi.fn().mockResolvedValue(undefined) }
  const deps = { snapshot: () => ({ ...snapshot }), console, check: vi.fn().mockResolvedValue({ connected: true }), setAvailable: vi.fn() }
  return { snapshot, deps, lifecycle: createDiscordConsoleLifecycle(deps) }
}
describe('application console lifecycle', () => {
  it('shows the tab only in CDP mode with connectivity and redirects a hidden active tab', () => {
    for (const mode of ['simulate', 'heartbeat', 'cdp']) for (const available of [false, true]) {
      const enabled = consoleTabEnabled(mode, available)
      expect(enabled).toBe(mode === 'cdp' && available)
      expect(consoleFallbackTab('console', enabled)).toBe(enabled ? 'console' : 'settings')
      expect(consoleFallbackTab('game', enabled)).toBe('game')
    }
  })
  it('probes without opening a console until requested, and reconnects a previously opened console', async () => {
    const { lifecycle, deps } = setup()
    await lifecycle.refresh()
    expect(deps.console.connect).not.toHaveBeenCalled()
    deps.console.opened = true
    await lifecycle.refresh()
    expect(deps.console.connect).toHaveBeenCalledWith(9223)
    deps.console.connected = true
    await lifecycle.refresh()
    expect(deps.console.connect).toHaveBeenCalledTimes(1)
  })
  it('hides the tab on endpoint failure without changing task mode or clearing records', async () => {
    const { lifecycle, deps, snapshot } = setup()
    deps.check.mockRejectedValue(new Error('offline'))
    await lifecycle.refresh()
    expect(deps.setAvailable).toHaveBeenCalledWith(false)
    expect(deps.console.disconnect).toHaveBeenCalledWith()
    expect(snapshot.mode).toBe('cdp')
  })
  it('discards an old probe after port/account changes and prevents overlapping probes', async () => {
    const { lifecycle, deps, snapshot } = setup()
    let resolve!: (value: { connected: boolean }) => void
    deps.check.mockReturnValue(new Promise(r => { resolve = r }))
    const first = lifecycle.refresh()
    const second = lifecycle.refresh()
    expect(deps.check).toHaveBeenCalledTimes(1)
    snapshot.port = 9224
    snapshot.account = 'two'
    await lifecycle.invalidate(true)
    resolve({ connected: true })
    await Promise.all([first, second])
    expect(deps.setAvailable).not.toHaveBeenCalled()
    expect(deps.console.disconnect).toHaveBeenCalledWith(true)
  })
  it('stops polling and clears session state at app shutdown', async () => {
    const { lifecycle, deps, snapshot } = setup()
    snapshot.mode = 'simulate'
    await lifecycle.refresh()
    expect(deps.check).not.toHaveBeenCalled()
    await lifecycle.stop()
    snapshot.mode = 'cdp'
    await lifecycle.refresh()
    expect(deps.check).not.toHaveBeenCalled()
    expect(deps.console.disconnect).toHaveBeenCalledWith(true)
  })
})
