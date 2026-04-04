<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { getDebugInfo, getRunnerInfo, captureDiscordHeadersCdp, type DebugInfo, type RunnerInfo, type CdpCapturedHeaders } from '@/api/tauri'
import { useAuthStore } from '@/stores/auth'
import { useI18n } from 'vue-i18n'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog'
import { RefreshCw, Copy, Check, Key, Package, Radio, ChevronRight, Search, X } from 'lucide-vue-next'

const { t } = useI18n()
const authStore = useAuthStore()

const debugInfo = ref<DebugInfo | null>(null)
const runnerInfo = ref<RunnerInfo | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const copied = ref<string | null>(null)
const capturedHeaders = ref<CdpCapturedHeaders | null>(null)
const capturing = ref(false)
const captureError = ref<string | null>(null)
const captureDuration = ref(30)

const CAPTURE_STORAGE_KEY = 'debug_captured_headers'

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
      capturedHeaders.value = JSON.parse(saved)
    }
  } catch (e) {
    console.error('Failed to load cached headers:', e)
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

const availableRequestTypes = computed<RequestType[]>(() => {
  if (!capturedHeaders.value?.requests) return ['All']
  const found = new Set<RequestType>()
  for (const req of capturedHeaders.value.requests) {
    found.add(inferRequestType(req.url, req.headers))
  }
  return REQUEST_TYPES.filter(t => t === 'All' || found.has(t))
})

const filteredRequests = computed(() => {
  if (!capturedHeaders.value?.requests) return []
  return capturedHeaders.value.requests.filter(req => {
    const matchesType = requestTypeFilter.value === 'All' || inferRequestType(req.url, req.headers) === requestTypeFilter.value
    const q = requestSearch.value.trim().toLowerCase()
    const matchesSearch = !q || req.url.toLowerCase().includes(q) ||
      Object.entries(req.headers).some(([k, v]) => k.includes(q) || v.toLowerCase().includes(q))
    return matchesType && matchesSearch
  })
})

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
  error.value = null
  try {
    const [debug, runner] = await Promise.all([getDebugInfo(), getRunnerInfo()])
    debugInfo.value = debug
    runnerInfo.value = runner
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
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
    capturedHeaders.value = await captureDiscordHeadersCdp(undefined, captureDuration.value)
    saveCapturedHeaders()
  } catch (e) {
    captureError.value = String(e)
  } finally {
    capturing.value = false
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

    <div v-if="error" class="p-4 bg-destructive/10 text-destructive rounded-lg">
      {{ error }}
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
              <span class="font-mono ml-1">{{ runnerInfo.size_bytes > 0 ? (runnerInfo.size_bytes / 1024).toFixed(1) + ' KB' : 'N/A' }}</span>
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
              {{ debugInfo.source }}
            </span>
          </div>
        </CardHeader>
        <CardContent class="space-y-3">
          <div class="flex items-center justify-between p-3 bg-muted rounded-lg">
            <div>
              <div class="text-sm font-medium">launch_signature</div>
              <code class="text-xs text-muted-foreground break-all">{{ debugInfo.launch_signature }}</code>
            </div>
            <Button variant="ghost" size="icon" @click="copyToClipboard(debugInfo.launch_signature, 'launch_signature')">
              <Check v-if="copied === 'launch_signature'" class="w-4 h-4 text-green-500" />
              <Copy v-else class="w-4 h-4" />
            </Button>
          </div>
          
          <div class="flex items-center justify-between p-3 bg-muted rounded-lg">
            <div>
              <div class="text-sm font-medium">client_launch_id</div>
              <code class="text-xs text-muted-foreground break-all">{{ debugInfo.client_launch_id }}</code>
            </div>
            <Button variant="ghost" size="icon" @click="copyToClipboard(debugInfo.client_launch_id, 'client_launch_id')">
              <Check v-if="copied === 'client_launch_id'" class="w-4 h-4 text-green-500" />
              <Copy v-else class="w-4 h-4" />
            </Button>
          </div>
          
          <div class="flex items-center justify-between p-3 bg-muted rounded-lg">
            <div>
              <div class="text-sm font-medium">client_heartbeat_session_id</div>
              <code class="text-xs text-muted-foreground break-all">{{ debugInfo.client_heartbeat_session_id }}</code>
            </div>
            <Button variant="ghost" size="icon" @click="copyToClipboard(debugInfo.client_heartbeat_session_id, 'client_heartbeat_session_id')">
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
              <span class="text-muted-foreground">os:</span> {{ debugInfo.super_properties.os }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">browser:</span> {{ debugInfo.super_properties.browser }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">release_channel:</span> {{ debugInfo.super_properties.release_channel }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">client_version:</span> {{ debugInfo.super_properties.client_version }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">os_version:</span> {{ debugInfo.super_properties.os_version }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">os_arch:</span> {{ debugInfo.super_properties.os_arch }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">system_locale:</span> {{ debugInfo.super_properties.system_locale }}
            </div>
            <div class="p-2 bg-muted rounded">
              <span class="text-muted-foreground">browser_version:</span> {{ debugInfo.super_properties.browser_version }}
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">client_build_number:</span> 
              <span class="font-mono font-bold text-primary">{{ debugInfo.super_properties.client_build_number }}</span>
            </div>
            <div class="p-2 bg-muted rounded col-span-2">
              <span class="text-muted-foreground">has_client_mods:</span> 
              <span :class="debugInfo.super_properties.has_client_mods ? 'text-destructive' : 'text-green-500'">
                {{ debugInfo.super_properties.has_client_mods }}
              </span>
            </div>
          </div>
          
          <!-- Base64 -->
          <div class="space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-sm font-medium">Base64 Encoded</span>
              <Button variant="ghost" size="sm" @click="copyToClipboard(debugInfo.x_super_properties_base64, 'base64')">
                <Check v-if="copied === 'base64'" class="w-4 h-4 mr-1 text-green-500" />
                <Copy v-else class="w-4 h-4 mr-1" />
                {{ t('debug.copy') }}
              </Button>
            </div>
            <div class="p-3 bg-muted rounded-lg overflow-x-auto">
              <code class="text-xs break-all">{{ debugInfo.x_super_properties_base64 }}</code>
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
                <span class="font-mono font-bold ml-1">{{ capturedHeaders.total_requests }}</span>
              </div>
              <div class="p-2 bg-muted rounded">
                <span class="text-muted-foreground">{{ t('debug.capture_duration') }}:</span>
                <span class="font-mono ml-1">{{ capturedHeaders.capture_duration_secs }}s</span>
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
                <div class="text-sm font-medium">{{ t('debug.request_list') }} <span class="text-muted-foreground font-normal">({{ filteredRequests.length }}/{{ capturedHeaders.requests.length }})</span></div>
                <Button variant="ghost" size="sm" @click="copyToClipboard(JSON.stringify(capturedHeaders.requests, null, 2), 'requests_json')">
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
