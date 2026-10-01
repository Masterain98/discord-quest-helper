import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

const source = readFileSync('src-tauri/src/cdp_quest.rs', 'utf8')
const script = source.match(/const JS_INIT_QUEST_MODULES: &str = r#"([\s\S]*?)"#;/)[1]
const activityHelpers = source.match(/const JS_ACTIVITY_HELPERS: &str = r#"([\s\S]*?)"#;/)[1]
const sdkDiscovery = source.match(/async function waitForSdk\(\) \{([\s\S]*?)\r?\n    \}\r?\n\r?\n    try \{\r?\n        let sdkState;/)[1]
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
  if (options.apiShape === 'inherited') modules.api = Object.create(modules.api)
  if (options.apiShape === 'accessor') {
    modules.api = Object.defineProperties({}, Object.fromEntries(Object.entries(modules.api)
      .map(([name, method]) => [name, { get: () => method, enumerable: true }])))
  }
  if (options.apiShape === 'throwingAccessor') {
    Object.defineProperty(modules.api, 'put', { get: () => { throw new Error('unavailable') } })
  }
  if (options.apiShape === 'proxy') {
    modules.api = proxyFacade()
  }
  if (options.extraApi) modules.otherApi = { ...modules.api }
  if (options.proxyDecoy) modules.proxy = proxyFacade()
  function proxyFacade() {
    return new Proxy({}, { get: (_, name) => () => {
      if (['get', 'post', 'put', 'patch', 'del'].includes(name)) requests++
      return { locale: 'en' }
    } })
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
  const video = [['api', 'get'], ['api', 'post'], ['QuestsStore', 'getQuest']]
  it.each(['inherited', 'accessor'])('accepts %s HTTP methods without business requests', async apiShape => {
    const { result, requests } = await discover(video, { apiShape, proxyDecoy: true, discoverOnly: true })
    expect(result.success).toBe(true)
    expect(requests).toBe(0)
  })
  it.each(['proxy', 'throwingAccessor'])('rejects a %s facade without sending requests', async apiShape => {
    const { result, requests } = await discover(video, { apiShape, discoverOnly: true })
    expect(result.missing).toEqual(['api.get', 'api.post'])
    expect(requests).toBe(0)
  })
  it('rejects ambiguous concrete facades', async () => {
    const { result, requests } = await discover(video, { extraApi: true, discoverOnly: true })
    expect(result.success).toBe(false)
    expect(requests).toBe(0)
  })
  it('discovers stream and companion game capabilities together before installing state', async () => {
    const { result, window } = await discover([...stream, ...game], { stream: true, game: false, discoverOnly: false })
    expect(result.missing).toEqual(['RunningGameStore.getRunningGames', 'RunningGameStore.getGameForPID'])
    expect(window.__dqh_cdp).toBeUndefined()
  })
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

describe('activity SDK capability discovery', () => {
  async function waitForSdk(availableAfterMs) {
    let elapsed = 0
    let checks = 0
    let calls = 0
    const window = {
      get discordSDK() {
        checks++
        return elapsed >= availableAfterMs
          ? { commands: { questStartTimer: () => { calls++ } } }
          : undefined
      },
    }
    const promise = runInNewContext(
      `${activityHelpers}\nasync function waitForSdk() {${sdkDiscovery}\n}\nwaitForSdk()`,
      { window, Date: { now: () => elapsed }, sleep: async ms => { elapsed += ms } },
    )
    return { promise, elapsed: () => elapsed, checks: () => checks, calls: () => calls }
  }

  it('accepts an SDK loaded after the former 750ms window without calling commands', async () => {
    const result = await waitForSdk(1500)
    expect((await result.promise).waitedMs).toBe(1800)
    expect(result.checks()).toBe(3)
    expect(result.calls()).toBe(0)
  })

  it('reports a missing SDK within three checks and the two-second budget', async () => {
    const result = await waitForSdk(Infinity)
    await expect(result.promise).rejects.toThrow('cdp_capability_missing')
    expect(result.checks()).toBe(3)
    expect(result.elapsed()).toBeLessThanOrEqual(2000)
    expect(result.calls()).toBe(0)
  })
})
