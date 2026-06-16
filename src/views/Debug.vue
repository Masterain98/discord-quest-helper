<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import {
  getDebugInfo,
  getRunnerInfo,
  captureDiscordHeadersCdp,
  getQuestDecisionDebug,
  getQuestDecisionsDebug,
  type DebugInfo,
  type RunnerInfo,
  type CdpCapturedHeaders,
  type CapturedRequest,
} from '@/api/tauri'
import { useAuthStore } from '@/stores/auth'
import { useI18n } from 'vue-i18n'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog'
import { RefreshCw, Copy, Check, Key, Package, Radio, ChevronRight, Search, X, Server, Play } from 'lucide-vue-next'

const { t } = useI18n()
const authStore = useAuthStore()

const debugInfo = ref<DebugInfo | null>(null)
const runnerInfo = ref<RunnerInfo | null>(null)
const loading = ref(false)
const loadingStep = ref<string | null>(null)
const lastLoadDurationMs = ref<number | null>(null)
const error = ref<string | null>(null)
const copied = ref<string | null>(null)
const capturedHeaders = ref<CdpCapturedHeaders | null>(null)
const capturing = ref(false)
const captureError = ref<string | null>(null)
const captureDuration = ref(30)
const decisionPlacement = ref(1)
const decisionsPlacement = ref(3)
const decisionsNum = ref(1)
const decisionLoading = ref(false)
const decisionError = ref<string | null>(null)
const decisionResult = ref<Record<string, unknown> | null>(null)
const decisionsResult = ref<Record<string, unknown> | null>(null)

const fallbackText = 'N/A'
const DEBUG_COMMAND_TIMEOUT_MS = 5000
const CAPTURE_STORAGE_KEY = 'debug_captured_headers'

function debugText(value: unknown): string {
  if (value === null || value === undefined || value === '') return fallbackText
  if (typeof value === 'boolean') return value ? 'true' : 'false'
  return String(value)
}

function normalizeHeaders(value: unknown): Record<string, string> {
  if (!value || typeof value !== 'object') return {}
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, headerValue]) => [
      key,
      debugText(headerValue),
    ]),
  )
}

