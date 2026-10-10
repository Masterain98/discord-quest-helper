interface Snapshot { mode: string; port: number; account: string | null }
interface ConsoleLifecycleDependencies {
  snapshot(): Snapshot
  check(port: number): Promise<{ connected: boolean }>
  setAvailable(available: boolean): void
  console: {
    opened: boolean
    connected: boolean
    connect(port: number): Promise<void>
    disconnect(clear?: boolean): Promise<void>
  }
}

/** Application-owned monitor, independent of the currently mounted tab. */
export function createDiscordConsoleLifecycle(deps: ConsoleLifecycleDependencies) {
  let revision = 0
  let inFlight: Promise<void> | null = null
  let stopped = false
  async function refresh() {
    if (stopped || deps.snapshot().mode !== 'cdp') return
    if (inFlight) return inFlight
    const captured = deps.snapshot()
    const generation = revision
    const work = (async () => {
      let available = false
      try { available = (await deps.check(captured.port)).connected } catch { /* unavailable */ }
      const current = deps.snapshot()
      if (stopped || generation !== revision || current.mode !== captured.mode || current.port !== captured.port || current.account !== captured.account) return
      deps.setAvailable(available)
      if (!available) await deps.console.disconnect()
      else if (deps.console.opened && !deps.console.connected) await deps.console.connect(captured.port)
    })()
    inFlight = work
    try { await work } finally { if (inFlight === work) inFlight = null }
  }
  async function invalidate(clear = false) { revision++; await deps.console.disconnect(clear) }
  async function stop() { stopped = true; await invalidate(true) }
  return { refresh, invalidate, stop }
}
