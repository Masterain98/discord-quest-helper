<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { Check, Copy, FolderOpen, Loader2, RotateCw, SlidersHorizontal } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { documentDir, join } from '@tauri-apps/api/path'
import { mkdir } from '@tauri-apps/plugin-fs'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import { getSuperPropertiesMode, retrySuperProperties, type SuperPropertiesModeInfo } from '@/api/tauri'
import SettingRow from './SettingRow.vue'
import { navigateToTab } from '@/utils/navigate'
import SettingsSectionCard from './SettingsSectionCard.vue'
import { cn } from '@/lib/utils'
import { settingToneClass, type SettingsTone } from './settingTones'

function goToPortSection() {
  navigateToTab('settings', 'discord_integration')
  nextTick(() => {
    setTimeout(() => {
      document.getElementById('custom-port-section')?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    }, 100)
  })
}

const { t } = useI18n()
const questsStore = useQuestsStore()

const cachePath = ref('')
const copied = ref(false)
const superPropsMode = ref<SuperPropertiesModeInfo | null>(null)
const retryingMode = ref(false)
const debugModeEnabled = ref(localStorage.getItem('debugMode') === 'true')

const superPropsTone = computed<SettingsTone>(() => {
  if (superPropsMode.value?.mode === 'cdp') return 'success'
  if (superPropsMode.value?.mode === 'remote_js') return 'warning'
  return 'danger'
})

const developerModeTone = computed<SettingsTone>(() => debugModeEnabled.value ? 'success' : 'neutral')

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
  cachePath.value = await join(docDir, 'DiscordQuestGames')
  debugModeEnabled.value = localStorage.getItem('debugMode') === 'true'
  await loadSuperPropsMode()
})
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.advanced_title')"
    :description="t('settings.advanced_desc')"
    :icon="SlidersHorizontal"
    tone="warning"
    content-class="space-y-5"
  >
      <div class="rounded-lg border px-4">
        <SettingRow :label="t('settings.cdp_port')" :description="t('settings.cdp_port_hint')">
          <div class="flex items-center gap-2">
            <Badge variant="outline" class="border-primary/40 bg-primary/10 font-mono text-primary">
              {{ questsStore.cdpPort }}
            </Badge>
            <Button
              variant="outline"
              size="sm"
              :class="settingToneClass.primary.buttonSoft"
              @click="goToPortSection"
            >
              {{ t('settings.edit_port') }}
            </Button>
          </div>
        </SettingRow>

        <SettingRow :label="t('settings.super_props_mode')" :description="t('settings.super_props_mode_desc')">
          <div class="flex items-center gap-2">
            <Badge
              variant="outline"
              :class="settingToneClass[superPropsTone].badge"
            >
              {{ superPropsMode?.mode === 'cdp' ? 'CDP' : (superPropsMode?.mode === 'remote_js' ? t('settings.remote_js') : t('settings.default_mode')) }}
            </Badge>
            <Button
              variant="outline"
              size="sm"
              :aria-label="t('settings.super_props_mode_desc')"
              :class="cn('h-7 gap-1 px-2', settingToneClass.info.buttonSoft)"
              @click="retrySuperProps"
              :disabled="retryingMode"
            >
              <Loader2 v-if="retryingMode" class="h-3 w-3 animate-spin" />
              <RotateCw v-else class="h-3 w-3" />
            </Button>
          </div>
        </SettingRow>

        <SettingRow :label="t('settings.developer_mode')" :description="t('settings.developer_mode_desc')">
          <Badge variant="outline" :class="settingToneClass[developerModeTone].badge">
            {{ debugModeEnabled ? t('settings.debug_already_unlocked') : t('settings.developer_mode_locked') }}
          </Badge>
        </SettingRow>
      </div>

      <div class="space-y-3 rounded-lg border border-sky-500/25 bg-sky-500/5 p-4">
        <div>
          <Label>{{ t('settings.cache') }}</Label>
          <p class="mt-1 text-xs text-muted-foreground">{{ t('settings.cache_desc') }}</p>
        </div>
        <div class="flex items-center gap-2 rounded-md border bg-background/80 p-3" v-if="cachePath">
          <code class="flex-1 break-all text-xs font-mono">{{ cachePath }}</code>
          <Button
            variant="ghost"
            size="icon"
            :aria-label="t('debug.copy')"
            class="h-7 w-7 shrink-0 text-sky-700 hover:bg-sky-500/10 hover:text-sky-700 dark:text-sky-300 dark:hover:text-sky-300"
            @click="copyPath"
          >
            <Check v-if="copied" class="h-3.5 w-3.5" />
            <Copy v-else class="h-3.5 w-3.5" />
          </Button>
        </div>
        <Button variant="outline" :class="cn('gap-2', settingToneClass.info.buttonSoft)" @click="openCacheDir">
          <FolderOpen class="h-4 w-4" />
          {{ t('settings.open_cache_dir') }}
        </Button>
      </div>
  </SettingsSectionCard>
</template>
