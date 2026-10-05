import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'
import { recoveryExpression } from '../../scripts/repair-discord-game-cache.mjs'

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
