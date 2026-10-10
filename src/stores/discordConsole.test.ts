import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { ConsoleEvent, ConsoleSession } from '@/api/discordConsole'
import { useDiscordConsoleStore } from './discordConsole'

const api = vi.hoisted(() => ({
  openDiscordConsole: vi.fn(), evaluateDiscordConsole: vi.fn(), getDiscordConsoleProperties: vi.fn(),
  releaseDiscordConsoleObjects: vi.fn(), closeDiscordConsole: vi.fn(),
}))
vi.mock('@/api/discordConsole', () => api)
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(r => { resolve = r })
  return { promise, resolve }
}
let receive: (event: ConsoleEvent) => void
const info = (sessionId = 'one'): ConsoleSession => ({ sessionId, port: 9223, targetTitle: 'Discord', targetUrl: 'https://discord.com/channels/@me' })
function event(sequence: number, args: ConsoleEvent['args'] = [], sessionId = 'one'): ConsoleEvent {
  return { sessionId, sequence, args, timestamp: 1, kind: 'console', level: 'log' }
}
beforeEach(() => {
  setActivePinia(createPinia())
  vi.resetAllMocks()
  api.openDiscordConsole.mockImplementation(async (_port, callback) => { receive = callback; return info() })
  api.closeDiscordConsole.mockResolvedValue(undefined)
  api.releaseDiscordConsoleObjects.mockResolvedValue(undefined)
})