function normalizeCapturedRequest(value: unknown): CapturedRequest | null {
  if (!value || typeof value !== 'object') return null
  const request = value as Record<string, unknown>
  const url = typeof request.url === 'string' ? request.url : ''
  if (!url) return null

  return {
    url,
    method: typeof request.method === 'string' ? request.method : 'GET',
    headers: normalizeHeaders(request.headers),
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function payloadPreview(value: unknown): string {
  if (value === undefined) return 'undefined'
  if (value === null) return 'null'
  if (typeof value !== 'object') return `${typeof value}: ${String(value)}`
  try {
    return JSON.stringify(value).slice(0, 300)
  } catch {
    return Object.prototype.toString.call(value)
  }
}

function saveCapturedHeaders() {
  if (capturedHeaders.value) {
    try {
      sessionStorage.setItem(CAPTURE_STORAGE_KEY, JSON.stringify(capturedHeaders.value))
    } catch (e) {
      console.error('Failed to save captured headers:', e)
      captureError.value = 'Captured headers could not be saved locally. They are still available for this session.'
    }
  }
}

function loadCapturedHeaders() {
  try {
    const saved = sessionStorage.getItem(CAPTURE_STORAGE_KEY)
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<CdpCapturedHeaders> | null
      if (parsed && typeof parsed === 'object' && Array.isArray(parsed.requests)) {
        capturedHeaders.value = {
          total_requests: Number(parsed.total_requests) || parsed.requests.length,
          requests: parsed.requests
            .map((request) => normalizeCapturedRequest(request))
            .filter((request): request is CapturedRequest => request !== null),
          header_key_counts: parsed.header_key_counts || {},
          header_kv_counts: parsed.header_kv_counts || {},
          capture_duration_secs: Number(parsed.capture_duration_secs) || 0,
        }
      } else {
        sessionStorage.removeItem(CAPTURE_STORAGE_KEY)
      }
    }
  } catch (e) {
    console.error('Failed to load cached headers:', e)
    sessionStorage.removeItem(CAPTURE_STORAGE_KEY)
  }
}

function clearCapturedHeaders() {
  capturedHeaders.value = null
  captureError.value = null
  requestSearch.value = ''
  requestTypeFilter.value = 'All'
  sessionStorage.removeItem(CAPTURE_STORAGE_KEY)
}

// Request type filter & search
const requestSearch = ref('')
const requestTypeFilter = ref('All')

const REQUEST_TYPES = ['All', 'Fetch/XHR', 'Doc', 'CSS', 'JS', 'Img', 'Media', 'Manifest', 'Socket', 'Wasm', 'Other'] as const
type RequestType = typeof REQUEST_TYPES[number]

function inferRequestType(url: string, headers: Record<string, string>): RequestType {
  const u = url.toLowerCase().split('?')[0]
  const accept = (headers['accept'] || '').toLowerCase()
  const contentType = (headers['content-type'] || '').toLowerCase()

  if (u.endsWith('.wasm')) return 'Wasm'
  if (u.endsWith('.css')) return 'CSS'
  if (u.match(/\.(js|mjs|ts)$/)) return 'JS'
  if (u.match(/\.(png|jpg|jpeg|gif|webp|svg|ico|avif)$/)) return 'Img'
  if (u.match(/\.(mp4|webm|mp3|ogg|wav|flac|m4a|avi)$/)) return 'Media'
  if (u.endsWith('manifest.json') || u.endsWith('.webmanifest')) return 'Manifest'
  if (u.includes('/gateway') || u.includes('socket') || accept.includes('text/event-stream')) return 'Socket'
  if (accept.includes('text/html') || contentType.includes('text/html')) return 'Doc'
  if (contentType.includes('text/css')) return 'CSS'
  if (contentType.includes('javascript')) return 'JS'
  if (contentType.includes('image/')) return 'Img'
  if (contentType.includes('audio/') || contentType.includes('video/')) return 'Media'
  // Discord API & fetch calls
  if (u.includes('/api/') || accept.includes('application/json') || contentType.includes('application/json')) return 'Fetch/XHR'
  return 'Other'
}

const capturedRequests = computed<CapturedRequest[]>(() => {
  if (!Array.isArray(capturedHeaders.value?.requests)) return []
  return capturedHeaders.value.requests
    .map((request) => normalizeCapturedRequest(request))
    .filter((request): request is CapturedRequest => request !== null)
})

const availableRequestTypes = computed<RequestType[]>(() => {
  if (capturedRequests.value.length === 0) return ['All']
  const found = new Set<RequestType>()
  for (const req of capturedRequests.value) {
    found.add(inferRequestType(req.url, req.headers))
  }
  return REQUEST_TYPES.filter(t => t === 'All' || found.has(t))
})

const filteredRequests = computed(() => {
  if (capturedRequests.value.length === 0) return []
  return capturedRequests.value.filter(req => {
    const matchesType = requestTypeFilter.value === 'All' || inferRequestType(req.url, req.headers) === requestTypeFilter.value
    const q = requestSearch.value.trim().toLowerCase()
    const matchesSearch = !q || req.url.toLowerCase().includes(q) ||
      Object.entries(req.headers).some(([k, v]) => k.toLowerCase().includes(q) || v.toLowerCase().includes(q))
    return matchesType && matchesSearch
  })
})

interface QuestBaselineEndpoint {
  method: string
  path: string
  count: number
  queryKeys: string[]
  category: 'core' | 'decision' | 'surrounding'
}

function requestPath(url: string): string {
  try {
    return new URL(url).pathname
  } catch {
    return url.split('?')[0]
  }
}

function requestQueryKeys(url: string): string[] {
  try {
    return Array.from(new URL(url).searchParams.keys()).filter((key, index, keys) => keys.indexOf(key) === index)
  } catch {
    return []
  }
}

function questBaselineCategory(path: string): QuestBaselineEndpoint['category'] | null {
  if (path === '/api/v9/quests/@me') return 'core'
  if (path === '/api/v9/quests/decision' || path === '/api/v9/quests/get-decisions') return 'decision'
  if (
    path.includes('/billing/subscriptions')
    || path.includes('/virtual-currency/balance')
    || path.includes('/entitlements')
    || path.includes('/program-rewards')
    || path.includes('/library')
    || path.includes('/collectibles-marketing')
    || path.includes('/content-inventory/users/@me')
    || path.includes('/promotions')
  ) {
    return 'surrounding'
  }
  return null
}

const questApiBaseline = computed<QuestBaselineEndpoint[]>(() => {
  if (capturedRequests.value.length === 0) return []
  const map = new Map<string, QuestBaselineEndpoint>()

  for (const req of capturedRequests.value) {
    const path = requestPath(req.url)
    const category = questBaselineCategory(path)
    if (!category) continue

    const key = `${req.method} ${path}`
    const existing = map.get(key)
    const queryKeys = requestQueryKeys(req.url)
    if (existing) {
      existing.count += 1
      existing.queryKeys = Array.from(new Set([...existing.queryKeys, ...queryKeys]))
    } else {
      map.set(key, {
        method: req.method,
        path,
        count: 1,
        queryKeys,
        category,
      })
    }
  }

  return Array.from(map.values()).sort((a, b) => a.path.localeCompare(b.path))
})

function metadataPresence(value: unknown): string {
  if (value == null) return 'absent'
  if (typeof value === 'string') return `present, length ${value.length}`
  return 'present'
}

function summarizeDecisionPayload(payload: Record<string, unknown> | null): Record<string, unknown> | null {
  if (!payload) return null
  const decisions = Array.isArray(payload.decisions) ? payload.decisions : null
  return {
    keys: Object.keys(payload),
    request_id_present: typeof payload.request_id === 'string',
    quest: payload.quest == null ? 'null' : 'present',
    decisions_count: decisions?.length ?? null,
    response_ttl_seconds: payload.response_ttl_seconds ?? null,
    metadata_sealed: metadataPresence(payload.metadata_sealed),
    traffic_metadata_raw: metadataPresence(payload.traffic_metadata_raw),
    traffic_metadata_sealed: metadataPresence(payload.traffic_metadata_sealed),
  }
}

const decisionSummary = computed(() => summarizeDecisionPayload(decisionResult.value))
const decisionsSummary = computed(() => summarizeDecisionPayload(decisionsResult.value))

// x-super-properties decoder dialog
const decoderOpen = ref(false)
const decoderInput = ref('')
const decoderResult = ref<Record<string, unknown> | null>(null)
const decoderError = ref<string | null>(null)

// Expanded header keys in the grouped view
const expandedKeys = ref<Set<string>>(new Set())

// Grouped headers: key -> { count, values: { value -> count }[] }
interface HeaderGroup {
  key: string
  count: number
  values: { value: string; count: number }[]
}

const groupedHeaders = computed<HeaderGroup[]>(() => {
  if (!capturedHeaders.value) return []
  const kvCounts =
    capturedHeaders.value?.header_kv_counts && typeof capturedHeaders.value.header_kv_counts === 'object'
      ? capturedHeaders.value.header_kv_counts
      : {}
  const keyCounts =
    capturedHeaders.value?.header_key_counts && typeof capturedHeaders.value.header_key_counts === 'object'
      ? capturedHeaders.value.header_key_counts
      : {}
  
  // Build grouped map from kv entries: "key: value" -> count
  const groups = new Map<string, Map<string, number>>()
  for (const [kvString, count] of Object.entries(kvCounts)) {
    const colonIdx = kvString.indexOf(': ')
    if (colonIdx === -1) continue
    const key = kvString.substring(0, colonIdx)
    const value = kvString.substring(colonIdx + 2)
    if (!groups.has(key)) groups.set(key, new Map())
    groups.get(key)!.set(value, count)
  }
  
  // Convert to sorted array
  return Object.entries(keyCounts)
    .sort((a, b) => b[1] - a[1])
    .map(([key, count]) => {
      const valuesMap = groups.get(key) || new Map<string, number>()
      const values = Array.from(valuesMap.entries())
        .map(([value, vCount]) => ({ value, count: vCount }))
        .sort((a, b) => b.count - a.count)
      return { key, count, values }
    })
})

function toggleKey(key: string) {
  if (expandedKeys.value.has(key)) {
    expandedKeys.value.delete(key)
  } else {
    expandedKeys.value.add(key)
  }
  // Trigger reactivity
  expandedKeys.value = new Set(expandedKeys.value)
}

function isBase64SuperProps(value: string): boolean {
  // x-super-properties values are long base64 strings
  return value.length > 80 && /^[A-Za-z0-9+/=]+$/.test(value.replace(/\.\.\.[^]*$/, ''))
}

function openDecoder(base64Value?: string) {
  decoderInput.value = base64Value || ''
  decoderResult.value = null
  decoderError.value = null
  if (base64Value) {
    decodeBase64()
  }
  decoderOpen.value = true
}

function decodeBase64() {
  decoderResult.value = null
  decoderError.value = null
  const input = decoderInput.value.trim()
  if (!input) return
  try {
    const decoded = atob(input)
    const parsed = JSON.parse(decoded)
    decoderResult.value = parsed
  } catch (e) {
    decoderError.value = String(e)
  }
}

async function loadDebugInfo() {
  loading.value = true
  loadingStep.value = 'get_debug_info'
  lastLoadDurationMs.value = null
  error.value = null
  const startedAt = performance.now()
  const errors: string[] = []
  try {
    const debug = await withCommandTimeout(getDebugInfo(), 'get_debug_info')
    if (isRecord(debug)) {
      debugInfo.value = debug as DebugInfo
    } else {
      debugInfo.value = {}
      errors.push(`get_debug_info returned an empty or invalid response: ${payloadPreview(debug)}`)
    }
  } catch (e) {
    debugInfo.value = {}
    errors.push(String(e))
  }

  loadingStep.value = 'get_runner_info'
  try {
    runnerInfo.value = await withCommandTimeout(getRunnerInfo(), 'get_runner_info')
  } catch (e) {
    errors.push(String(e))
  } finally {
    lastLoadDurationMs.value = Math.round(performance.now() - startedAt)
    error.value = errors.length > 0 ? errors.join('\n') : null
    loadingStep.value = null
    loading.value = false
  }
}

async function withCommandTimeout<T>(promise: Promise<T>, commandName: string): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = setTimeout(() => {
      reject(new Error(`${commandName} timed out after ${DEBUG_COMMAND_TIMEOUT_MS}ms`))
    }, DEBUG_COMMAND_TIMEOUT_MS)
  })

  try {
    return await Promise.race([promise, timeout])
  } finally {
    if (timeoutId) clearTimeout(timeoutId)
  }
}

