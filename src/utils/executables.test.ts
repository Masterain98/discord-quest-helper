import { describe, it, expect } from 'vitest'
import {
  getCompatibleExecutables,
  resolveSimulationExecutable,
  type DetectableExecutable,
} from './executables'

const win: DetectableExecutable = { name: 'game.exe', os: 'win32' }
const lin: DetectableExecutable = { name: 'game', os: 'linux' }
const mac: DetectableExecutable = { name: 'game.app', os: 'darwin' }

describe('getCompatibleExecutables', () => {
  it('prefers the first OS in the priority list that has a match', () => {
    const result = getCompatibleExecutables([win, lin], ['linux', 'win32'])
    expect(result.sourceOs).toBe('linux')
    expect(result.executables).toEqual([lin])
  })

  it('falls back to the next OS when the first has no match', () => {
    const result = getCompatibleExecutables([win], ['linux', 'win32'])
    expect(result.sourceOs).toBe('win32')
    expect(result.executables).toEqual([win])
  })

  it('returns an empty result when nothing matches', () => {
    const result = getCompatibleExecutables([mac], ['linux', 'win32'])
    expect(result.sourceOs).toBeNull()
    expect(result.executables).toEqual([])
  })
})

describe('resolveSimulationExecutable', () => {
  it('non-Linux hosts keep the win32-first behavior', () => {
    expect(resolveSimulationExecutable([win, lin], 'win32')).toEqual({
      kind: 'supported',
      executable: win,
    })
  })

  it('non-Linux hosts honor an explicit selection', () => {
    expect(resolveSimulationExecutable([win, lin], 'darwin', 'game')).toEqual({
      kind: 'supported',
      executable: lin,
    })
  })

  it('Linux prefers a native linux executable', () => {
    expect(resolveSimulationExecutable([win, lin], 'linux')).toEqual({
      kind: 'supported',
      executable: lin,
    })
  })

  it('Linux reports win32-only games as a recoverable result', () => {
    expect(resolveSimulationExecutable([win], 'linux')).toEqual({
      kind: 'win32_only_on_linux',
      windowsExecutables: [win],
    })
  })

  it('Linux flags an explicitly selected win32 executable', () => {
    expect(resolveSimulationExecutable([win, lin], 'linux', 'game.exe')).toEqual({
      kind: 'win32_only_on_linux',
      windowsExecutables: [win],
    })
  })

  it('reports not_found when there is no usable executable', () => {
    expect(resolveSimulationExecutable([mac], 'linux')).toEqual({ kind: 'not_found' })
    expect(resolveSimulationExecutable([], 'win32')).toEqual({ kind: 'not_found' })
  })
})