describe('Discord console session', () => {
  it('freezes output while receiving logs and execution results, and resumes in order', async () => {
    api.evaluateDiscordConsole.mockResolvedValue({ result: { type: 'number', value: 42 } })
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'string', value: 'before' }]))
    store.togglePaused()
    receive(event(2, [{ type: 'string', value: 'during' }]))
    store.input = '21 * 2'
    await store.execute()
    store.input = 'draft'
    const same = useDiscordConsoleStore()
    expect(same.paused).toBe(true)
    expect(same.input).toBe('draft')
    expect(same.visibleEntries.map(e => e.text)).toEqual(['before'])
    expect(same.pendingCount).toBe(3)
    expect(same.connected).toBe(true)
    same.togglePaused()
    expect(same.entries.map(e => e.text)).toEqual(['before', 'during', '21 * 2', '42'])
    expect(same.pendingCount).toBe(0)
    expect(api.closeDiscordConsole).not.toHaveBeenCalled()
  })

  it('clears both paused output and buffered objects without resuming, and resets on logout', async () => {
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'object', objectId: 'shown' }]))
    store.togglePaused()
    receive(event(2, [{ type: 'object', objectId: 'buffered' }]))
    store.clearOutput()
    expect(store.entries).toEqual([])
    expect(store.pendingCount).toBe(0)
    expect(store.paused).toBe(true)
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledWith('one', expect.arrayContaining(['shown', 'buffered']))
    receive(event(3, [{ type: 'string', value: 'next' }]))
    expect(store.visibleEntries).toEqual([])
    expect(store.pendingCount).toBe(1)
    await store.disconnect(true)
    expect(store.paused).toBe(false)
    expect(store.pendingCount).toBe(0)
    expect(store.entries).toEqual([])
  })

  it('bounds the pause buffer and releases handles only after the frozen owner is removed', async () => {
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'object', objectId: 'shared' }]))
    store.togglePaused()
    receive(event(2, [{ type: 'object', objectId: 'shared' }]))
    receive(event(3, [{ type: 'object', objectId: 'buffer-only' }]))
    for (let n = 4; n <= 2003; n++) receive(event(n, [{ type: 'number', value: n }]))
    expect(store.pendingCount).toBe(2000)
    expect(store.entries).toHaveLength(1)
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledTimes(1)
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledWith('one', ['buffer-only'])
    store.togglePaused()
    expect(store.entries).toHaveLength(2000)
    expect(store.entries[0].text).toBe('4')
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledWith('one', ['shared'])
  })

  it('buffers events delivered before the open response and preserves data without a mounted view', async () => {
    api.openDiscordConsole.mockImplementation(async (_port, callback) => { receive = callback; callback(event(1, [{ type: 'string', value: 'early' }])); return info() })
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    store.input = 'draft'
    receive(event(2, [{ type: 'number', value: 42 }]))
    const same = useDiscordConsoleStore()
    expect(same.entries.map(e => e.text)).toEqual(['early', '42'])
    expect(same.input).toBe('draft')
    await same.connect(9223)
    expect(api.openDiscordConsole).toHaveBeenCalledTimes(1)
  })

  it('closes a late connection after logout and discards all late events', async () => {
    const pending = deferred<ConsoleSession>()
    api.openDiscordConsole.mockImplementation((_port, callback) => { receive = callback; return pending.promise })
    const store = useDiscordConsoleStore()
    const connection = store.connect(9223)
    store.input = 'private draft'
    await store.disconnect(true)
    pending.resolve(info())
    await connection
    receive(event(1, [{ type: 'string', value: 'old account' }]))
    expect(store.session).toBeNull()
    expect(store.entries).toEqual([])
    expect(store.input).toBe('')
    expect(store.opened).toBe(false)
    expect(api.closeDiscordConsole).toHaveBeenCalledWith('one')
  })

  it('streams logs during pending evaluation, ignores duplicate and foreign events, and serializes execution', async () => {
    const pending = deferred<{ result: { type: string; value: number } }>()
    api.evaluateDiscordConsole.mockReturnValue(pending.promise)
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    store.input = 'await Promise.resolve(42)'
    const execution = store.execute()
    store.input = 'next'
    await store.execute()
    expect(api.evaluateDiscordConsole).toHaveBeenCalledTimes(1)
    receive(event(1, [{ type: 'string', value: 'during' }]))
    receive(event(1, [{ type: 'string', value: 'duplicate' }]))
    receive(event(2, [{ type: 'string', value: 'foreign' }], 'other'))
    expect(store.entries.map(e => e.kind)).toEqual(['command', 'console'])
    pending.resolve({ result: { type: 'number', value: 42 } })
    await execution
    expect(store.entries[store.entries.length - 1]?.text).toBe('42')
    expect(store.input).toBe('next')
    expect(store.executing).toBe(false)
  })

  it('invalidates references and a pending command on disconnect, without replaying it', async () => {
    const pending = deferred<{ result: { type: string; value: number } }>()
    api.evaluateDiscordConsole.mockReturnValue(pending.promise)
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'object', objectId: 'root', description: 'Object' }]))
    const entry = store.entries[0]
    store.input = 'slow()'
    const execution = store.execute()
    receive({ ...event(2), kind: 'disconnected', message: 'console_session_invalidated' })
    expect(store.connected).toBe(false)
    await expect(store.expand(entry, 'root')).rejects.toThrow('console_object_expired')
    pending.resolve({ result: { type: 'number', value: 99 } })
    await execution
    expect(store.entries.some(e => e.text === '99')).toBe(false)
    expect(store.executing).toBe(false)
    expect(api.evaluateDiscordConsole).toHaveBeenCalledTimes(1)
  })

  it('reads property descriptors without calling accessors and releases nested references on clear', async () => {
    api.getDiscordConsoleProperties.mockResolvedValue({ result: [
      { name: 'child', value: { type: 'object', objectId: 'child' } },
      { name: 'secret', get: { type: 'function', objectId: 'getter' } },
    ] })
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'object', objectId: 'root' }]))
    const entry = store.entries[0]
    await store.expand(entry, 'root')
    await store.expand(entry, 'root')
    expect(api.getDiscordConsoleProperties).toHaveBeenCalledTimes(1)
    expect(api.evaluateDiscordConsole).not.toHaveBeenCalled()
    store.clearOutput()
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledWith('one', expect.arrayContaining(['root', 'child', 'getter']))
    expect(store.entries).toEqual([])
  })

  it('releases results of property requests whose entry was removed while waiting', async () => {
    const pending = deferred<{ result: Array<{ name: string; value: { type: string; objectId: string } }> }>()
    api.getDiscordConsoleProperties.mockReturnValue(pending.promise)
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'object', objectId: 'root' }]))
    const expansion = store.expand(store.entries[0], 'root')
    store.clearOutput()
    pending.resolve({ result: [{ name: 'child', value: { type: 'object', objectId: 'child' } }] })
    await expect(expansion).rejects.toThrow('console_object_expired')
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledWith('one', ['child'])
  })

  it('bounds output at 2000 records and does not release shared handles while still retained', async () => {
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'object', objectId: 'shared' }]))
    receive(event(2, [{ type: 'object', objectId: 'shared' }]))
    for (let n = 3; n <= 2001; n++) receive(event(n, [{ type: 'number', value: n }]))
    expect(store.entries).toHaveLength(2000)
    expect(api.releaseDiscordConsoleObjects).not.toHaveBeenCalled()
    receive(event(2002, [{ type: 'number', value: 2002 }]))
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledWith('one', ['shared'])
  })

  it('retains 100 history entries and restores the draft after browsing history', async () => {
    api.evaluateDiscordConsole.mockResolvedValue({ result: { type: 'undefined' } })
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    for (let n = 0; n < 101; n++) { store.input = `${n}`; await store.execute() }
    expect(store.history).toHaveLength(100)
    expect(store.history[0]).toBe('1')
    store.input = 'unfinished\ncommand'
    store.recall(-1)
    expect(store.input).toBe('100')
    store.recall(1)
    expect(store.input).toBe('unfinished\ncommand')
    await store.disconnect(true)
    expect(store.history).toEqual([])
  })

  it('keeps expanded children alive when another retained entry references the same root', async () => {
    api.getDiscordConsoleProperties.mockResolvedValue({ result: [{ name: 'child', value: { type: 'object', objectId: 'child' } }] })
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive(event(1, [{ type: 'object', objectId: 'root' }]))
    await store.expand(store.entries[0], 'root')
    receive(event(2, [{ type: 'object', objectId: 'root' }]))
    for (let n = 3; n <= 2001; n++) receive(event(n, [{ type: 'number', value: n }]))
    expect(api.releaseDiscordConsoleObjects).not.toHaveBeenCalled()
    await store.expand(store.entries[0], 'root')
    expect(api.getDiscordConsoleProperties).toHaveBeenCalledTimes(1)
    store.clearOutput()
    expect(api.releaseDiscordConsoleObjects).toHaveBeenCalledWith('one', expect.arrayContaining(['root', 'child']))
  })

  it('filters full output and reports errors without exposing unexpected IPC error strings', async () => {
    const store = useDiscordConsoleStore()
    await store.connect(9223)
    receive({ ...event(1, [{ type: 'string', value: 'warn marker' }]), level: 'warn' })
    receive(event(2, [{ type: 'string', value: 'log marker' }]))
    store.level = 'warn'
    store.search = 'MARKER'
    expect(store.visibleEntries).toHaveLength(1)
    api.evaluateDiscordConsole.mockRejectedValue('unexpected sensitive error')
    store.input = 'bad()'
    await store.execute()
    expect(store.lastError).toBe('console_protocol_error')
    expect(store.entries[store.entries.length - 1]?.text).toBe('console_protocol_error')
  })
})
