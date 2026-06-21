<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Check, Copy, FolderOpen, Loader2, RotateCw } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { documentDir } from '@tauri-apps/api/path'
import { mkdir } from '@tauri-apps/plugin-fs'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import { getSuperPropertiesMode, retrySuperProperties, type SuperPropertiesModeInfo } from '@/api/tauri'
import SettingRow from './SettingRow.vue'

const { t } = useI18n()
const questsStore = useQuestsStore()

const cachePath = ref('')
const copied = ref(false)
const superPropsMode = ref<SuperPropertiesModeInfo | null>(null)
const retryingMode = ref(false)
const debugModeEnabled = ref(localStorage.getItem('debugMode') === 'true')

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
  } catch (e) {
    console.error('Retry failed:', e)
  } finally {
    retryingMode.value = false
  }
}

async function copyPath() {
  if (!cachePath.value) return
  await navigator.clipboard.writeText(cachePath.value)
  copied.value = true
  setTimeout(() => { copied.value = false }, 2000)
}

async function openCacheDir() {
  if (!cachePath.value) return
  try {
    await mkdir(cachePath.value, { recursive: true })
    await invoke('open_in_explorer', { path: cachePath.value })
  } catch (e) {
    console.error('Failed to open cache dir:', e)
  }
}

onMounted(async () => {
  const docDir = await documentDir()
  const normalizedDocDir = docDir.replace(/[\\/]+$/, '')
  cachePath.value = `${normalizedDocDir}\\DiscordQuestGames`
  debugModeEnabled.value = localStorage.getItem('debugMode') === 'true'
  await loadSuperPropsMode()
})
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle>{{ t('settings.advanced_title') }}</CardTitle>
      <CardDescription>{{ t('settings.advanced_desc') }}</CardDescription>
    </CardHeader>
    <CardContent class="space-y-5">
      <div class="rounded-lg border px-4">
        <SettingRow :label="t('settings.cdp_port')" :description="t('settings.cdp_port_hint')">
          <Input
            type="number"
            v-model.number="questsStore.cdpPort"
            min="1024"
            max="65535"
            class="w-32"
          />
        </SettingRow>

        <SettingRow :label="t('settings.super_props_mode')" :description="t('settings.super_props_mode_desc')">
          <div class="flex items-center gap-2">
            <Badge
              :variant="superPropsMode?.mode === 'cdp' ? 'default' : (superPropsMode?.mode === 'remote_js' ? 'secondary' : 'outline')"
              :class="[
                superPropsMode?.mode === 'cdp' && 'bg-green-500 text-white',
                superPropsMode?.mode === 'remote_js' && 'bg-yellow-500 text-black',
                superPropsMode?.mode === 'default' && 'border-red-500/50 bg-red-500/20 text-red-500',
              ]"
            >
              {{ superPropsMode?.mode === 'cdp' ? 'CDP' : (superPropsMode?.mode === 'remote_js' ? t('settings.remote_js') : t('settings.default_mode')) }}
            </Badge>
            <Button variant="ghost" size="sm" @click="retrySuperProps" :disabled="retryingMode" class="h-7 px-2">
              <Loader2 v-if="retryingMode" class="h-3 w-3 animate-spin" />
              <RotateCw v-else class="h-3 w-3" />
            </Button>
          </div>
        </SettingRow>

        <SettingRow :label="t('settings.developer_mode')" :description="t('settings.developer_mode_desc')">
          <Badge :variant="debugModeEnabled ? 'default' : 'outline'">
            {{ debugModeEnabled ? t('settings.debug_already_unlocked') : t('settings.developer_mode_locked') }}
          </Badge>
        </SettingRow>
      </div>

      <div class="space-y-3 rounded-lg border p-4">
        <div>
          <Label>{{ t('settings.cache') }}</Label>
          <p class="mt-1 text-xs text-muted-foreground">{{ t('settings.cache_desc') }}</p>
        </div>
        <div class="flex items-center gap-2 rounded-lg bg-muted/50 p-3" v-if="cachePath">
          <code class="flex-1 break-all text-xs font-mono">{{ cachePath }}</code>
          <Button variant="ghost" size="icon" class="h-7 w-7 shrink-0" @click="copyPath">
            <Check v-if="copied" class="h-3.5 w-3.5 text-green-500" />
            <Copy v-else class="h-3.5 w-3.5" />
          </Button>
        </div>
        <Button variant="outline" @click="openCacheDir">
          <FolderOpen class="mr-2 h-4 w-4" />
          {{ t('settings.open_cache_dir') }}
        </Button>
      </div>
    </CardContent>
  </Card>
</template>
