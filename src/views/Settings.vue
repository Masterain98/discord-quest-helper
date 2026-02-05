<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Eye, EyeOff, Loader2, CheckCircle2, Copy, Check, AlertTriangle, Download, Link2, Wifi, WifiOff, RotateCw } from 'lucide-vue-next'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogAction,
  AlertDialogCancel,
} from '@/components/ui/alert-dialog'
import { useI18n } from 'vue-i18n'
import { checkCdpStatus, fetchSuperPropertiesCdp, createDiscordDebugShortcut, getSuperPropertiesMode, retrySuperProperties, type CdpStatus, type SuperPropertiesModeInfo } from '@/api/tauri'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const versionStore = useVersionStore()
const manualToken = ref('')
const showToken = ref(false)
const copied = ref(false)

const exporting = ref(false)

// CDP state
const cdpStatus = ref<CdpStatus | null>(null)
const cdpChecking = ref(false)
const cdpFetching = ref(false)
const cdpFetchSuccess = ref(false)
const cdpFetchError = ref('')
const shortcutCreating = ref(false)
const shortcutSuccess = ref(false)
const shortcutError = ref('')

// SuperProperties Mode state
const superPropsMode = ref<SuperPropertiesModeInfo | null>(null)
const retryingMode = ref(false)

// Debug mode unlock (tap version 7 times like Android developer options)
const debugModeEnabled = ref(false)
const versionTapCount = ref(0)
const lastTapTime = ref(0)
const showDebugUnlockHint = ref(false)

function handleVersionTap() {
  const now = Date.now()
  // Reset counter if more than 2 seconds since last tap
  if (now - lastTapTime.value > 2000) {
    versionTapCount.value = 0
  }
  lastTapTime.value = now
  versionTapCount.value++
  
  // Show hint when getting close
  if (versionTapCount.value >= 4 && versionTapCount.value < 7) {
    showDebugUnlockHint.value = true
  }
  
  if (versionTapCount.value >= 7) {
    debugModeEnabled.value = true
    localStorage.setItem('debugMode', 'true')
    versionTapCount.value = 0
    showDebugUnlockHint.value = false
    // Emit event to notify App.vue
    emit('debug-unlocked')
  }
}

const emit = defineEmits<{
  'navigate-to-home': []
  'debug-unlocked': []
}>()

async function loadSuperPropsMode() {
  try {
    superPropsMode.value = await getSuperPropertiesMode()
  } catch (e) {
    console.error('Failed to get SuperProperties mode:', e)
  }
}

async function retrySuperProps() {
  retryingMode.value = true
  try {
    await retrySuperProperties(questsStore.cdpPort)
    await loadSuperPropsMode()
    await checkCdp()
  } catch (e) {
    console.error('Retry failed:', e)
  } finally {
    retryingMode.value = false
  }
}

async function checkCdp() {
  cdpChecking.value = true
  try {
    cdpStatus.value = await checkCdpStatus(questsStore.cdpPort)
    // Also refresh SuperProperties mode after checking CDP status (fix race condition)
    await loadSuperPropsMode()
  } catch (e) {
    console.error('CDP check failed:', e)
    cdpStatus.value = { available: false, connected: false, target_title: null, error: String(e) }
  } finally {
    cdpChecking.value = false
  }
}

async function fetchCdpSuperProperties() {
  cdpFetching.value = true
  cdpFetchSuccess.value = false
  cdpFetchError.value = ''
  try {
    const result = await fetchSuperPropertiesCdp(questsStore.cdpPort)
    console.log('CDP SuperProperties fetched:', result)
    cdpFetchSuccess.value = true
    setTimeout(() => { cdpFetchSuccess.value = false }, 5000)
    await checkCdp() // Refresh status
    await loadSuperPropsMode() // Refresh mode display
  } catch (e) {
    console.error('CDP fetch failed:', e)
    cdpFetchError.value = String(e)
    setTimeout(() => { cdpFetchError.value = '' }, 5000)
  } finally {
    cdpFetching.value = false
  }
}

