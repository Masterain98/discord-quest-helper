// Platform-aware selection of a detectable game's executable.
//
// Discord's detectable-game metadata lists executables tagged by OS
// (`win32`, `linux`, `darwin`). Windows historically only ever looked at
// `win32`; these helpers generalize that so Linux can prefer a native `linux`
// executable and fall back to `win32` where it makes sense (CDP mode), while
// refusing to run a `win32`-only game through the Linux process simulator.

export interface DetectableExecutable {
  name: string
  os: string
}

/**
 * Pick the executables that match a platform's OS priority list.
 * Returns the first OS in `priority` that has at least one executable.
 */
export function getCompatibleExecutables(
  executables: DetectableExecutable[],
  priority: string[]
): { sourceOs: string | null; executables: DetectableExecutable[] } {
  for (const os of priority) {
    const matches = executables.filter((executable) => executable.os === os)
    if (matches.length > 0) {
      return { sourceOs: os, executables: matches }
    }
  }
  return { sourceOs: null, executables: [] }
}

/**
 * Executables the *process simulator* can actually launch on this host.
 *
 * Mirrors `resolveSimulationExecutable`'s platform rule: Linux only ever runs a
 * native `linux` executable, so win32 entries must not be offered there even
 * though they stay in `executableOsPriority` for CDP mode. Other hosts keep the
 * plain priority-order behavior.
 */
export function getSimulationExecutables(
  executables: DetectableExecutable[],
  hostOs: string,
  priority: string[]
): DetectableExecutable[] {
  if (hostOs === 'linux') {
    return executables.filter((executable) => executable.os === 'linux')
  }
  return getCompatibleExecutables(executables, priority).executables
}

export type SimulationExecutableResolution =
  | { kind: 'supported'; executable: DetectableExecutable }
  | { kind: 'win32_only_on_linux'; windowsExecutables: DetectableExecutable[] }
  | { kind: 'not_found' }

/**
 * Resolve which executable the *process simulator* should use.
 *
 * On non-Linux hosts this preserves today's behavior (the selected name, or the
 * first `win32` executable). On Linux it only accepts a native `linux`
 * executable; a `win32`-only game yields a recoverable `win32_only_on_linux`
 * result so the caller can surface a soft error and offer CDP mode instead of
 * running a Windows binary through Wine or a fake `.exe`.
 */
export function resolveSimulationExecutable(
  executables: DetectableExecutable[],
  hostOs: string,
  selectedName?: string
): SimulationExecutableResolution {
  if (hostOs !== 'linux') {
    const selected = selectedName
      ? executables.find((executable) => executable.name === selectedName)
      : executables.find((executable) => executable.os === 'win32')

    return selected ? { kind: 'supported', executable: selected } : { kind: 'not_found' }
  }

  if (selectedName) {
    const selected = executables.find((executable) => executable.name === selectedName)

    if (selected?.os === 'linux') {
      return { kind: 'supported', executable: selected }
    }

    if (selected?.os === 'win32') {
      return { kind: 'win32_only_on_linux', windowsExecutables: [selected] }
    }
  }

  const linuxExecutable = executables.find((executable) => executable.os === 'linux')
  if (linuxExecutable) {
    return { kind: 'supported', executable: linuxExecutable }
  }

  const windowsExecutables = executables.filter((executable) => executable.os === 'win32')
  if (windowsExecutables.length > 0) {
    return { kind: 'win32_only_on_linux', windowsExecutables }
  }

  return { kind: 'not_found' }
}
