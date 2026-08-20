export interface RunningCdpSession {
  channel: 'stable' | 'ptb' | 'canary'
  port: number
}

export interface CloseRequestEvent {
  preventDefault(): void
}

export interface ExitGuardState {
  checking: boolean
  dialogOpen: boolean
  closing: boolean
}

export interface ExitGuardDependencies {
  listSessions(): Promise<RunningCdpSession[]>
  startRestoreHelper(): Promise<void>
  prepareExit(): Promise<void>
  exitApplication(): Promise<void>
  showError(message: string): Promise<void>
  logError(error: unknown): void
  onStateChange(state: ExitGuardState): void
  /** Prevent a hung prepare IPC from stalling the close UI. Backend exit still retries CDP cleanup. */
  exitPreparationTimeoutMs?: number
}

export function createAppExitGuard(dependencies: ExitGuardDependencies) {
  const state: ExitGuardState = { checking: false, dialogOpen: false, closing: false }
  let closeRequestActive = false

  function publish() {
    dependencies.onStateChange({ ...state })
  }

  async function prepareExitWithinDeadline() {
    const timeoutMs = dependencies.exitPreparationTimeoutMs ?? 3_000
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      await Promise.race([
        dependencies.prepareExit(),
        new Promise<void>((_, reject) => {
          timer = setTimeout(() => reject(new Error('App exit cleanup timed out')), timeoutMs)
        }),
      ])
    } finally {
      if (timer !== undefined) clearTimeout(timer)
    }
  }

  async function closeApplication() {
    if (state.closing) return
    state.checking = false
    state.closing = true
    state.dialogOpen = false
    publish()
    try {
      await prepareExitWithinDeadline()
    } catch (error) {
      dependencies.logError(error)
    }
    try {
      await dependencies.exitApplication()
    } catch (error) {
      // A failed exit IPC must not make future close attempts no-op.
      dependencies.logError(error)
      state.closing = false
      publish()
    }
  }

  async function requestClose(event: CloseRequestEvent) {
    event.preventDefault()
    if (closeRequestActive || state.dialogOpen || state.closing) return
    closeRequestActive = true
    state.checking = true
    publish()
    try {
      const sessions = await dependencies.listSessions()
      if (sessions.length > 0) {
        state.dialogOpen = true
        return
      }
      await closeApplication()
    } catch (error) {
      dependencies.logError(error)
      await closeApplication()
    } finally {
      state.checking = false
      closeRequestActive = false
      publish()
    }
  }

  async function closeOnly() {
    await closeApplication()
  }

  async function restoreAndClose() {
    if (state.closing) return
    state.checking = true
    publish()
    try {
      await dependencies.startRestoreHelper()
    } catch (error) {
      dependencies.logError(error)
      try {
        await dependencies.showError(String(error))
      } catch (dialogError) {
        dependencies.logError(dialogError)
      }
    }
    await closeApplication()
  }

  publish()
  return { requestClose, closeOnly, restoreAndClose }
}
