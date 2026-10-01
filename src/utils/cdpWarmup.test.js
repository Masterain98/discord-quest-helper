import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

const source = readFileSync('src-tauri/src/cdp_quest.rs', 'utf8')
const template = source.match(/fn js_warmup_quest_route[\s\S]*?r#"([\s\S]*?)"#,/)[1]
const code = template
  .replaceAll('{warmup_url}', '"https://discord.com/quest-home"')
  .replaceAll('{restore_url}', '"https://discord.com/channels/@me"')
  .replaceAll('{dwell_ms}', '1500').replaceAll('{restore_settle_ms}', '800')
  .replaceAll('{{', '{').replaceAll('}}', '}')

async function warmup({ historyHandled = false, routeSignal = true, routerHandled = true, aliasWindow = false } = {}) {
  let elapsed = 0
  let url = new URL('https://discord.com/channels/@me')
  const calls = []
  const location = Object.fromEntries(['pathname', 'search', 'hash', 'href'].map(name => [name, undefined]))
  for (const name of Object.keys(location)) Object.defineProperty(location, name, { get: () => url[name] })
  const setUrl = path => { url = new URL(path, url) }
  const router = {
    transitionTo(path) {
      calls.push(['router', path, url.pathname])
      setUrl(path)
      if (routerHandled && routeSignal) router.location = aliasWindow ? location : new URL(url)
    },
    replaceWith(path) { calls.push(['replace', path]); setUrl(path) },
    navigate(path) { calls.push(['navigate', path]); setUrl(path) },
  }
  if (routeSignal) router.location = aliasWindow ? location : new URL(url)
  const history = {
    state: { original: true },
    pushState(_state, _title, path) { calls.push(['history', path]); setUrl(path) },
    replaceState(state, _title, path) { calls.push(['restore-history', path]); setUrl(path); history.state = state },
  }
  const window = { location, dispatchEvent(event) {
    if (event.type === 'popstate' && historyHandled && routeSignal) router.location = new URL(url)
  } }
  const chunks = []
  chunks.push = () => ({ c: { one: { exports: { router } } } })
  class Event { constructor(type) { this.type = type } }
  const result = JSON.parse(await runInNewContext(code, {
    URL, window, history, document: { dispatchEvent() {} },
    Event, PopStateEvent: Event, webpackChunkdiscord_app: chunks,
    Date: { now: () => elapsed }, setTimeout: (callback, ms) => { elapsed += ms; callback() },
  }))
  return { result, calls, finalPath: url.pathname }
}

describe('verified SPA quest warmup', () => {
  it('uses History API only after Discord router state confirms both routes', async () => {
    const { result, calls, finalPath } = await warmup({ historyHandled: true })
    expect(result.success).toBe(true)
    expect(result.warmupMethod).toBe('history.pushState')
    expect(result.restoreMethod).toBe('history.pushState')
    expect(calls.filter(([method]) => method === 'router')).toHaveLength(0)
    expect(finalPath).toBe('/channels/@me')
  })

  it('does not accept a URL-only change and restores the URL before router fallback', async () => {
    const { result, calls } = await warmup()
    expect(result.success).toBe(true)
    expect(result.warmupMethod).toBe('router.transitionTo')
    expect(calls).toContainEqual(['restore-history', '/channels/@me'])
    expect(calls).toContainEqual(['router', '/quest-home', '/channels/@me'])
  })

  it('reports failure so Page.navigate can run when no independent signal exists', async () => {
    const { result, calls } = await warmup({ routeSignal: false })
    expect(result.success).toBe(false)
    expect(result.details).toContain('history.pushState:no-route-signal')
    expect(calls.some(([method]) => method === 'history')).toBe(false)
    expect(calls.map(([method]) => method)).toEqual(['router', 'replace', 'navigate'])
  })

  it('rejects router state aliased to Window.location', async () => {
    const { result } = await warmup({ aliasWindow: true })
    expect(result.success).toBe(false)
    expect(result.details).toContain('history.pushState:no-route-signal')
  })

  it('does not accept router methods that only change the URL', async () => {
    const { result } = await warmup({ routerHandled: false })
    expect(result.success).toBe(false)
    expect(result.details).toContain('history.pushState:no-route-change')
    expect(result.details).toContain('router.transitionTo:no-route-change')
  })
})
