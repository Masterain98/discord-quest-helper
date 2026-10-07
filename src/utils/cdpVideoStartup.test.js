import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const source = readFileSync('src-tauri/src/cdp_quest.rs', 'utf8')
const template = source.match(/fn js_start_video_quest[\s\S]*?r#"([\s\S]*?)"#,/)[1]
const code = Object.entries({ quest_id: 'qid', seconds_needed: '100', initial_seconds: '0',
  video_speed: '10', video_interval: '1', video_max_future: '10' })
  .reduce((js, [key, value]) => js.replaceAll(`{${key}}`, value), template)
  .replaceAll('{{', '{').replaceAll('}}', '}')

function serverQuest(enrolledAt) {
  return { id: 'qid', user_status: { enrolled_at: enrolledAt } }
}

function start({ cached = null, body, get } = {}) {
  const api = { get: vi.fn(get ?? (async () => ({ body }))),
    post: vi.fn(async () => ({ body: { completed_at: new Date().toISOString() } })) }
  const dqh = { initialized: true, QuestsStore: { getQuest: vi.fn(() => cached) }, api }
  const navigation = vi.fn(() => { throw new Error('Startup must not navigate') })
  const location = { href: 'https://discord.com/channels/@me', reload: navigation, assign: navigation }
  const window = { __dqh_cdp: dqh, location }
  const pending = runInNewContext(code, { window, Date, setTimeout, clearTimeout,
    history: { pushState: navigation, replaceState: navigation } })
    .then(JSON.parse)
  return { pending, api, dqh, window, navigation }
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers() })

describe('video startup on the current Discord page', () => {
  it('starts from cached enrollment without a request or navigation', async () => {
    const run = start({ cached: { userStatus: { enrolledAt: new Date(Date.now() - 60000).toISOString() } } })
    expect(await run.pending).toEqual({ success: true, started: true })
    await run.dqh._videoPromise
    expect(run.api.get).not.toHaveBeenCalled()
    expect(run.api.post).toHaveBeenCalledOnce()
    expect(run.dqh._videoCompleted).toBe(true)
    expect(run.navigation).not.toHaveBeenCalled()
    expect(run.window.location.href).toBe('https://discord.com/channels/@me')
  })

  it.each(['array', 'object'])('starts a newly enrolled quest from an API %s without a store update', async shape => {
    const quest = serverQuest(new Date(Date.now() - 60000).toISOString())
    const quests = [{ id: 'other', user_status: {} }, quest]
    const run = start({ body: shape === 'array' ? quests : { quests } })
    expect(await run.pending).toEqual({ success: true, started: true })
    await run.dqh._videoPromise
    expect(run.api.get).toHaveBeenCalledExactlyOnceWith({ url: '/quests/@me' })
    expect(run.api.post).toHaveBeenCalledOnce()
    expect(run.dqh._videoCompleted).toBe(true)
    expect(run.navigation).not.toHaveBeenCalled()
  })

  it.each([null, {}, { userStatus: { enrolledAt: 'invalid' } }])('resolves missing or invalid local status (%s)', async cached => {
    const run = start({ cached, body: { quests: [{ id: 'qid', userStatus: {
      enrolledAt: new Date(Date.now() - 60000).toISOString(),
    } }] } })
    expect((await run.pending).success).toBe(true)
    await run.dqh._videoPromise
    expect(run.api.get).toHaveBeenCalledOnce()
    expect(run.api.post).toHaveBeenCalledOnce()
  })

  it.each([
    { body: [], error: 'Quest not found in /quests/@me' },
    { body: [serverQuest(null)], error: 'Quest not enrolled' },
    { body: [serverQuest('invalid')], error: 'Quest enrollment timestamp is invalid' },
    { body: { quests: {} }, error: 'Video quest enrollment response has no quests array' },
    { body: null, error: 'Video quest enrollment response has no quests array' },
  ])('rejects $error before starting the progress loop', async ({ body, error }) => {
    const run = start({ body })
    expect(await run.pending).toEqual({ success: false, error })
    expect(run.api.post).not.toHaveBeenCalled()
    expect(run.dqh._videoPromise).toBeUndefined()
    expect(run.dqh._videoQuestId).toBeUndefined()
    expect(run.navigation).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('reports a failed request without launching progress', async () => {
    const run = start({ get: async () => { throw new Error('Read failed') } })
    expect(await run.pending).toEqual({ success: false,
      error: 'Failed to fetch video quest enrollment: Error: Read failed' })
    expect(run.api.post).not.toHaveBeenCalled()
    expect(run.dqh._videoPromise).toBeUndefined()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('times out after ten seconds and never launches when a late response arrives', async () => {
    let resolve
    const run = start({ get: () => new Promise(r => { resolve = r }) })
    await vi.advanceTimersByTimeAsync(9999)
    expect(run.dqh._videoPromise).toBeUndefined()
    await vi.advanceTimersByTimeAsync(1)
    expect(await run.pending).toEqual({ success: false,
      error: 'Failed to fetch video quest enrollment: Error: Video quest enrollment request timed out after 10000ms' })
    resolve({ body: [serverQuest(new Date().toISOString())] })
    await Promise.resolve()
    expect(run.api.post).not.toHaveBeenCalled()
    expect(run.dqh._videoPromise).toBeUndefined()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('does not revive a cleaned-up run after the enrollment lookup', async () => {
    let resolve
    const run = start({ get: () => new Promise(r => { resolve = r }) })
    delete run.window.__dqh_cdp
    resolve({ body: [serverQuest(new Date().toISOString())] })
    expect(await run.pending).toEqual({ success: false,
      error: 'Video quest startup cancelled during enrollment lookup' })
    expect(run.api.post).not.toHaveBeenCalled()
    expect(run.dqh._videoPromise).toBeUndefined()
  })
})