async function copyToClipboard(text: string, key: string) {
  try {
    await navigator.clipboard.writeText(text)
    copied.value = key
    setTimeout(() => {
      copied.value = null
    }, 2000)
  } catch (e) {
    console.error('Failed to copy:', e)
  }
}

async function captureHeaders() {
  capturing.value = true
  captureError.value = null
  try {
    const duration = Math.min(
      120,
      Math.max(5, Number.isFinite(captureDuration.value) ? captureDuration.value : 30),
    )
    capturedHeaders.value = await captureDiscordHeadersCdp(undefined, duration)
    saveCapturedHeaders()
  } catch (e) {
    captureError.value = String(e)
  } finally {
    capturing.value = false
  }
}

async function fetchQuestDecisionDebug() {
  decisionLoading.value = true
  decisionError.value = null
  try {
    decisionResult.value = await withCommandTimeout(
      getQuestDecisionDebug(decisionPlacement.value),
      'get_quest_decision_debug'
    ) as Record<string, unknown>
  } catch (e) {
    decisionError.value = String(e)
  } finally {
    decisionLoading.value = false
  }
}

async function fetchQuestDecisionsDebug() {
  decisionLoading.value = true
  decisionError.value = null
  try {
    decisionsResult.value = await withCommandTimeout(
      getQuestDecisionsDebug(decisionsPlacement.value, decisionsNum.value),
      'get_quest_decisions_debug'
    ) as Record<string, unknown>
  } catch (e) {
    decisionError.value = String(e)
  } finally {
    decisionLoading.value = false
  }
}

