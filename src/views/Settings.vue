<script setup lang="ts">
import { ref } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Eye, EyeOff, Loader2 } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const manualToken = ref('')
const showToken = ref(false)

async function handleManualLogin() {
  if (manualToken.value) {
    await authStore.loginWithToken(manualToken.value)
  }
}

// Version management
const appVersion = ref('Dev')
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
  const docDir = await documentDir()
  const path = `${docDir}\\DiscordQuestGames`
  
  try {
    // Try to create it recursively (will not fail if exists)
    await mkdir(path, { recursive: true })
    
    // Use custom Rust command to ensure explorer opens
    await invoke('open_in_explorer', { path })
  } catch (e) {
    console.error('Failed to open cache dir:', e)
    // Fallback using same custom command
    try {
        await invoke('open_in_explorer', { path: docDir })
    } catch (e2) {
        console.error('Fallback failed:', e2)
    }
  }
}


import { onMounted } from 'vue'

onMounted(async () => {
    const docDir = await documentDir()
    cachePath.value = `${docDir}\\DiscordQuestGames`
// ... existing onMounted logic ...
  try {
    // In dev, this might 404 if not generated, handling gracefully
    const res = await fetch('/version.txt')
    if (res.ok) {
      const text = await res.text()
      if (text) {
        appVersion.value = text.trim()
      }
    }
  } catch (e) {
    // Keep default 'Dev'
  }
})
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
          <div v-if="authStore.user" class="p-3 bg-green-500/10 border border-green-500/20 rounded-md text-green-500 flex items-center gap-2">
            <span class="h-2 w-2 rounded-full bg-green-500"></span>
            {{ t('auth.authenticated_as') }} <span class="font-bold">{{ authStore.user.username }}</span>
          </div>
          
          <div v-else class="space-y-4">
             <Button 
               @click="authStore.tryAutoDetect" 
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
      
      <!-- Quest Configuration -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t('settings.quest_config') }}</CardTitle>
          <CardDescription>{{ t('settings.quest_config_desc') }}</CardDescription>
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
               max="10"
               step="1"
               class="w-full h-2 bg-secondary rounded-lg appearance-none cursor-pointer accent-primary"
             />
             <div class="flex justify-between text-xs text-muted-foreground">
               <span>1s ({{ t('settings.interval_fast') }})</span>
               <span>10s ({{ t('settings.interval_slow') }})</span>
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
             <div class="p-3 bg-muted/50 rounded-lg text-xs font-mono break-all" v-if="cachePath">
               {{ cachePath }}
             </div>
             <Button variant="outline" @click="openCacheDir">
               <FolderOpen class="w-4 h-4 mr-2" />
               {{ t('settings.open_cache_dir') }}
             </Button>
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
             <p>Discord Quest Helper v{{ appVersion }}</p>
             <p>
               {{ t('settings.about_desc') }}
             </p>
             <p>
               Project: 
               <a href="#" @click.prevent="openExternal('https://github.com/Masterain98/discord-quest-helper')" class="text-primary hover:underline">
                 GitHub
               </a>
             </p>
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
                <p class="font-medium text-foreground mb-1">{{ t('settings.credits_desc') }}</p>
                <ul class="list-disc list-inside">
                  <li>
                    <a href="#" @click.prevent="openExternal('https://github.com/markterence/discord-quest-completer')" class="hover:underline">
                      markterence/discord-quest-completer
                    </a>
                  </li>
                  <li>
                    <a href="#" @click.prevent="openExternal('https://github.com/power0matin/discord-quest-auto-completer')" class="hover:underline">
                      power0matin/discord-quest-auto-completer
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
