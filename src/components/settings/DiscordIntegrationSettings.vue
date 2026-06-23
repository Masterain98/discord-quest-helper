<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Check, Link2, Loader2, Play, Wifi, WifiOff } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import {
  checkCdpStatus,
  createDiscordCdpLauncherShortcut,
  fetchSuperPropertiesCdp,
  getDebugInfo,
  isDiscordRunning,
  launchDiscordCdp,
  restartDiscordCdp,
  type CdpStatus,
  type DebugInfo
} from '@/api/tauri'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from '@/components/ui/alert-dialog'
import AdvancedDisclosure from './AdvancedDisclosure.vue'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import { cn } from '@/lib/utils'
import { settingToneClass, type SettingsTone } from './settingTones'

const { t } = useI18n()
const questsStore = useQuestsStore()

const cdpStatus = ref<CdpStatus | null>(null)
const cdpChecking = ref(false)

// Dynamic button label: show "Restart" when CDP is already connected
const cdpPrimaryLabelKey = computed(() =>
  cdpStatus.value?.connected ? 'settings.cdp_restart' : 'settings.cdp_launch'
)
const cdpFetching = ref(false)
const cdpFetchSuccess = ref(false)
const cdpFetchError = ref('')
const cdpActionBusy = ref(false)
const cdpLaunchSuccess = ref('')
const cdpLaunchError = ref('')
const shortcutCreating = ref(false)
const shortcutSuccess = ref(false)
const shortcutError = ref('')
const cdpDialogOpen = ref(false)
const discordWasRunning = ref(false)
const discordWasConnected = ref(false)
const debugInfo = ref<DebugInfo | null>(null)
const debugInfoLoading = ref(false)

const cdpSectionTone = computed<SettingsTone>(() => cdpStatus.value?.connected ? 'success' : 'warning')
const cdpStatusTone = computed<SettingsTone>(() => {
  if (cdpChecking.value) return 'info'
  return cdpStatus.value?.connected ? 'success' : 'warning'
})
const cdpStatusIcon = computed(() => {
  if (cdpChecking.value) return Loader2
  return cdpStatus.value?.connected ? Wifi : WifiOff
})
const debugInfoSourceTone = computed<SettingsTone>(() => {
  const source = debugInfo.value?.source ?? ''
  if (source.includes('CDP')) return 'success'
  if (source.includes('Remote')) return 'warning'
  return 'danger'
})

async function checkCdp() {
  cdpChecking.value = true
  try {
    cdpStatus.value = await checkCdpStatus(questsStore.cdpPort)
    questsStore.cdpAvailable = cdpStatus.value.connected
  } catch (e) {
    cdpStatus.value = { available: false, connected: false, target_title: null, error: String(e) }
    questsStore.cdpAvailable = false
  } finally {
    cdpChecking.value = false
  }
}

async function loadDebugInfo() {
  debugInfoLoading.value = true
  try {
    debugInfo.value = await getDebugInfo()
  } catch (e) {
    console.error('Failed to load debug info:', e)
  } finally {
    debugInfoLoading.value = false
  }
}

async function fetchCdpSuperProperties() {
  cdpFetching.value = true
  cdpFetchSuccess.value = false
  cdpFetchError.value = ''
  try {
    await fetchSuperPropertiesCdp(questsStore.cdpPort)
    cdpFetchSuccess.value = true
    setTimeout(() => { cdpFetchSuccess.value = false }, 5000)
    await checkCdp()
    await loadDebugInfo()
  } catch (e) {
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
    await createDiscordCdpLauncherShortcut(questsStore.cdpPort, 'auto')
    shortcutSuccess.value = true
    setTimeout(() => { shortcutSuccess.value = false }, 3000)
  } catch (e) {
    shortcutError.value = String(e)
    setTimeout(() => { shortcutError.value = '' }, 5000)
  } finally {
    shortcutCreating.value = false
  }
}

function resetLaunchMessage() {
  cdpLaunchSuccess.value = ''
  cdpLaunchError.value = ''
}

async function requestCdpAction() {
  resetLaunchMessage()
  cdpActionBusy.value = true
  try {
    const running = await isDiscordRunning('auto')
    discordWasRunning.value = running
    discordWasConnected.value = !!cdpStatus.value?.connected
    if (running) {
      cdpDialogOpen.value = true
      return
    }
    await performLaunch(false)
  } catch (e) {
    cdpLaunchError.value = String(e)
    setTimeout(() => { cdpLaunchError.value = '' }, 6000)
  } finally {
    cdpActionBusy.value = false
  }
}

async function confirmCdpAction() {
  if (cdpActionBusy.value) return
  cdpDialogOpen.value = false
  await performLaunch(true)
}