onMounted(() => {
  loadDebugInfo()
  loadCapturedHeaders()
})
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-2xl font-bold">{{ t('debug.title') }}</h2>
        <p class="text-muted-foreground">{{ t('debug.description') }}</p>
      </div>
      <Button variant="outline" size="sm" @click="loadDebugInfo" :disabled="loading">
        <RefreshCw :class="['w-4 h-4 mr-2', { 'animate-spin': loading }]" />
        {{ t('debug.refresh') }}
      </Button>
    </div>

    <div v-if="loading || lastLoadDurationMs !== null" class="text-xs text-muted-foreground">
      <template v-if="loading">
        {{ t('debug.loading_status', { command: loadingStep || fallbackText }) }}
      </template>
      <template v-else>
        {{ t('debug.last_load_duration', { ms: lastLoadDurationMs }) }}
      </template>
    </div>

    <div v-if="error" class="p-4 bg-destructive/10 text-destructive rounded-lg">
      <div class="font-medium">{{ t('debug.load_failed') }}</div>
      <pre class="mt-2 whitespace-pre-wrap text-xs">{{ error }}</pre>
    </div>

    <div v-if="debugInfo" class="grid gap-4">
      <!-- Runner Info -->
      <Card>
        <CardHeader>
          <div class="flex items-center justify-between">
            <div>
              <CardTitle class="flex items-center gap-2">
                <Package class="w-5 h-5" />
                {{ t('debug.runner_title') }}
              </CardTitle>
              <CardDescription>{{ t('debug.runner_desc') }}</CardDescription>
            </div>
            <span 
              :class="[
                'px-3 py-1 text-xs font-medium rounded-full',
                runnerInfo?.embedded 
                  ? 'bg-green-500/10 text-green-500' 
                  : 'bg-yellow-500/10 text-yellow-500'
              ]"
            >
              {{ runnerInfo?.embedded ? t('debug.runner_ready') : t('debug.runner_not_built') }}
            </span>
          </div>
        </CardHeader>
        <CardContent v-if="runnerInfo">
          <div class="grid grid-cols-2 gap-3 text-sm">
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">{{ t('debug.runner_commit') }}:</span>
              <span class="font-mono ml-1">{{ runnerInfo.commit_hash || 'N/A' }}</span>
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">{{ t('debug.runner_build_time') }}:</span>
              <span class="font-mono ml-1">{{ runnerInfo.build_time || 'N/A' }}</span>
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">{{ t('debug.runner_size') }}:</span>
              <span class="font-mono ml-1">{{ (runnerInfo.size_bytes ?? 0) > 0 ? (runnerInfo.size_bytes / 1024).toFixed(1) + ' KB' : fallbackText }}</span>
            </div>
          </div>
        </CardContent>
      </Card>

      <!-- Token Copy (Developer Only) -->
      <Card v-if="authStore.token">
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            <Key class="w-5 h-5" />
            {{ t('debug.token') }}
          </CardTitle>
          <CardDescription>{{ t('debug.token_desc') }}</CardDescription>
        </CardHeader>
        <CardContent>
          <div class="flex items-center justify-between p-3 bg-muted rounded-lg">
            <div class="flex-1 mr-4">
              <code class="text-xs text-muted-foreground break-all">
                {{ authStore.token.substring(0, 20) }}...{{ authStore.token.substring(authStore.token.length - 10) }}
              </code>
            </div>
            <Button variant="outline" size="sm" @click="copyToClipboard(authStore.token, 'token')">
              <Check v-if="copied === 'token'" class="w-4 h-4 mr-1 text-green-500" />
              <Copy v-else class="w-4 h-4 mr-1" />
              {{ t('debug.copy') }}
            </Button>
          </div>
        </CardContent>
      </Card>

      <!-- Session IDs -->
      <Card>
        <CardHeader>
          <div class="flex items-center justify-between">
            <div>
              <CardTitle>{{ t('debug.session_ids') }}</CardTitle>
              <CardDescription>{{ t('debug.session_ids_desc') }}</CardDescription>
            </div>
            <span 
              :class="[
                'px-3 py-1 text-xs font-medium rounded-full',
                debugInfo.source === 'Default' 
                  ? 'bg-blue-500/10 text-blue-500' 
                  : 'bg-green-500/10 text-green-500'
              ]"
            >
              {{ debugText(debugInfo.source) }}
            </span>
          </div>
        </CardHeader>
        <CardContent class="space-y-3">
          <div class="flex items-center justify-between p-3 bg-muted rounded-lg">
            <div>
              <div class="text-sm font-medium">launch_signature</div>
              <code class="text-xs text-muted-foreground break-all">{{ debugText(debugInfo.launch_signature) }}</code>
            </div>
            <Button variant="ghost" size="icon" @click="copyToClipboard(debugText(debugInfo.launch_signature), 'launch_signature')">
              <Check v-if="copied === 'launch_signature'" class="w-4 h-4 text-green-500" />
              <Copy v-else class="w-4 h-4" />
            </Button>
          </div>
          
          <div class="flex items-center justify-between p-3 bg-muted rounded-lg">
            <div>
              <div class="text-sm font-medium">client_launch_id</div>
              <code class="text-xs text-muted-foreground break-all">{{ debugText(debugInfo.client_launch_id) }}</code>
            </div>
            <Button variant="ghost" size="icon" @click="copyToClipboard(debugText(debugInfo.client_launch_id), 'client_launch_id')">
              <Check v-if="copied === 'client_launch_id'" class="w-4 h-4 text-green-500" />
              <Copy v-else class="w-4 h-4" />
            </Button>
          </div>
          
          <div class="flex items-center justify-between p-3 bg-muted rounded-lg">
            <div>
              <div class="text-sm font-medium">client_heartbeat_session_id</div>
              <code class="text-xs text-muted-foreground break-all">{{ debugText(debugInfo.client_heartbeat_session_id) }}</code>
            </div>
            <Button variant="ghost" size="icon" @click="copyToClipboard(debugText(debugInfo.client_heartbeat_session_id), 'client_heartbeat_session_id')">
              <Check v-if="copied === 'client_heartbeat_session_id'" class="w-4 h-4 text-green-500" />
              <Copy v-else class="w-4 h-4" />
            </Button>
          </div>
        </CardContent>
      </Card>

      <!-- Super Properties -->
      <Card>
        <CardHeader>
          <CardTitle>X-Super-Properties</CardTitle>
          <CardDescription>{{ t('debug.super_properties_desc') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="grid grid-cols-2 gap-3 text-sm">
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">os:</span> {{ debugText(debugInfo.super_properties?.os) }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">browser:</span> {{ debugText(debugInfo.super_properties?.browser) }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">release_channel:</span> {{ debugText(debugInfo.super_properties?.release_channel) }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">client_version:</span> {{ debugText(debugInfo.super_properties?.client_version) }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">os_version:</span> {{ debugText(debugInfo.super_properties?.os_version) }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">os_arch:</span> {{ debugText(debugInfo.super_properties?.os_arch) }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">system_locale:</span> {{ debugText(debugInfo.super_properties?.system_locale) }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">browser_version:</span> {{ debugText(debugInfo.super_properties?.browser_version) }}
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">client_build_number:</span> 
              <span class="font-mono font-bold text-primary">{{ debugText(debugInfo.super_properties?.client_build_number) }}</span>
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">has_client_mods:</span> 
              <span :class="debugInfo.super_properties?.has_client_mods ? 'text-destructive' : 'text-green-500'">
                {{ debugText(debugInfo.super_properties?.has_client_mods) }}
              </span>
            </div>
          </div>
          
          <!-- Base64 -->
          <div class="space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-sm font-medium">Base64 Encoded</span>
              <Button variant="ghost" size="sm" @click="copyToClipboard(debugText(debugInfo.x_super_properties_base64), 'base64')">
                <Check v-if="copied === 'base64'" class="w-4 h-4 mr-1 text-green-500" />
                <Copy v-else class="w-4 h-4 mr-1" />
                {{ t('debug.copy') }}
              </Button>
            </div>
            <div class="p-3 bg-muted rounded-lg overflow-x-auto">
              <code class="text-xs break-all">{{ debugText(debugInfo.x_super_properties_base64) }}</code>
            </div>
          </div>
        </CardContent>
      </Card>

      <!-- Header Profile -->
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            <Server class="w-5 h-5" />
            {{ t('debug.header_profile') }}
          </CardTitle>
          <CardDescription>{{ t('debug.header_profile_desc') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="grid grid-cols-2 gap-3 text-sm">
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">identity_source:</span>
              <span class="font-mono ml-1">{{ debugText(debugInfo.client_identity?.source) }}</span>
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">user_agent:</span>
              <span class="font-mono ml-1 break-all">{{ debugText(debugInfo.client_identity?.user_agent) }}</span>
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">timezone:</span>
              <span class="font-mono ml-1">{{ debugText(debugInfo.header_profile?.timezone) }}</span>
              <span class="text-xs text-muted-foreground ml-1">({{ debugText(debugInfo.header_profile?.timezone_source) }})</span>
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">locale:</span>
              <span class="font-mono ml-1">{{ debugText(debugInfo.header_profile?.locale) }}</span>
              <span class="text-xs text-muted-foreground ml-1">({{ debugText(debugInfo.header_profile?.locale_source) }})</span>
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">accept_language:</span>
              <span class="font-mono ml-1">{{ debugText(debugInfo.header_profile?.accept_language) }}</span>
              <span class="text-xs text-muted-foreground ml-1">({{ debugText(debugInfo.header_profile?.accept_language_source) }})</span>
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">x-installation-id:</span>
              <span class="font-mono ml-1">{{ debugInfo.header_profile ? (debugInfo.header_profile.installation_id_present ? 'present' : 'absent') : fallbackText }}</span>
              <span class="text-xs text-muted-foreground ml-1">({{ debugText(debugInfo.header_profile?.installation_id_source) }})</span>
            </div>
          </div>
        </CardContent>
      </Card>

      <!-- Quest API Baseline -->
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            <Server class="w-5 h-5" />
            {{ t('debug.quest_api_baseline') }}
          </CardTitle>
          <CardDescription>{{ t('debug.quest_api_baseline_desc') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-3">
          <div v-if="questApiBaseline.length > 0" class="space-y-2">
            <div
              v-for="endpoint in questApiBaseline"
              :key="endpoint.method + endpoint.path"
              class="rounded border border-border bg-muted/30 p-3 text-xs"
            >
              <div class="flex flex-wrap items-center gap-2">
                <span class="rounded bg-primary/10 px-1.5 py-0.5 font-mono font-bold text-primary">{{ endpoint.method }}</span>
                <code class="break-all">{{ endpoint.path }}</code>
                <span class="rounded bg-muted px-1.5 py-0.5 text-muted-foreground">{{ endpoint.category }}</span>
                <span class="text-muted-foreground">x{{ endpoint.count }}</span>
              </div>
              <div class="mt-2 text-muted-foreground">
                {{ t('debug.query_keys') }}:
                <span class="font-mono">{{ endpoint.queryKeys.length ? endpoint.queryKeys.join(', ') : 'none' }}</span>
              </div>
            </div>
          </div>
          <div v-else class="text-sm text-muted-foreground">
            {{ t('debug.quest_api_baseline_empty') }}
          </div>
        </CardContent>
      </Card>

      <!-- Quest Placement Decisions -->
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            <Play class="w-5 h-5" />
            {{ t('debug.quest_placement_decisions') }}
          </CardTitle>
          <CardDescription>{{ t('debug.quest_placement_decisions_desc') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div v-if="decisionError" class="p-3 bg-destructive/10 text-destructive rounded-lg text-sm">
            {{ decisionError }}
          </div>
          <div class="grid gap-3 md:grid-cols-2">
            <div class="space-y-2 rounded border border-border p-3">
              <div class="text-sm font-medium">/quests/decision</div>
              <div class="flex items-center gap-2">
                <input v-model.number="decisionPlacement" type="number" min="1" class="h-8 w-20 rounded border border-input bg-background px-2 text-xs" />
                <Button size="sm" variant="outline" :disabled="decisionLoading" @click="fetchQuestDecisionDebug">
                  <RefreshCw v-if="decisionLoading" class="w-4 h-4 mr-2 animate-spin" />
                  {{ t('debug.fetch') }}
                </Button>
              </div>
              <pre v-if="decisionSummary" class="max-h-48 overflow-auto rounded bg-muted p-2 text-xs">{{ JSON.stringify(decisionSummary, null, 2) }}</pre>
            </div>

            <div class="space-y-2 rounded border border-border p-3">
              <div class="text-sm font-medium">/quests/get-decisions</div>
              <div class="flex items-center gap-2">
                <input v-model.number="decisionsPlacement" type="number" min="1" class="h-8 w-20 rounded border border-input bg-background px-2 text-xs" />
                <input v-model.number="decisionsNum" type="number" min="1" max="5" class="h-8 w-20 rounded border border-input bg-background px-2 text-xs" />
                <Button size="sm" variant="outline" :disabled="decisionLoading" @click="fetchQuestDecisionsDebug">
                  <RefreshCw v-if="decisionLoading" class="w-4 h-4 mr-2 animate-spin" />
                  {{ t('debug.fetch') }}
                </Button>
              </div>
              <pre v-if="decisionsSummary" class="max-h-48 overflow-auto rounded bg-muted p-2 text-xs">{{ JSON.stringify(decisionsSummary, null, 2) }}</pre>
            </div>
          </div>
        </CardContent>
      </Card>

      <!-- Captured Discord Headers (Network Dump) -->
      <Card>
        <CardHeader>
          <div class="flex items-center justify-between">
            <div>
              <CardTitle class="flex items-center gap-2">
                <Radio class="w-5 h-5" />
                {{ t('debug.captured_headers') }}
              </CardTitle>
              <CardDescription>{{ t('debug.captured_headers_desc') }}</CardDescription>
            </div>
            <div class="flex items-center gap-2">
              <div class="flex items-center gap-1.5">
                <input
                  v-model.number="captureDuration"
                  type="number"
                  min="5"
                  max="120"
                  :disabled="capturing"
                  class="w-16 h-8 px-2 text-xs text-center rounded border border-input bg-background"
                />
                <span class="text-xs text-muted-foreground">s</span>
              </div>
              <Button variant="outline" size="sm" @click="captureHeaders" :disabled="capturing">
                <RefreshCw v-if="capturing" class="w-4 h-4 mr-2 animate-spin" />
                {{ capturing ? t('debug.capturing') : t('debug.capture_btn') }}
              </Button>
              <Button v-if="capturedHeaders" variant="ghost" size="sm" @click="clearCapturedHeaders" :disabled="capturing">
                {{ t('general.clear') }}
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent class="space-y-4">
          <div v-if="captureError" class="p-3 bg-destructive/10 text-destructive rounded-lg text-sm">
            {{ captureError }}
          </div>

          <div v-if="capturedHeaders" class="space-y-4">
            <!-- Summary -->
            <div class="grid grid-cols-2 gap-3 text-sm">
              <div class="p-2 bg-muted rounded">
                <span class="text-muted-foreground">{{ t('debug.total_requests') }}:</span>
                <span class="font-mono font-bold ml-1">{{ capturedHeaders.total_requests ?? capturedRequests.length }}</span>
              </div>
              <div class="p-2 bg-muted rounded">
                <span class="text-muted-foreground">{{ t('debug.capture_duration') }}:</span>
                <span class="font-mono ml-1">{{ capturedHeaders.capture_duration_secs ?? 0 }}s</span>
              </div>
            </div>

            <!-- Grouped Headers -->
            <div class="space-y-2">
              <div class="flex items-center justify-between">
                <div class="text-sm font-medium">{{ t('debug.header_key_counts') }}</div>
                <Button variant="ghost" size="sm" @click="copyToClipboard(JSON.stringify(capturedHeaders.header_kv_counts, null, 2), 'kv_json')">
                  <Check v-if="copied === 'kv_json'" class="w-4 h-4 mr-1 text-green-500" />
                  <Copy v-else class="w-4 h-4 mr-1" />
                  JSON
                </Button>
              </div>
              <div class="max-h-[32rem] overflow-y-auto rounded border border-border">
                <div v-for="group in groupedHeaders" :key="group.key">
                  <!-- Header key row -->
                  <div
                    class="flex items-center justify-between px-3 py-2 cursor-pointer hover:bg-muted/50 border-b border-border"
                    @click="toggleKey(group.key)"
                  >
                    <div class="flex items-center gap-2 font-mono text-xs">
                      <ChevronRight 
                        class="w-3.5 h-3.5 text-muted-foreground transition-transform" 
                        :class="{ 'rotate-90': expandedKeys.has(group.key) }" 
                      />
                      <span class="font-medium text-primary">{{ group.key }}</span>
                      <span class="text-muted-foreground">({{ group.values.length }} unique)</span>
                    </div>
                    <span class="font-mono text-xs tabular-nums">{{ group.count }}</span>
                  </div>
                  <!-- Expanded values -->
                  <div v-if="expandedKeys.has(group.key)" class="bg-muted/30">
                    <div 
                      v-for="(v, vi) in group.values" 
                      :key="vi"
                      class="flex items-center justify-between px-3 py-1.5 pl-9 text-xs border-b border-border/50 gap-2"
                    >
                      <div class="flex items-center gap-1.5 min-w-0 flex-1">
                        <code class="font-mono text-muted-foreground break-all">{{ v.value.length > 120 ? v.value.substring(0, 120) + '...' : v.value }}</code>
                        <Button 
                          v-if="group.key === 'x-super-properties' && isBase64SuperProps(v.value)" 
                          variant="ghost" 
                          size="icon" 
                          class="h-5 w-5 shrink-0" 
                          @click.stop="openDecoder(v.value)"
                          :title="t('debug.decode')"
                        >
                          <Search class="w-3 h-3" />
                        </Button>
                      </div>
                      <span class="font-mono tabular-nums shrink-0">{{ v.count }}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <!-- Request List -->
            <div class="space-y-2">
              <div class="flex items-center justify-between">
                <div class="text-sm font-medium">{{ t('debug.request_list') }} <span class="text-muted-foreground font-normal">({{ filteredRequests.length }}/{{ capturedRequests.length }})</span></div>
                <Button variant="ghost" size="sm" @click="copyToClipboard(JSON.stringify(capturedRequests, null, 2), 'requests_json')">
                  <Check v-if="copied === 'requests_json'" class="w-4 h-4 mr-1 text-green-500" />
                  <Copy v-else class="w-4 h-4 mr-1" />
                  JSON
                </Button>
              </div>

              <!-- Type filters -->
              <div class="flex flex-wrap gap-1">
                <button
                  v-for="type in availableRequestTypes" :key="type"
                  @click="requestTypeFilter = type"
                  :class="[
                    'px-2 py-0.5 text-[11px] rounded transition-colors',
                    requestTypeFilter === type
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted text-muted-foreground hover:bg-muted/80'
                  ]"
                >{{ type }}</button>
              </div>

              <!-- Search -->
              <div class="relative">
                <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
                <input
                  v-model="requestSearch"
                  class="w-full pl-8 pr-8 py-1.5 bg-muted rounded border border-input text-xs"
                  :placeholder="t('debug.request_search')"
                />
                <button v-if="requestSearch" @click="requestSearch = ''" class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground">
                  <X class="w-3.5 h-3.5" />
                </button>
              </div>

              <div class="max-h-96 overflow-y-auto space-y-2">
                <div v-if="filteredRequests.length === 0" class="text-xs text-muted-foreground text-center py-4">{{ t('debug.request_no_match') }}</div>
                <div v-for="(req, idx) in filteredRequests" :key="idx" class="p-3 bg-muted rounded-lg text-xs space-y-1">
                  <div class="flex items-center gap-2">
                    <span class="px-1.5 py-0.5 rounded text-[10px] font-bold" :class="req.method === 'GET' ? 'bg-blue-500/20 text-blue-500' : req.method === 'POST' ? 'bg-green-500/20 text-green-500' : 'bg-yellow-500/20 text-yellow-500'">
                      {{ req.method }}
                    </span>
                    <span class="px-1.5 py-0.5 rounded text-[10px] bg-muted-foreground/20 text-muted-foreground">{{ inferRequestType(req.url, req.headers) }}</span>
                    <code class="break-all text-muted-foreground flex-1 min-w-0">{{ req.url }}</code>
                    <Button variant="ghost" size="icon" class="h-5 w-5 shrink-0" @click="copyToClipboard(JSON.stringify(req, null, 2), 'req_' + idx)" :title="t('debug.copy')">
                      <Check v-if="copied === 'req_' + idx" class="w-3 h-3 text-green-500" />
                      <Copy v-else class="w-3 h-3" />
                    </Button>
                  </div>
                  <div class="pl-2 border-l-2 border-border mt-1 space-y-0.5">
                    <div v-for="(val, hkey) in req.headers" :key="hkey" class="font-mono">
                      <span class="text-primary">{{ hkey }}</span>: <span class="text-muted-foreground">{{ val.length > 120 ? val.substring(0, 120) + '...' : val }}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div v-else-if="!capturing && !captureError" class="text-sm text-muted-foreground">
            {{ t('debug.capture_hint') }}
          </div>
        </CardContent>
      </Card>
    </div>

    <div v-else-if="!loading && !error" class="text-center text-muted-foreground py-8">
      {{ t('debug.no_data') }}
    </div>

    <!-- x-super-properties Decoder Dialog -->
    <Dialog v-model:open="decoderOpen">
      <DialogContent class="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{{ t('debug.decoder_title') }}</DialogTitle>
          <DialogDescription>{{ t('debug.decoder_desc') }}</DialogDescription>
        </DialogHeader>
        <div class="space-y-3">
          <div class="flex gap-2">
            <input
              v-model="decoderInput"
              class="flex-1 px-3 py-2 bg-muted rounded border border-border font-mono text-xs"
              :placeholder="t('debug.decoder_placeholder')"
              @keydown.enter="decodeBase64"
            />
            <Button size="sm" @click="decodeBase64">{{ t('debug.decoder_decode') }}</Button>
          </div>
          <div v-if="decoderError" class="text-sm text-destructive bg-destructive/10 p-2 rounded">
            {{ decoderError }}
          </div>
          <div v-if="decoderResult" class="space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-sm font-medium">{{ t('debug.decoder_result') }}</span>
              <Button variant="ghost" size="sm" @click="copyToClipboard(JSON.stringify(decoderResult, null, 2), 'decoder_json')">
                <Check v-if="copied === 'decoder_json'" class="w-4 h-4 mr-1 text-green-500" />
                <Copy v-else class="w-4 h-4 mr-1" />
                Copy
              </Button>
            </div>
            <pre class="p-3 bg-muted rounded text-xs font-mono overflow-x-auto whitespace-pre-wrap break-all">{{ JSON.stringify(decoderResult, null, 2) }}</pre>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  </div>
</template>
