import { describe, expect, it, vi } from 'vitest'
import { createAppExitGuard, type ExitGuardDependencies, type ExitGuardState } from './appExitGuard'

function harness(overrides: Partial<ExitGuardDependencies> = {}) {
  const states: ExitGuardState[] = []
  const dependencies: ExitGuardDependencies = {
    listSessions: vi.fn().mockResolvedValue([]),
    startRestoreHelper: vi.fn().mockResolvedValue(undefined),
    prepareExit: vi.fn().mockResolvedValue(undefined),
    exitApplication: vi.fn().mockResolvedValue(undefined),
    showError: vi.fn().mockResolvedValue(undefined),
    logError: vi.fn(),
    onStateChange: state => states.push(state),
    ...overrides,
  }
  return { guard: createAppExitGuard(dependencies), dependencies, states }
}

describe('app exit guard', () => {
  it('closes directly when no Discord CDP session exists', async () => {
    const { guard, dependencies } = harness()
    const event = { preventDefault: vi.fn() }
    await guard.requestClose(event)
    expect(event.preventDefault).toHaveBeenCalledOnce()
    expect(dependencies.prepareExit).toHaveBeenCalledOnce()
    expect(dependencies.exitApplication).toHaveBeenCalledOnce()
  })

  it('fails open when CDP detection fails', async () => {
    const { guard, dependencies } = harness({
      listSessions: vi.fn().mockRejectedValue(new Error('scan failed')),
    })
    await guard.requestClose({ preventDefault: vi.fn() })
    expect(dependencies.logError).toHaveBeenCalled()
    expect(dependencies.exitApplication).toHaveBeenCalledOnce()
  })

  it('shows one prompt for repeated close requests when a session exists', async () => {
    let resolve!: (value: unknown[]) => void
    const listSessions = vi.fn(() => new Promise<unknown[]>(done => { resolve = done }))
    const { guard, dependencies, states } = harness({ listSessions: listSessions as ExitGuardDependencies['listSessions'] })
    const first = guard.requestClose({ preventDefault: vi.fn() })
    await guard.requestClose({ preventDefault: vi.fn() })
    resolve([{ channel: 'stable', port: 9223 }])
    await first
    expect(listSessions).toHaveBeenCalledOnce()
    expect(dependencies.exitApplication).not.toHaveBeenCalled()
    expect(states.some(state => state.dialogOpen)).toBe(true)
  })

  it('starts the detached helper before closing', async () => {
    const { guard, dependencies } = harness()
    await guard.restoreAndClose()
    expect(dependencies.startRestoreHelper).toHaveBeenCalledOnce()
    expect(dependencies.prepareExit).toHaveBeenCalledOnce()
    expect(dependencies.exitApplication).toHaveBeenCalledOnce()
  })

  it('shows a helper launch error before still closing', async () => {
    const order: string[] = []
    const { guard } = harness({
      startRestoreHelper: vi.fn(async () => { throw new Error('spawn failed') }),
      showError: vi.fn(async () => { order.push('error') }),
      prepareExit: vi.fn(async () => { order.push('prepare') }),
      exitApplication: vi.fn(async () => { order.push('exit') }),
    })
    await guard.restoreAndClose()
    expect(order).toEqual(['error', 'prepare', 'exit'])
  })

  it('exits the application when exit cleanup does not settle', async () => {
    const { guard, dependencies } = harness({
      prepareExit: vi.fn(() => new Promise<void>(() => {})),
      exitPreparationTimeoutMs: 1,
    })
    await guard.restoreAndClose()
    expect(dependencies.exitApplication).toHaveBeenCalledOnce()
  })

  it('unlocks a failed exit so a later close request can retry', async () => {
    const exitApplication = vi
      .fn<ExitGuardDependencies['exitApplication']>()
      .mockRejectedValueOnce(new Error('exit IPC failed'))
      .mockResolvedValueOnce(undefined)
    const { guard, dependencies } = harness({ exitApplication })
    await guard.closeOnly()
    await guard.closeOnly()
    expect(dependencies.exitApplication).toHaveBeenCalledTimes(2)
  })

  it('still exits when the helper launch error dialog fails', async () => {
    const { guard, dependencies } = harness({
      startRestoreHelper: vi.fn().mockRejectedValue(new Error('spawn failed')),
      showError: vi.fn().mockRejectedValue(new Error('dialog failed')),
    })
    await guard.restoreAndClose()
    expect(dependencies.logError).toHaveBeenCalledTimes(2)
    expect(dependencies.exitApplication).toHaveBeenCalledOnce()
  })
})
