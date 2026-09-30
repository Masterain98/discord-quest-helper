import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

const source = readFileSync('src-tauri/src/cdp_quest.rs', 'utf8')
const script = source.match(/const JS_INIT_QUEST_MODULES: &str = r#"([\s\S]*?)"#;/)[1]
const game = [['RunningGameStore', 'getRunningGames'], ['RunningGameStore', 'getGameForPID'],
  ['QuestsStore', 'getQuest'], ['FluxDispatcher', 'dispatch'], ['FluxDispatcher', 'subscribe'], ['FluxDispatcher', 'unsubscribe']]
const stream = [['ApplicationStreamingStore', 'getStreamerActiveStreamMetadata'], ['QuestsStore', 'getQuest'], ['FluxDispatcher', 'dispatch']]

async function discover(required, options = { stream: false, game: true, discoverOnly: true }) {
  let requests = 0
  class Quests { getQuest() { return null } }
  class Dispatcher { flushWaitQueue() {} dispatch() {} subscribe() {} unsubscribe() {} }
  class Streaming { getStreamerActiveStreamMetadata() { return null } }
  const modules = {
    quests: new Quests(), dispatcher: new Dispatcher(),
    decoy: { get: () => { requests++; throw new Error('wrong facade') }, post: () => {} },
    api: Object.fromEntries(['get', 'post', 'put', 'patch', 'del'].map(name => [name, () => { requests++ }])),
  }
  if (options.game) modules.game = { getRunningGames: () => [], getGameForPID: () => null }
  if (options.stream) modules.streaming = new Streaming()
  const window = options.window ?? {}
  const chunks = []
  chunks.push = () => ({ c: { one: { exports: modules } } })
  const code = script.replace('__DQH_REQUIRED__', JSON.stringify(required))
    .replace('__DQH_DISCOVER_ONLY__', String(options.discoverOnly))
  const result = JSON.parse(await runInNewContext(code, { window, webpackChunkdiscord_app: chunks }))
  return { result, window, requests }
}

describe('on-demand CDP module discovery', () => {
  it('permits games without a streaming module and discovery installs no state', async () => {
    const result = await discover(game)
    expect(result.result.success).toBe(true)
    expect(result.window.__dqh_cdp).toBeUndefined()
    expect(result.requests).toBe(0)
  })
  it('reports the actual missing stream method without sending requests', async () => {
    const { result, requests } = await discover(stream)
    expect(result).toEqual({ success: false, code: 'cdp_capability_missing', missing: ['ApplicationStreamingStore.getStreamerActiveStreamMetadata'] })
    expect(requests).toBe(0)
  })
  it('permits video API discovery without game or streaming modules', async () => {
    const result = await discover([['api', 'get'], ['api', 'post'], ['QuestsStore', 'getQuest']], { stream: false, game: false, discoverOnly: true })
    expect(result.result.success).toBe(true)
    expect(result.requests).toBe(0)
  })
  it('installs state only after successful operation-specific discovery', async () => {
    const result = await discover(game, { stream: false, game: true, discoverOnly: false })
    expect(result.result.success).toBe(true)
    expect(result.window.__dqh_cdp).toBeDefined()
    expect(result.requests).toBe(0)
  })
  it('adding a companion game preserves streaming patches and cleanup originals', async () => {
    const first = await discover(stream, { stream: true, game: false, discoverOnly: false })
    const state = first.window.__dqh_cdp
    const original = state._origGetStreamerActiveStreamMetadata
    state._heartbeatFn = () => {}
    state.ApplicationStreamingStore.getStreamerActiveStreamMetadata = () => ({ spoofed: true })
    const second = await discover(game, { stream: true, game: true, discoverOnly: false, window: first.window })
    expect(second.result.success).toBe(true)
    expect(second.window.__dqh_cdp).toBe(state)
    expect(state._origGetStreamerActiveStreamMetadata).toBe(original)
    expect(state._heartbeatFn).toBeDefined()
    expect(state.RunningGameStore).toBeDefined()
    expect(second.requests).toBe(0)
  })
})
