import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

const source = readFileSync('src-tauri/src/cdp_quest.rs', 'utf8')
const startTemplate = source.match(/fn js_spoof_play_game_for[\s\S]*?r#"([\s\S]*?)"#/)[1]
const start = Object.entries({
  safe_app_id: '"123"', safe_app_name: '"Example"', host_os_json: '"win32"',
  os_priority_json: '["win32"]', path_templates_json: '{"cmdLine":"{exe}","exePath":"{exe}"}',
  hints_json: '[]', unix_host: 'false',
}).reduce((js, [key, value]) => js.replaceAll(`{${key}}`, value), startTemplate)
  .replaceAll('{{', '{').replaceAll('}}', '}')
const cleanup = source.match(/const JS_CLEANUP_SPOOF: &str = r#"([\s\S]*?)"#;/)[1]

const game = () => ({ id: '123', name: 'Example', executables: [{ name: 'example.exe', os: 'win32' }],
  aliases: ['Example game'], thirdPartySkus: [{ id: '42', distributor: 'steam' }] })

async function exercise(raw, cached, missingStoreOnStop = false) {
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
  const store = { getRunningGames: () => [], getGameForPID: () => null }
  const dqh = { initialized: true, RunningGameStore: store, FluxDispatcher: dispatcher,
    DetectableGameStore: { games: raw }, NativeUtils: { setObservedGamesCallback() {} },
    _origGetRunningGames: store.getRunningGames, _origGetGameForPID: store.getGameForPID,
    api: { get: async () => ({ body: [] }) }, _detectableGamesPayload: cached }
  const chunks = []
  chunks.push = () => ({ c: {} })
  const context = { window: { __dqh_cdp: dqh }, webpackChunkdiscord_app: chunks,
    setTimeout: callback => callback() }
  const result = JSON.parse(await runInNewContext(start, context))
  // Simulate a legacy bridge remembering a poisoned payload before Stop.
  if (cached) dqh._detectableGamesPayload = cached
  if (missingStoreOnStop) delete dqh.DetectableGameStore
  const stopped = JSON.parse(await runInNewContext(cleanup, context))
  return { result, stopped, events, persisted, context }
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
    expect(result.success).toBe(true)
    expect(stopped.success).toBe(true)
    expect(events.filter(event => event.type === 'GAMES_DATABASE_UPDATE')).toEqual([])
    expect(persisted.size).toBe(0)
  })

  it('preserves third-party identity across observer registration and cleanup', async () => {
    const row = game()
    const { result, stopped, events, persisted, context } = await exercise([row])
    expect(result.success).toBe(true)
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
