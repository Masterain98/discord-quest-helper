<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { getDebugInfo, type DebugInfo } from '@/api/tauri'
import { useAuthStore } from '@/stores/auth'
import { useI18n } from 'vue-i18n'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { RefreshCw, Copy, Check, Key } from 'lucide-vue-next'

const { t } = useI18n()
const authStore = useAuthStore()

const debugInfo = ref<DebugInfo | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const copied = ref<string | null>(null)

async function loadDebugInfo() {
  loading.value = true
  error.value = null
  try {
    debugInfo.value = await getDebugInfo()
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

onMounted(() => {
  loadDebugInfo()
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
    </div>

    <div v-else-if="!loading && !error" class="text-center text-muted-foreground py-8">
      {{ t('debug.no_data') }}
    </div>
  </div>
</template>