async function performLaunch(restart: boolean) {
  cdpActionBusy.value = true
  resetLaunchMessage()

  try {
    const result = restart
      ? await restartDiscordCdp(questsStore.cdpPort, 'auto')
      : await launchDiscordCdp(questsStore.cdpPort, 'auto')
    cdpLaunchSuccess.value = result.cdp_connected
      ? t('settings.cdp_launch_success')
      : t('settings.cdp_launch_started')
    setTimeout(() => { cdpLaunchSuccess.value = '' }, 5000)
    await checkCdp()
  } catch (e) {
    cdpLaunchError.value = String(e)
    setTimeout(() => { cdpLaunchError.value = '' }, 8000)
  } finally {
    cdpActionBusy.value = false
  }
}

onMounted(() => {
  checkCdp()
  loadDebugInfo()
})
</script>

<template>
  <AlertDialog :open="cdpDialogOpen" @update:open="cdpDialogOpen = $event">
    <AlertDialogContent class="max-w-[520px]">
      <AlertDialogHeader>
        <AlertDialogTitle>
          {{ discordWasConnected ? t('settings.cdp_dialog_title_connected') : t('settings.cdp_dialog_title_disconnected') }}
        </AlertDialogTitle>
        <AlertDialogDescription>
          {{ discordWasConnected ? t('settings.cdp_dialog_desc_connected') : t('settings.cdp_dialog_desc_disconnected') }}
        </AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
        <AlertDialogAction :disabled="cdpActionBusy" @click="confirmCdpAction">
          {{ t('settings.cdp_dialog_confirm') }}
        </AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>

  <SettingsSectionCard
    :title="t('settings.cdp_title')"
    :description="t('settings.cdp_desc')"
    :icon="cdpStatus?.connected ? Wifi : WifiOff"
    :tone="cdpSectionTone"
    content-class="space-y-5"
  >
      <SettingsStatusPanel :tone="cdpStatusTone" :icon="cdpStatusIcon">
        <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <span class="min-w-0">
            <template v-if="cdpChecking">{{ t('settings.cdp_checking') }}</template>
            <template v-else-if="cdpStatus?.connected">
              {{ t('settings.cdp_connected') }}
              <span v-if="cdpStatus.target_title" class="ml-1 text-muted-foreground">({{ cdpStatus.target_title }})</span>
            </template>
            <template v-else>{{ t('settings.cdp_disconnected') }}</template>
          </span>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 gap-2 text-muted-foreground hover:text-foreground"
            @click="checkCdp"
            :disabled="cdpChecking"
          >
            <Loader2 v-if="cdpChecking" class="h-4 w-4 animate-spin" />
            <template v-else>{{ t('general.refresh') }}</template>
          </Button>
        </div>
      </SettingsStatusPanel>

      <div class="space-y-3 rounded-lg border border-primary/20 bg-primary/5 p-4">
        <p class="text-sm font-semibold">{{ t('settings.integration_setup') }}</p>
        <p class="text-sm text-muted-foreground">{{ t('settings.cdp_launch_desc') }}</p>
        <div class="flex flex-wrap items-center gap-2">
          <Button
            :variant="cdpStatus?.connected ? 'outline' : 'default'"
            :class="cn('gap-2', cdpStatus?.connected ? settingToneClass.warning.buttonSoft : 'shadow-sm')"
            @click="requestCdpAction"
            :disabled="cdpActionBusy"
          >
            <Loader2 v-if="cdpActionBusy" class="h-4 w-4 animate-spin" />
            <Play v-else class="h-4 w-4" />
            {{ t(cdpPrimaryLabelKey) }}
          </Button>
        </div>
        <SettingsStatusPanel v-if="cdpLaunchSuccess" tone="success" :icon="Check">
          {{ cdpLaunchSuccess }}
        </SettingsStatusPanel>
        <SettingsStatusPanel v-if="cdpLaunchError" tone="danger">
          {{ cdpLaunchError }}
        </SettingsStatusPanel>
      </div>

      <div class="space-y-3 rounded-lg border border-sky-500/25 bg-sky-500/5 p-4">
        <p class="text-sm font-semibold">{{ t('settings.cdp_shortcut_title') }}</p>
        <p class="text-sm text-muted-foreground">{{ t('settings.cdp_shortcut_desc') }}</p>
        <div class="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            :class="cn('gap-2', settingToneClass.info.buttonSoft)"
            @click="createShortcut"
            :disabled="shortcutCreating"
          >
            <Loader2 v-if="shortcutCreating" class="h-4 w-4 animate-spin" />
            {{ t('settings.cdp_create_shortcut') }}
          </Button>
        </div>
        <SettingsStatusPanel v-if="shortcutSuccess" tone="success" :icon="Check">
          {{ t('settings.cdp_shortcut_success') }}
        </SettingsStatusPanel>
        <SettingsStatusPanel v-if="shortcutError" tone="danger">
          {{ shortcutError }}
        </SettingsStatusPanel>
      </div>

      <AdvancedDisclosure
        :title="t('settings.client_emulation')"
        :description="t('settings.client_emulation_desc')"
        tone="info"
        default-open
      >
        <div class="space-y-3">
          <div v-if="cdpStatus?.connected" class="flex flex-wrap items-center gap-3">
            <Button
              variant="outline"
              size="sm"
              :class="cn('gap-2', settingToneClass.primary.buttonSoft)"
              @click="fetchCdpSuperProperties"
              :disabled="cdpFetching"
            >
              <Loader2 v-if="cdpFetching" class="h-4 w-4 animate-spin" />
              <Link2 v-else class="h-4 w-4" />
              {{ t('settings.cdp_sync') }}
            </Button>
            <Badge
              v-if="cdpFetchSuccess"
              variant="outline"
              :class="cn('gap-1', settingToneClass.success.badge)"
            >
              <Check class="h-3 w-3" /> {{ t('settings.cdp_sync_success') }}
            </Badge>
            <Badge
              v-if="cdpFetchError"
              variant="outline"
              :class="settingToneClass.danger.badge"
            >
              {{ cdpFetchError }}
            </Badge>
          </div>
          <template v-if="debugInfo">
            <div class="flex items-center gap-2">
              <Badge
                variant="outline"
                :class="settingToneClass[debugInfoSourceTone].badge"
              >
                {{ debugInfo.source }}
              </Badge>
              <span class="text-xs text-muted-foreground">{{ t('settings.super_props_mode') }}</span>
            </div>
            <div class="grid gap-2 md:grid-cols-2">
              <div class="rounded-md border bg-muted/30 px-3 py-2">
                <p class="text-[11px] text-muted-foreground">{{ t('settings.field_client_build_number') }}</p>
                <p class="mt-1 truncate font-mono text-sm font-medium">{{ debugInfo.super_properties?.client_build_number }}</p>
              </div>
              <div class="rounded-md border bg-muted/30 px-3 py-2">
                <p class="text-[11px] text-muted-foreground">{{ t('settings.field_client_version') }}</p>
                <p class="mt-1 truncate font-mono text-sm font-medium">{{ debugInfo.super_properties?.client_version ?? '—' }}</p>
              </div>
              <div class="rounded-md border bg-muted/30 px-3 py-2">
                <p class="text-[11px] text-muted-foreground">{{ t('settings.field_native_build_number') }}</p>
                <p class="mt-1 truncate font-mono text-sm font-medium">{{ debugInfo.super_properties?.native_build_number ?? '—' }}</p>
              </div>
              <div class="rounded-md border bg-muted/30 px-3 py-2">
                <p class="text-[11px] text-muted-foreground">{{ t('settings.field_os') }}</p>
                <p class="mt-1 truncate font-mono text-sm font-medium">{{ debugInfo.super_properties?.os }} {{ debugInfo.super_properties?.os_version }}</p>
              </div>
              <div class="rounded-md border bg-muted/30 px-3 py-2">
                <p class="text-[11px] text-muted-foreground">{{ t('settings.field_browser') }}</p>
                <p class="mt-1 truncate font-mono text-sm font-medium">{{ debugInfo.super_properties?.browser }}</p>
              </div>
              <div class="rounded-md border bg-muted/30 px-3 py-2">
                <p class="text-[11px] text-muted-foreground">{{ t('settings.field_release_channel') }}</p>
                <p class="mt-1 truncate font-mono text-sm font-medium">{{ debugInfo.super_properties?.release_channel }}</p>
              </div>
              <div class="rounded-md border bg-muted/30 px-3 py-2">
                <p class="text-[11px] text-muted-foreground">{{ t('settings.field_system_locale') }}</p>
                <p class="mt-1 truncate font-mono text-sm font-medium">{{ debugInfo.super_properties?.system_locale }}</p>
              </div>
            </div>
          </template>
        </div>
      </AdvancedDisclosure>

      <AdvancedDisclosure
        id="custom-port-section"
        :title="t('settings.custom_port')"
        :description="t('settings.custom_port_desc')"
        tone="warning"
        default-open
      >
        <div class="space-y-2">
          <Label>{{ t('settings.cdp_port') }}</Label>
          <div class="flex items-center gap-2">
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
      </AdvancedDisclosure>
  </SettingsSectionCard>
</template>