async function createShortcut() {
  shortcutCreating.value = true
  shortcutSuccess.value = false
  shortcutError.value = ''
  try {
    await createDiscordDebugShortcut(questsStore.cdpPort)
    shortcutSuccess.value = true
    setTimeout(() => { shortcutSuccess.value = false }, 3000)
  } catch (e) {
    shortcutError.value = String(e)
    setTimeout(() => { shortcutError.value = '' }, 5000)
  } finally {
    shortcutCreating.value = false
  }
}

// Emit for tab navigation (consolidated above)

// Heartbeat Safety Warning Logic
const showHeartbeatWarning = ref(false)

function handleHeartbeatClick() {
  if (questsStore.gameQuestMode === 'heartbeat') return // Already selected
  showHeartbeatWarning.value = true
}

function confirmHeartbeatMode() {
  questsStore.gameQuestMode = 'heartbeat'
  showHeartbeatWarning.value = false
}

async function copyPath() {
  if (cachePath.value) {
    await navigator.clipboard.writeText(cachePath.value)
    copied.value = true
    setTimeout(() => { copied.value = false }, 2000)
  }
}



async function handleAutoDetect() {
  await authStore.tryAutoDetect()
  // If multiple accounts were detected, navigate to Home
  if (authStore.detectedAccounts.length > 0) {
    emit('navigate-to-home')
  }
}

async function handleManualLogin() {
  if (manualToken.value) {
    await authStore.loginWithToken(manualToken.value)
  }
}

// Cache path
const cachePath = ref('')

// External link handling
import { open } from '@tauri-apps/plugin-shell'
import { invoke } from '@tauri-apps/api/core'
import { documentDir } from '@tauri-apps/api/path'
import { mkdir } from '@tauri-apps/plugin-fs'
import { FolderOpen } from 'lucide-vue-next'

async function openExternal(url: string) {
  try {
    await open(url)
  } catch (error) {
    console.error('Failed to open URL:', error)
  }
}

async function openCacheDir() {
  const path = cachePath.value
  console.log('Attempting to open cache directory:', path)
  
  if (!path) {
    console.error('Cache path is empty!')
    return
  }
  
  try {
    // Try to create it recursively (will not fail if exists)
    await mkdir(path, { recursive: true })
    console.log('Directory created/verified:', path)
    
    // Use custom Rust command to ensure explorer opens
    await invoke('open_in_explorer', { path })
  } catch (e) {
    console.error('Failed to open cache dir:', e)
    // Fallback to documents directory
    try {
        const docDir = await documentDir()
        await invoke('open_in_explorer', { path: docDir })
    } catch (e2) {
        console.error('Fallback failed:', e2)
    }
  }
}


onMounted(async () => {
  const docDir = await documentDir()
  // Remove trailing backslash if present, then append subdirectory
  const normalizedDocDir = docDir.replace(/[\\/]+$/, '')
  cachePath.value = `${normalizedDocDir}\\DiscordQuestGames`
  
  // Check CDP status (will also load SuperProperties mode after check completes)
  checkCdp()
})

// Log export functionality
import { save } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'

const exportSuccess = ref(false)
const exportError = ref(false)

