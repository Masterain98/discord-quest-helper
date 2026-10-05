import { runInNewContext } from 'node:vm'
import { describe, expect, it, vi } from 'vitest'
import { dispatchCdpResponse, isRecoveryTarget, recoveryExpression } from '../../scripts/repair-discord-game-cache.mjs'

function fixture(value) {
  let current = value
  let reads = 0
  let writes = 0
  let frames = 0
  const storage = {
    getItem(key) { expect(key).toBe('GameStore'); reads++; return current },
    setItem(key, value) { expect(key).toBe('GameStore'); writes++; current = value },
  }
  const document = { body: { append() { frames++ } }, createElement(tag) {
    expect(tag).toBe('iframe')
    return { style: {}, contentWindow: { localStorage: storage }, remove() { frames-- } }
  } }
  return { run: (apply, expected) => runInNewContext(recoveryExpression(apply, expected), { document }),
    value: () => current, writes: () => writes, frames: () => frames, reads: () => reads }
}

const valid = { id: '123', name: 'Example' }
const poisoned = { executables: [], aliases: [], thirdPartySkus: [] }

describe('recovery CDP transport', () => {
  const target = overrides => ({ type: 'page', url: 'https://discord.com/channels/@me',
    webSocketDebuggerUrl: 'ws://127.0.0.1:9223/devtools/page/main', ...overrides })

  it.each(['/app', '/channels/@me', '/quest-home', '/store', '/shop', '/shop/item'])(
    'accepts the Discord main renderer on %s', route => {
      for (const host of ['discord.com', 'discordapp.com']) {
        expect(isRecoveryTarget(target({ url: `https://${host}${route}` }), 9223)).toBe(true)
      }
    })

  it.each([
    { type: 'worker' }, { url: 'https://discord.com/overlay' }, { url: 'https://discord.com/popout' },
    { url: 'https://discord.com/storefront' }, { url: 'https://discord.com.attacker.example/shop' },
    { url: 'about:blank' }, { url: 'https://example.com/shop' },
    { webSocketDebuggerUrl: 'ws://example.com:9223/devtools/page/main' },
    { webSocketDebuggerUrl: 'ws://127.0.0.1:9999/devtools/page/main' },
    { webSocketDebuggerUrl: 'ws://user@127.0.0.1:9223/devtools/page/main' },
    { webSocketDebuggerUrl: 'ws://127.0.0.1:9223/devtools/browser/main' },
  ])('rejects unrelated pages or unsafe debugger endpoints: %j', overrides => {
    expect(isRecoveryTarget(target(overrides), 9223)).toBe(false)
  })

  it('dispatches only matching numeric requests, including responses received out of order', () => {
    const first = vi.fn()
    const second = vi.fn()
    const pending = new Map([[1, first], [2, second]])
    const response = { id: 2, result: { result: { value: 'inspection' } } }
    expect(dispatchCdpResponse(JSON.stringify(response), pending)).toBe(true)
    expect(second).toHaveBeenCalledWith(response)
    expect(first).not.toHaveBeenCalled()
    expect(dispatchCdpResponse('{"id":1,"error":{"message":"failed"}}', pending)).toBe(true)
    expect(first).toHaveBeenCalledWith({ id: 1, error: { message: 'failed' } })
  })

  it.each(['invalid JSON', 'null', '{}', '{"method":"Runtime.event"}',
    '{"id":"1"}', '{"id":"toString"}', '{"id":"__proto__"}', '{"id":0}',
    '{"id":-1}', '{"id":1.5}', '{"id":9007199254740992}', '{"id":999}'])(
    'ignores malformed messages, events and unsolicited IDs: %s', data => {
      const handler = vi.fn()
      expect(dispatchCdpResponse(data, new Map([[1, handler]]))).toBe(false)
      expect(handler).not.toHaveBeenCalled()
    })

  it('does not invoke a non-function pending entry', () => {
    expect(dispatchCdpResponse('{"id":1}', new Map([[1, {}]]))).toBe(false)
  })
})

describe('targeted Discord game cache recovery', () => {
  it.each([true, false])('preserves every healthy row and cache field (versioned=%s)', async versioned => {
    const state = { detectableGames: [valid, poisoned], detectableGamesEtag: 'original',
      blocklistEtag: 'blocklist', blocklistExecutables: ['blocked.exe'], blocklistPatterns: ['pattern'] }
    const raw = JSON.stringify(versioned ? { _state: state, _version: 4 } : state)
    const f = fixture(raw)
    const inspection = await f.run(false)
    expect(inspection.removed).toBe(1)
    expect(inspection.original).toBe(raw)
    expect(f.writes()).toBe(0)
    const result = await f.run(true, raw)
    expect(result.repaired).toBe(true)
    const repaired = JSON.parse(f.value())
    expect(repaired._state ?? repaired).toEqual({ ...state, detectableGames: [valid] })
    if (versioned) expect(repaired._version).toBe(4)
    expect(f.frames()).toBe(0)
  })
  it('never rewrites a healthy cache', async () => {
    const raw = JSON.stringify({ detectableGames: [valid] })
    const f = fixture(raw)
    expect((await f.run(true, raw)).removed).toBe(0)
    expect(f.writes()).toBe(0)
  })
  it('refuses to overwrite a cache that changed since its backup', async () => {
    const raw = JSON.stringify({ detectableGames: [poisoned] })
    const f = fixture(raw)
    await expect(f.run(true, 'old snapshot')).rejects.toThrow('Game cache changed')
    expect(f.value()).toBe(raw)
    expect(f.writes()).toBe(0)
    expect(f.frames()).toBe(0)
  })
  it.each(['invalid JSON', '{"unexpected":true}'])('leaves unknown cache data intact: %s', async raw => {
    const f = fixture(raw)
    await expect(f.run(false)).rejects.toThrow()
    expect(f.value()).toBe(raw)
    expect(f.writes()).toBe(0)
    expect(f.frames()).toBe(0)
  })
})
