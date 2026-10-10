import { computed, ref, shallowRef } from 'vue'
import { defineStore } from 'pinia'
import * as api from '@/api/discordConsole'
import { formatConsoleException, formatRemoteObject, remoteObjectIds } from '@/utils/discordConsole'

export interface ConsoleEntry {
  id: number
  sessionId: string
  timestamp: number
  kind: 'command' | 'result' | 'console' | 'exception' | 'system'
  level: api.ConsoleLevel
  args: api.RemoteObject[]
  text: string
  errorCode?: string
}

export const useDiscordConsoleStore = defineStore('discordConsole', () => {
  const session = shallowRef<api.ConsoleSession | null>(null)
  const connecting = ref(false)
  const executing = ref(false)
  const opened = ref(false)
  const input = ref('')
  const entries = ref<ConsoleEntry[]>([])
  const paused = ref(false)
  const pendingEntries = ref<ConsoleEntry[]>([])
  const pendingCount = computed(() => pendingEntries.value.length)
  const history = ref<string[]>([])
  const historyIndex = ref(-1)
  const search = ref('')
  const level = ref<api.ConsoleLevel | 'all'>('all')
  const lastError = ref<string | null>(null)
  let revision = 0
  let nextEntry = 0
  let lastSequence = 0
  let historyDraft = ''
  let connectPromise: Promise<void> | null = null
  // Expanded properties are owned by their root output entry, including nested handles.
  const entryObjects = new Map<number, Set<string>>()
  const properties = shallowRef(new Map<string, api.ConsoleProperties>())
  const propertyRequests = new Map<string, Promise<api.ConsoleProperties>>()

  const visibleEntries = computed(() => entries.value.filter(entry =>
    (level.value === 'all' || entry.kind === 'command' || entry.level === level.value)
    && entry.text.toLocaleLowerCase().includes(search.value.toLocaleLowerCase())))
  const connected = computed(() => session.value !== null)

  function currentOwners() {
    return [...entries.value, ...pendingEntries.value].filter(entry => entry.sessionId === session.value?.sessionId)
      .map(entry => entryObjects.get(entry.id)).filter((ids): ids is Set<string> => !!ids)
  }
  function inheritCachedObjects(ids: Set<string>) {
    // Iterating a Set also visits newly added descendants, with cycles deduplicated.
    for (const id of ids) remoteObjectIds(properties.value.get(id)).forEach(child => ids.add(child))
  }

  function releaseEntries(removed: ConsoleEntry[]) {
    const current = session.value?.sessionId
    const released = new Set<string>()
    for (const entry of removed) {
      const ids = entryObjects.get(entry.id)
      if (entry.sessionId === current) ids?.forEach(id => released.add(id))
      entryObjects.delete(entry.id)
    }
    // A handle may appear in several records; release only when no retained record owns it.
    for (const ids of currentOwners()) ids.forEach(id => released.delete(id))
    if (current && released.size) {
      released.forEach(id => properties.value.delete(id))
      void api.releaseDiscordConsoleObjects(current, [...released]).catch(() => undefined)
    }
  }

  function append(value: Omit<ConsoleEntry, 'id'>) {
    const entry = { ...value, id: ++nextEntry }
    const output = paused.value ? pendingEntries.value : entries.value
    output.push(entry)
    const ids = new Set(remoteObjectIds(entry.args))
    inheritCachedObjects(ids)
    entryObjects.set(entry.id, ids)
    if (output.length > 2000) releaseEntries(output.splice(0, output.length - 2000))
    return entry.id
  }
  function system(errorCode: string, sessionId = session.value?.sessionId ?? '') {
    lastError.value = errorCode
    append({ sessionId, timestamp: Date.now(), kind: 'system', level: 'warn', args: [], text: errorCode, errorCode })
  }
  function receive(event: api.ConsoleEvent) {
    if (event.sessionId !== session.value?.sessionId || event.sequence <= lastSequence) return
    lastSequence = event.sequence
    if (event.kind === 'disconnected') {
      system(event.message ?? 'console_session_invalidated', event.sessionId)
      revision += 1
      session.value = null
      executing.value = false
      properties.value = new Map()
      propertyRequests.clear()
      return
    }
    const args = event.exception?.exception ? [event.exception.exception] : event.args
    append({ sessionId: event.sessionId, timestamp: event.timestamp, kind: event.kind,
      level: event.level, args, text: event.exception ? formatConsoleException(event.exception) : args.map(formatRemoteObject).join(' ') })
  }

  async function connect(port: number) {
    opened.value = true
    if (session.value?.port === port) return
    if (connectPromise) return connectPromise
    const generation = ++revision
    connecting.value = true
    const buffered: api.ConsoleEvent[] = []
    let accepted = false
    const work = (async () => {
      try {
        const next = await api.openDiscordConsole(port, event => {
          if (generation !== revision) return
          if (accepted) receive(event)
          else buffered.push(event)
        })
        if (generation !== revision) { await api.closeDiscordConsole(next.sessionId); return }
        session.value = next
        lastSequence = 0
        lastError.value = null
        accepted = true
        buffered.forEach(receive)
      } catch (error) {
        if (generation === revision) system(errorCode(error))
      } finally {
        if (generation === revision) connecting.value = false
      }
    })()
    connectPromise = work
    try { await work } finally { if (connectPromise === work) connectPromise = null }
  }

  function errorCode(error: unknown): string {
    return typeof error === 'string' && error.startsWith('console_') ? error : 'console_protocol_error'
  }

  async function disconnect(clear = false) {
    ++revision
    connecting.value = false
    executing.value = false
    const previous = session.value
    session.value = null
    properties.value = new Map()
    propertyRequests.clear()
    if (clear) {
      entries.value = []
      pendingEntries.value = []
      paused.value = false
      entryObjects.clear()
      history.value = []
      historyIndex.value = -1
      historyDraft = ''
      input.value = ''
      search.value = ''
      level.value = 'all'
      lastError.value = null
      opened.value = false
    }
    if (previous) await api.closeDiscordConsole(previous.sessionId).catch(() => undefined)
  }

  async function execute() {
    const current = session.value
    const expression = input.value
    if (!current || executing.value || !expression.trim()) return
    const generation = revision
    executing.value = true
    lastError.value = null
    history.value.push(expression)
    if (history.value.length > 100) history.value.splice(0, history.value.length - 100)
    historyIndex.value = -1
    historyDraft = ''
    input.value = ''
    append({ sessionId: current.sessionId, timestamp: Date.now(), kind: 'command', level: 'log', args: [], text: expression })
    try {
      const result = await api.evaluateDiscordConsole(current.sessionId, expression)
      if (generation !== revision) return
      const exception = result.exceptionDetails
      const object = exception?.exception ?? result.result
      append({ sessionId: current.sessionId, timestamp: Date.now(), kind: exception ? 'exception' : 'result',
        level: exception ? 'error' : 'log', args: object ? [object] : [],
        text: exception ? formatConsoleException(exception) : formatRemoteObject(result.result) })
    } catch (error) {
      if (generation === revision) system(errorCode(error), current.sessionId)
    } finally { if (generation === revision) executing.value = false }
  }

  function recall(direction: -1 | 1) {
    if (!history.value.length) return
    if (historyIndex.value < 0) {
      if (direction === 1) return
      historyDraft = input.value
      historyIndex.value = history.value.length
    }
    const next = historyIndex.value + direction
    if (next < 0) return
    if (next >= history.value.length) { historyIndex.value = -1; input.value = historyDraft; return }
    historyIndex.value = next
    input.value = history.value[next]
  }

  async function expand(entry: ConsoleEntry, objectId: string): Promise<api.ConsoleProperties> {
    const current = session.value
    if (!current || current.sessionId !== entry.sessionId || !entryObjects.has(entry.id)) throw new Error('console_object_expired')
    const cached = properties.value.get(objectId)
    if (cached) {
      inheritCachedObjects(entryObjects.get(entry.id)!)
      return cached
    }
    const key = `${current.sessionId}:${objectId}`
    let request = propertyRequests.get(key)
    if (!request) {
      request = api.getDiscordConsoleProperties(current.sessionId, objectId).then(result => {
        if (session.value?.sessionId !== current.sessionId) throw new Error('console_object_expired')
        const owners = currentOwners().filter(ids => ids.has(objectId))
        if (!owners.length) {
          const retained = new Set(currentOwners().flatMap(ids => [...ids]))
          const unowned = remoteObjectIds(result).filter(id => !retained.has(id))
          void api.releaseDiscordConsoleObjects(current.sessionId, unowned).catch(() => undefined)
          throw new Error('console_object_expired')
        }
        properties.value.set(objectId, result)
        owners.forEach(inheritCachedObjects)
        return result
      })
      propertyRequests.set(key, request)
    }
    try {
      const result = await request
      if (session.value?.sessionId !== current.sessionId) throw new Error('console_object_expired')
      if (!entryObjects.has(entry.id)) {
        throw new Error('console_object_expired')
      }
      return result
    } finally { if (propertyRequests.get(key) === request) propertyRequests.delete(key) }
  }

  function togglePaused() {
    paused.value = !paused.value
    if (!paused.value) {
      entries.value.push(...pendingEntries.value.splice(0))
      if (entries.value.length > 2000) releaseEntries(entries.value.splice(0, entries.value.length - 2000))
    }
  }
  function clearOutput() {
    releaseEntries([...entries.value.splice(0), ...pendingEntries.value.splice(0)])
    properties.value = new Map()
  }
  return { session, connecting, executing, connected, opened, input, entries, paused, pendingCount, history, historyIndex,
    search, level, lastError, visibleEntries, connect, disconnect, execute, recall, expand, clearOutput, togglePaused }
})