async function exportLogs() {
  exporting.value = true
  exportSuccess.value = false
  exportError.value = false
  try {
    const logs = await invoke<string>('export_logs')
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
    const path = await save({
      filters: [{ name: 'JSON', extensions: ['json'] }],
      defaultPath: `dqh-logs-${timestamp}.json`
    })
    // If the user cancels the dialog, simply return without error
    if (!path) {
      return
    }
    await writeTextFile(path, logs)
    // Show success feedback
    exportSuccess.value = true
    setTimeout(() => { exportSuccess.value = false }, 3000)
  } catch (error) {
    console.error('Failed to export logs:', error)
    // Show error feedback
    exportError.value = true
    setTimeout(() => { exportError.value = false }, 5000)
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <div class="settings-view fade-in space-y-6 select-none">
    <h2 class="text-2xl font-bold tracking-tight">{{ t('settings.title') }}</h2>
    
    <div class="grid gap-6">
      <!-- Authentication -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.discord_token') }}</CardTitle>
          <CardDescription>
            {{ t('settings.discord_token_desc') }}
          </CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <div v-if="authStore.user" class="space-y-3">
            <div class="p-3 bg-green-500/10 border border-green-500/20 rounded-md text-green-500 flex items-center gap-2">
              <span class="h-2 w-2 rounded-full bg-green-500"></span>
              {{ t('auth.authenticated_as') }} <span class="font-bold">{{ authStore.user.username }}</span>
            </div>
          </div>
          
          <div v-else class="space-y-4">
             <Button 
               @click="handleAutoDetect" 
               :disabled="authStore.loading"
               variant="secondary"
               class="w-full sm:w-auto"
             >
               <Loader2 v-if="authStore.loading" class="w-4 h-4 mr-2 animate-spin" />
               Auto Detect Token
             </Button>

             <div class="relative">
               <div class="absolute inset-0 flex items-center">
                 <span class="w-full border-t" />
               </div>
               <div class="relative flex justify-center text-xs uppercase">
                 <span class="bg-card px-2 text-muted-foreground">Or manually</span>
               </div>
             </div>
             
             <div class="space-y-2">
               <Label for="token">Token</Label>
               <div class="flex gap-2">
                 <div class="relative flex-1">
                   <Input 
                     id="token"
                     v-model="manualToken"
                     :type="showToken ? 'text' : 'password'"
                     placeholder="Enter your token"
                   />
                   <Button
                     type="button"
                     variant="ghost"
                     size="icon"
                     class="absolute right-0 top-0 h-full px-3 text-muted-foreground hover:text-foreground"
                     @click="showToken = !showToken"
                   >
                     <Eye v-if="!showToken" class="w-4 h-4" />
                     <EyeOff v-else class="w-4 h-4" />
                   </Button>
                 </div>
                 <Button @click="handleManualLogin" :disabled="authStore.loading || !manualToken">
                   Login
                 </Button>
               </div>
               <p v-if="authStore.error" class="text-xs text-destructive">
                 {{ authStore.error }}
               </p>
             </div>
          </div>
        </CardContent>
      </Card>
      
      <!-- Video Quest Configuration -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.video_quest_config') }}</CardTitle>
          <CardDescription>{{ t('settings.video_quest_config_desc') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-8">
           <div class="space-y-4">
             <div class="flex justify-between items-center">
               <Label>{{ t('settings.completion_speed') }} ({{ questsStore.speedMultiplier }}x)</Label>
               <span class="text-xs text-muted-foreground">{{ t('settings.speed_hint') }}</span>
             </div>
             <input 
               type="range"
               v-model.number="questsStore.speedMultiplier"
               min="1"
               max="10"
               step="1"
               class="w-full h-2 bg-secondary rounded-lg appearance-none cursor-pointer accent-primary"
             />
             <div class="flex justify-between text-xs text-muted-foreground">
               <span>1x ({{ t('settings.speed_normal') }})</span>
               <span>10x ({{ t('settings.speed_fast') }})</span>
             </div>
           </div>

           <div class="space-y-4">
             <div class="flex justify-between items-center">
               <Label>{{ t('settings.request_interval') }} ({{ questsStore.heartbeatInterval }}s)</Label>
               <span class="text-xs text-muted-foreground">{{ t('settings.interval_hint') }}</span>
             </div>
             <input 
               type="range"
               v-model.number="questsStore.heartbeatInterval"
               min="1"
               max="30"
               step="1"
               class="w-full h-2 bg-secondary rounded-lg appearance-none cursor-pointer accent-primary"
             />
              <div class="flex justify-between text-xs text-muted-foreground">
                <span>1s ({{ t('settings.interval_fast') }})</span>
                <span>30s ({{ t('settings.interval_slow') }})</span>
              </div>
              <p v-if="questsStore.heartbeatInterval < 3" class="text-xs text-yellow-600 dark:text-yellow-500 mt-2">
                ⚠️ {{ t('settings.rate_limit_warning') }}
              </p>
            </div>
        </CardContent>
      </Card>
      
      <!-- Game Polling Configuration -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.game_polling_title') }}</CardTitle>
          <CardDescription>{{ t('settings.game_polling_desc') }}</CardDescription>
        </CardHeader>
        <CardContent>
          <div class="space-y-4">
            <div class="flex justify-between items-center">
              <Label>{{ t('settings.game_polling_interval') }} ({{ questsStore.gamePollingInterval }}s)</Label>
              <span class="text-xs text-muted-foreground">{{ t('settings.game_polling_hint') }}</span>
            </div>
            <input 
              type="range"
              v-model.number="questsStore.gamePollingInterval"
              min="5"
              max="180"
              step="1"
              class="w-full h-2 bg-secondary rounded-lg appearance-none cursor-pointer accent-primary"
            />
            <div class="flex justify-between text-xs text-muted-foreground">
              <span>5s ({{ t('settings.interval_fast') }})</span>
              <span>180s ({{ t('settings.interval_slow') }})</span>
            </div>
            <p v-if="questsStore.gamePollingInterval < 30" class="text-xs text-yellow-600 dark:text-yellow-500 mt-2">
              ⚠️ {{ t('settings.rate_limit_warning') }}
            </p>
          </div>
        </CardContent>
      </Card>
      
      <!-- Game Quest Mode -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.game_quest_mode') }}</CardTitle>
          <CardDescription>{{ t('settings.game_quest_mode_desc') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-6">
          <!-- Mode Selection -->
          <div class="space-y-3">
            <Label>{{ t('settings.game_quest_mode') }}</Label>
            <div class="grid grid-cols-2 gap-3">
              <button 
                @click="questsStore.gameQuestMode = 'simulate'"
                :class="[
                  'p-4 rounded-lg border-2 text-left transition-all',
                  questsStore.gameQuestMode === 'simulate' 
                    ? 'border-primary bg-primary/5' 
                    : 'border-border hover:border-primary/50'
                ]"
              >
                <div class="font-medium">{{ t('settings.game_mode_simulate') }}</div>
                <div class="text-xs text-muted-foreground mt-1">{{ t('settings.game_mode_simulate_desc') }}</div>
              </button>
              <button 
                @click="handleHeartbeatClick"
                :class="[
                  'p-4 rounded-lg border-2 text-left transition-all',
                  questsStore.gameQuestMode === 'heartbeat' 
                    ? 'border-destructive bg-destructive/5' 
                    : 'border-border hover:border-destructive/50'
                ]"
              >
                <div class="font-medium flex items-center gap-2">
                  {{ t('settings.game_mode_heartbeat') }}
                  <AlertTriangle v-if="questsStore.gameQuestMode === 'heartbeat'" class="w-4 h-4 text-destructive" />
                </div>
                <div class="text-xs text-muted-foreground mt-1">{{ t('settings.game_mode_heartbeat_desc') }}</div>
              </button>
            </div>
          </div>
        </CardContent>
      </Card>

      <!-- Safety Warning Dialog -->
      <AlertDialog :open="showHeartbeatWarning" @update:open="showHeartbeatWarning = $event">
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle class="flex items-center gap-2 text-destructive">
              <AlertTriangle class="w-5 h-5" />
              {{ t('settings.game_mode_warning_title') }}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {{ t('settings.game_mode_warning_desc') }}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel @click="showHeartbeatWarning = false">Cancel</AlertDialogCancel>
            <AlertDialogAction 
              class="bg-destructive hover:bg-destructive/90 text-destructive-foreground"
              @click="confirmHeartbeatMode"
            >
              I understand, enable unsafe mode
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      
      <!-- Discord CDP Integration -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.cdp_title') }}</CardTitle>
          <CardDescription>{{ t('settings.cdp_desc') }}</CardDescription>
        </CardHeader>
        <CardContent class="space-y-4">
          <!-- CDP Status -->
          <div class="flex items-center justify-between p-3 rounded-lg border" 
               :class="cdpStatus?.connected ? 'bg-green-500/10 border-green-500/30' : 'bg-muted/50 border-border'">
            <div class="flex items-center gap-2">
              <Wifi v-if="cdpStatus?.connected" class="w-4 h-4 text-green-500" />
              <WifiOff v-else class="w-4 h-4 text-muted-foreground" />
              <span class="text-sm">
                <template v-if="cdpChecking">{{ t('settings.cdp_checking') }}</template>
                <template v-else-if="cdpStatus?.connected">
                  {{ t('settings.cdp_connected') }}
                  <span v-if="cdpStatus.target_title" class="text-muted-foreground ml-1">({{ cdpStatus.target_title }})</span>
                </template>
                <template v-else>{{ t('settings.cdp_disconnected') }}</template>
              </span>
            </div>
            <Button variant="ghost" size="sm" @click="checkCdp" :disabled="cdpChecking">
              <Loader2 v-if="cdpChecking" class="w-4 h-4 animate-spin" />
              <template v-else>{{ t('general.refresh') }}</template>
            </Button>
          </div>

          <!-- CDP Port Setting -->
          <div class="space-y-2">
            <Label>{{ t('settings.cdp_port') }}</Label>
            <div class="flex gap-2 items-center">
              <Input 
                type="number" 
                v-model.number="questsStore.cdpPort" 
                min="1024" 
                max="65535" 
                class="w-32"
              />
              <span class="text-xs text-muted-foreground">{{ t('settings.cdp_port_hint') }}</span>
            </div>
          </div>

          <!-- Fetch SuperProperties Button -->
          <div v-if="cdpStatus?.connected" class="space-y-2">
            <div class="flex items-center gap-3">
              <Button variant="secondary" @click="fetchCdpSuperProperties" :disabled="cdpFetching">
                <Loader2 v-if="cdpFetching" class="w-4 h-4 mr-2 animate-spin" />
                <Link2 v-else class="w-4 h-4 mr-2" />
                {{ t('settings.cdp_fetch') }}
              </Button>
              <span v-if="cdpFetchSuccess" class="text-sm text-green-500 flex items-center gap-1">
                <Check class="w-4 h-4" /> {{ t('settings.cdp_fetch_success') }}
              </span>
              <span v-if="cdpFetchError" class="text-sm text-red-500">{{ cdpFetchError }}</span>
            </div>
          </div>

          <!-- Current SuperProperties Mode Display -->
          <div class="p-3 rounded-lg border bg-muted/30 space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-sm font-medium">{{ t('settings.super_props_mode') }}</span>
              <div class="flex items-center gap-2">
                <Badge 
                  :variant="superPropsMode?.mode === 'cdp' ? 'default' : (superPropsMode?.mode === 'remote_js' ? 'secondary' : 'outline')"
                  :class="[
                    superPropsMode?.mode === 'cdp' && 'bg-green-500 text-white',
                    superPropsMode?.mode === 'remote_js' && 'bg-yellow-500 text-black',
                    superPropsMode?.mode === 'default' && 'bg-red-500/20 text-red-500 border-red-500/50'
                  ]"
                >
                  {{ superPropsMode?.mode === 'cdp' ? 'CDP' : (superPropsMode?.mode === 'remote_js' ? t('settings.remote_js') : t('settings.default_mode')) }}
                </Badge>
                <Button variant="ghost" size="sm" @click="retrySuperProps" :disabled="retryingMode" class="h-7 px-2">
                  <Loader2 v-if="retryingMode" class="w-3 h-3 animate-spin" />
                  <RotateCw v-else class="w-3 h-3" />
                </Button>
              </div>
            </div>
            <div v-if="superPropsMode?.build_number" class="text-xs text-muted-foreground">
              Build: <span class="font-mono">{{ superPropsMode.build_number }}</span>
            </div>
            <p class="text-xs text-muted-foreground">
              <template v-if="superPropsMode?.mode === 'cdp'">✓ {{ t('settings.mode_cdp_desc') }}</template>
              <template v-else-if="superPropsMode?.mode === 'remote_js'">⚠ {{ t('settings.mode_remote_desc') }}</template>
              <template v-else>⚠ {{ t('settings.mode_default_desc') }}</template>
            </p>
          </div>

          <!-- Create Shortcut Section -->
          <div class="pt-2 border-t space-y-2">
            <p class="text-sm text-muted-foreground">{{ t('settings.cdp_shortcut_desc') }}</p>
            <div class="flex items-center gap-2">
              <Button variant="outline" @click="createShortcut" :disabled="shortcutCreating">
                <Loader2 v-if="shortcutCreating" class="w-4 h-4 mr-2 animate-spin" />
                {{ t('settings.cdp_create_shortcut') }}
              </Button>
              <span v-if="shortcutSuccess" class="text-sm text-green-500 flex items-center gap-1">
                <Check class="w-4 h-4" /> {{ t('settings.cdp_shortcut_success') }}
              </span>
              <span v-if="shortcutError" class="text-sm text-red-500">{{ shortcutError }}</span>
            </div>
          </div>
        </CardContent>
      </Card>

      <!-- Cache -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.cache') }}</CardTitle>
          <CardDescription>{{ t('settings.cache_desc') }}</CardDescription>
        </CardHeader>
        <CardContent>
          <div class="space-y-4">
             <div class="flex items-center gap-2 p-3 bg-muted/50 rounded-lg" v-if="cachePath">
                <code class="flex-1 text-xs font-mono break-all">{{ cachePath }}</code>
                <Button variant="ghost" size="icon" class="h-7 w-7 shrink-0" @click="copyPath">
                  <Check v-if="copied" class="w-3.5 h-3.5 text-green-500" />
                  <Copy v-else class="w-3.5 h-3.5" />
                </Button>
              </div>
             <Button variant="outline" @click="openCacheDir">
               <FolderOpen class="w-4 h-4 mr-2" />
               {{ t('settings.open_cache_dir') }}
             </Button>
          </div>
        </CardContent>
      </Card>
      
      <!-- Diagnostics -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.diagnostics') }}</CardTitle>
          <CardDescription>{{ t('settings.diagnostics_desc') }}</CardDescription>
        </CardHeader>
        <CardContent>
          <div class="space-y-4">
            <p class="text-sm text-muted-foreground">
              {{ t('settings.diagnostics_info') }}
            </p>
            <div class="flex items-center gap-3">
              <Button variant="outline" @click="exportLogs" :disabled="exporting">
                <Download v-if="!exporting" class="w-4 h-4 mr-2" />
                <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
                {{ t('settings.export_logs') }}
              </Button>
              <span v-if="exportSuccess" class="text-sm text-green-500 flex items-center gap-1">
                <Check class="w-4 h-4" /> {{ t('settings.export_success') }}
              </span>
              <span v-if="exportError" class="text-sm text-red-500">
                {{ t('settings.export_error') }}
              </span>
            </div>
          </div>
        </CardContent>
      </Card>
      
      <!-- About -->
      <div class="grid md:grid-cols-2 gap-6">
         <Card>
           <CardHeader>
             <CardTitle class="text-lg">{{ t('settings.about') }}</CardTitle>
           </CardHeader>
           <CardContent class="text-sm text-muted-foreground space-y-2">
              <p class="flex items-center flex-wrap gap-2">
                <span 
                  class="select-none cursor-pointer active:scale-95 transition-transform" 
                  @click="handleVersionTap"
                  title="Version Info"
                >
                  Discord Quest Helper v{{ versionStore.currentVersion }}
                </span>
                <Badge v-if="versionStore.isLatest" variant="outline" class="gap-1 text-green-600 border-green-600/50">
                  <CheckCircle2 class="w-3 h-3" />
                  {{ t('settings.version_latest') }}
                </Badge>
                <span v-else-if="versionStore.isChecking" class="text-xs text-muted-foreground">
                  {{ t('settings.version_checking') }}
                </span>
                
                <!-- Debug Unlock Hint -->
                <span v-if="showDebugUnlockHint" class="text-xs text-primary font-medium animate-pulse ml-2">
                  You are {{ 7 - versionTapCount }} steps away from being a developer.
                </span>
              </p>
             <p>
                {{ t('settings.about_desc') }}
              </p>
              <a href="#" @click.prevent="openExternal('https://github.com/Masterain98/discord-quest-helper')" class="inline-flex items-center gap-2 hover:opacity-80 transition-opacity">
                <img src="/icons/github-mark.svg" alt="GitHub" class="w-5 h-5 dark:hidden" />
                <img src="/icons/github-mark-white.svg" alt="GitHub" class="w-5 h-5 hidden dark:block" />
                <span class="text-primary hover:underline">Masterain98/discord-quest-helper</span>
              </a>
              <div class="flex flex-wrap gap-2 pt-2">
                <Button variant="outline" size="sm" @click="openExternal('https://github.com/Masterain98/discord-quest-helper/issues/new/choose')">
                  {{ t('settings.feedback') }}
                </Button>
                <Button variant="outline" size="sm" @click="openExternal('https://discord-quest-helper.dal.ao/')">
                  {{ t('settings.website') }}
                </Button>
              </div>
              <p class="text-yellow-500/90 dark:text-yellow-400">
                ⚠️ {{ t('settings.about_warning') }}
              </p>
            </CardContent>
          </Card>
          
           <Card>
             <CardHeader>
               <CardTitle class="text-lg">{{ t('settings.credits') }}</CardTitle>
             </CardHeader>
             <CardContent class="text-sm text-muted-foreground space-y-4">
               <div>
                 <p class="font-medium text-foreground mb-2">{{ t('settings.credits_desc') }}</p>
                 <ul class="space-y-2">
                   <li>
                     <a href="#" @click.prevent="openExternal('https://github.com/markterence/discord-quest-completer')" class="inline-flex items-center gap-2 hover:opacity-80 transition-opacity">
                       <img src="/icons/github-mark.svg" alt="GitHub" class="w-4 h-4 dark:hidden" />
                       <img src="/icons/github-mark-white.svg" alt="GitHub" class="w-4 h-4 hidden dark:block" />
                       <span class="hover:underline">markterence/discord-quest-completer</span>
                     </a>
                   </li>
                   <li>
                     <a href="#" @click.prevent="openExternal('https://github.com/power0matin/discord-quest-auto-completer')" class="inline-flex items-center gap-2 hover:opacity-80 transition-opacity">
                       <img src="/icons/github-mark.svg" alt="GitHub" class="w-4 h-4 dark:hidden" />
                       <img src="/icons/github-mark-white.svg" alt="GitHub" class="w-4 h-4 hidden dark:block" />
                       <span class="hover:underline">power0matin/discord-quest-auto-completer</span>
                     </a>
                   </li>
                 </ul>
               </div>
              <div>
                <p class="font-medium text-foreground mb-1">{{ t('settings.tech_stack') }}</p>
                <ul class="list-disc list-inside">
                  <li>Tauri</li>
                  <li>Vue 3</li>
                  <li>shadcn-vue</li>
                  <li>TailwindCSS</li>
                  <li>vue-i18n</li>
                </ul>
              </div>
            </CardContent>
          </Card>
      </div>
    </div>
  </div>
</template>
