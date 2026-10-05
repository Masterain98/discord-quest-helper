// Targeted recovery for #194. Requires Node 22+ and Discord CDP; no credentials
// or other local-storage entries are read. Inspection is the default.
import { mkdir, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

export function recoveryExpression(apply = false, expected = null) {
  return `(${async function (apply, expected) {
    const frame = document.createElement('iframe')
    frame.style.display = 'none'
    document.body.append(frame)
    try {
      // Discord removes window.localStorage; a same-origin about:blank frame
      // retains the browser accessor, even when webpack initialization failed.
      const storage = frame.contentWindow.localStorage
      const original = storage.getItem('GameStore')
      if (original === null) return { removed: 0, reason: 'No game cache' }
      const parsed = JSON.parse(original)
      const state = parsed._state ?? parsed
      if (!Array.isArray(state.detectableGames)) throw Error('Unknown GameStore format')
      const valid = game => game && typeof game.id === 'string' && typeof game.name === 'string'
      const games = state.detectableGames.filter(valid)
      const removed = state.detectableGames.length - games.length
      const summary = { before: state.detectableGames.length, after: games.length, removed }
      if (!apply) return { ...summary, original }
      if (original !== expected) throw Error('Game cache changed; inspect it again before repairing')
      if (removed === 0) return summary
      const repairedState = { ...state, detectableGames: games }
      const repaired = JSON.stringify(parsed._state ? { ...parsed, _state: repairedState } : repairedState)
      storage.setItem('GameStore', repaired)
      if (storage.getItem('GameStore') !== repaired) throw Error('Could not verify game cache write')
      return { ...summary, repaired: true }
    } finally {
      frame.remove()
    }
  }})(${JSON.stringify(apply)}, ${JSON.stringify(expected)})`
}

async function main() {
  const args = process.argv.slice(2)
  if (args.some(arg => arg !== '--apply' && !/^--port=\d+$/.test(arg))) {
    throw Error('Usage: node scripts/repair-discord-game-cache.mjs [--apply] [--port=9223]')
  }
  const port = Number(args.find(arg => arg.startsWith('--port='))?.slice(7) ?? 9223)
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw Error('Invalid CDP port')
  const response = await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(5000) })
  if (!response.ok) throw Error(`CDP returned HTTP ${response.status}`)
  const targets = (await response.json()).filter(target => {
    try {
      const url = new URL(target.url)
      const ws = new URL(target.webSocketDebuggerUrl)
      return target.type === 'page' && url.protocol === 'https:' &&
        ['discord.com', 'discordapp.com'].includes(url.hostname) &&
        /^\/(app|channels|quest-home)(\/|$)/.test(url.pathname) &&
        ws.protocol === 'ws:' && ws.hostname === '127.0.0.1' && Number(ws.port) === port &&
        !ws.username && !ws.password && !ws.search && !ws.hash && ws.pathname.startsWith('/devtools/page/')
    } catch { return false }
  })
  if (targets.length !== 1) throw Error(`Expected one main Discord renderer; found ${targets.length}`)
  const ws = new WebSocket(targets[0].webSocketDebuggerUrl)
  const pending = new Map()
  let nextId = 0
  ws.addEventListener('message', event => {
    const response = JSON.parse(event.data)
    pending.get(response.id)?.(response)
  })
  const evaluate = expression => new Promise((resolve, reject) => {
    const id = ++nextId
    const timer = setTimeout(() => { pending.delete(id); reject(Error('CDP evaluation timed out')) }, 15000)
    pending.set(id, response => {
      clearTimeout(timer)
      pending.delete(id)
      const error = response.error?.message ?? response.result?.exceptionDetails?.exception?.description
      if (error) reject(Error(error))
      else resolve(response.result.result.value)
    })
    ws.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: {
      expression, returnByValue: true, awaitPromise: true,
    } }))
  })
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(Error('CDP connection timed out')), 5000)
      ws.addEventListener('open', () => { clearTimeout(timer); resolve() }, { once: true })
      ws.addEventListener('error', () => { clearTimeout(timer); reject(Error('CDP connection failed')) }, { once: true })
    })
    const { original, ...summary } = await evaluate(recoveryExpression())
    console.log(JSON.stringify(summary))
    if (!args.includes('--apply') || summary.removed === 0) return
    const directory = resolve('deliverables')
    await mkdir(directory, { recursive: true })
    const backup = resolve(directory, `discord-game-cache-${Date.now()}.json`)
    await writeFile(backup, original, { flag: 'wx' })
    console.log(`Game cache backup: ${backup}`)
    console.log(JSON.stringify(await evaluate(recoveryExpression(true, original))))
    console.log('Repair verified. Restart Discord to finish recovery.')
  } finally { ws.close() }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1 })
}
