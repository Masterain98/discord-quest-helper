import { atob, btoa } from 'node:buffer'
import { createHash, webcrypto } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { TextEncoder } from 'node:util'
import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

const fixtureUserId = '456'
const fixtureHardwareToken = 'test-hardware-token'
const source = readFileSync('src-tauri/src/cdp_quest.rs', 'utf8')
const startTemplate = source.match(/fn js_spoof_play_game_for[\s\S]*?r#"([\s\S]*?)"#/)[1]
const start = Object.entries({
  safe_app_id: '"123"', safe_app_name: '"Example"', host_os_json: '"win32"',
  os_priority_json: '["win32"]', path_templates_json: '{"cmdLine":"{exe}","exePath":"{exe}"}',
  hints_json: '[]', host_hw_token_json: JSON.stringify(fixtureHardwareToken), unix_host: 'false',
}).reduce((js, [key, value]) => js.replaceAll(`{${key}}`, value), startTemplate)
  .replaceAll('{{', '{').replaceAll('}}', '}')
const cleanup = source.match(/const JS_CLEANUP_SPOOF: &str = r#"([\s\S]*?)"#;/)[1]

const game = () => ({ id: '123', name: 'Example', executables: [{ name: 'example.exe', os: 'win32' }],
  aliases: ['Example game'], thirdPartySkus: [{ id: '42', distributor: 'steam' }] })

function xorFingerprintPayload(bytes, userId) {
  const payload = Buffer.from(bytes)
  const key = createHash('sha256').update(userId + 'https://discord.com/careers').digest()
  for (let i = 1; i < payload.length; i++) payload[i] ^= key[(i - 1) & 31]
  return payload
}

function sourceFingerprint(hardware, flags) {
  const payload = Buffer.alloc(146)
  payload[0] = 1
  payload[1] = flags
  payload.writeBigUInt64LE(10_000_000n, 0x02)
  payload.writeUInt32LE(1_704_067_200, 0x0A)
  payload.writeUInt32LE(0x4000, 0x0E)
  hardware.copy(payload, 0x32)
  return xorFingerprintPayload(payload, fixtureUserId).toString('base64')
}

async function exercise(raw, cached, missingStoreOnStop = false, fingerprintOptions = {}) {
  const persisted = new Map()
  const events = []
  // Discord inserts the row BEFORE name.toLowerCase(), so a caught exception
  // still poisons the next startup. Keep that ordering in this regression.
  const dispatcher = { dispatch(event) {
    events.push(event)
    if (event.type !== 'GAMES_DATABASE_UPDATE') return
    for (const row of event.games) {
      const saved = { id: row.id, name: row.name, thirdPartySkus: row.third_party_skus ?? [] }
      persisted.set(row.id, saved)
      saved.name.toLowerCase()
    }
  }, subscribe() {}, unsubscribe() {} }
  const store = { getRunningGames: () => fingerprintOptions.runningGames ?? [], getGameForPID: () => null }
  const dqh = { initialized: true, RunningGameStore: store, FluxDispatcher: dispatcher,
    DetectableGameStore: { games: raw }, NativeUtils: { setObservedGamesCallback() {} },
    _origGetRunningGames: store.getRunningGames, _origGetGameForPID: store.getGameForPID,
    api: { get: async () => ({ body: [] }) }, _detectableGamesPayload: cached }
  const chunks = []
  chunks.push = () => ({ c: fingerprintOptions.userId ? {
    user: { exports: { default: { getCurrentUser: () => ({ id: fingerprintOptions.userId }) } } },
  } : {} })
  const context = { window: { __dqh_cdp: dqh }, webpackChunkdiscord_app: chunks,
    ...(fingerprintOptions.crypto ? { crypto: fingerprintOptions.crypto, TextEncoder, atob, btoa } : {}),
    setTimeout: callback => callback() }
  const result = JSON.parse(await runInNewContext(start, context))
  const fingerprint = dqh._fakeGame?.executableFingerprint
  // Simulate a legacy bridge remembering a poisoned payload before Stop.
  if (cached) dqh._detectableGamesPayload = cached
  if (missingStoreOnStop) delete dqh.DetectableGameStore
  const stopped = JSON.parse(await runInNewContext(cleanup, context))
  return { result, stopped, events, persisted, context, fingerprint }
}

describe('CDP game database persistence', () => {
  it.each([
    () => ({ locale: 'en', ast: [] }),
    { locale: 'en', ast: [] },
    [{ executables: [], aliases: [], thirdPartySkus: [] }],
    [{ ...game(), executables: [{ os: 'win32' }] }],
    [{ ...game(), aliases: [null] }],
    [game(), null],
  ])('never dispatches an invalid catalogue during Start or Stop: %s', async raw => {
    const { result, stopped, events, persisted } = await exercise(raw, ['en', { ast: [] }])
    expect(result.success, result.error).toBe(true)
    expect(stopped.success).toBe(true)
    expect(events.filter(event => event.type === 'GAMES_DATABASE_UPDATE')).toEqual([])
    expect(persisted.size).toBe(0)
  })

  it('preserves third-party identity across observer registration and cleanup', async () => {
    const row = game()
    const { result, stopped, events, persisted, context } = await exercise([row])
    expect(result.success, result.error).toBe(true)
    expect(stopped.success).toBe(true)
    expect(events.filter(event => event.type === 'GAMES_DATABASE_UPDATE')).toHaveLength(2)
    expect(persisted.get('123').thirdPartySkus).toEqual(row.thirdPartySkus)
    expect(row).not.toHaveProperty('third_party_skus')
    expect(context.window.__dqh_cdp).toBeUndefined()
  })

  it('rejects a legacy cached payload when the database store is unavailable during Stop', async () => {
    const { stopped, events, persisted } = await exercise(undefined, ['en', { ast: [] }], true)
    expect(stopped.success).toBe(true)
    expect(events.filter(event => event.type === 'GAMES_DATABASE_UPDATE')).toEqual([])
    expect(persisted.size).toBe(0)
  })
})

describe('CDP executable fingerprints', () => {
  const fallbackHardware = () => createHash('sha256')
    .update(`${fixtureHardwareToken}:${fixtureUserId}`).digest()

  it('generates and decrypts the renderer hardware fallback', async () => {
    const { result, stopped, fingerprint } = await exercise([game()], undefined, false, {
      userId: fixtureUserId, crypto: webcrypto,
    })
    expect(result.success, result.error).toBe(true)
    expect(result.hasFingerprint, result.fingerprintError).toBe(true)
    expect(result.fingerprintError).toBeNull()
    expect(stopped.success, stopped.error).toBe(true)
    const payload = xorFingerprintPayload(Buffer.from(fingerprint, 'base64'), fixtureUserId)
    expect(payload).toHaveLength(146)
    expect(payload[0]).toBe(1)
    expect(payload[1]).toBe(3)
    expect(payload.subarray(0x32, 0x52)).toEqual(fallbackHardware())
    expect(payload.readBigUInt64LE(0x02)).toBeGreaterThanOrEqual(20n * 1024n * 1024n)
    expect(payload.readBigUInt64LE(0x02)).toBeLessThan(140n * 1024n * 1024n)
  })

  it.each([0x02, 0x03])('harvests hardware independently of the source signed bit (flags=%i)', async flags => {
    const hardware = Buffer.alloc(32, 0xAB)
    const { result, fingerprint } = await exercise([game()], undefined, false, {
      userId: fixtureUserId, crypto: webcrypto,
      runningGames: [
        { id: 'broken', pid: 41, executableFingerprint: '!'.repeat(196) },
        { id: 'source', pid: 42, executableFingerprint: sourceFingerprint(hardware, flags) },
      ],
    })
    expect(result.success, result.error).toBe(true)
    expect(result.hasFingerprint, result.fingerprintError).toBe(true)
    expect(result.fingerprintError).toBeNull()
    const payload = xorFingerprintPayload(Buffer.from(fingerprint, 'base64'), fixtureUserId)
    expect(payload.subarray(0x32, 0x52)).toEqual(hardware)
  })

  it('rejects unknown source flags and keeps the hardware fallback', async () => {
    const { result, fingerprint } = await exercise([game()], undefined, false, {
      userId: fixtureUserId, crypto: webcrypto,
      runningGames: [{ id: 'source', pid: 42,
        executableFingerprint: sourceFingerprint(Buffer.alloc(32, 0xAB), 0x04) }],
    })
    expect(result.success, result.error).toBe(true)
    expect(result.hasFingerprint, result.fingerprintError).toBe(true)
    const payload = xorFingerprintPayload(Buffer.from(fingerprint, 'base64'), fixtureUserId)
    expect(payload.subarray(0x32, 0x52)).toEqual(fallbackHardware())
  })
})
