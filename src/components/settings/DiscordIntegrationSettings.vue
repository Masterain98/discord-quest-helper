<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Check, Link2, Loader2, Play, RotateCcw, Wifi, WifiOff } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import {
  checkCdpStatus,
  createDiscordCdpLauncherShortcut,
  fetchSuperPropertiesCdp,
  isDiscordRunning,
  launchDiscordCdp,
  restartDiscordCdp,
  type CdpStatus
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

const { t } = useI18n()
const questsStore = useQuestsStore()

const cdpStatus = ref<CdpStatus | null>(null)
const cdpChecking = ref(false)
const cdpFetching = ref(false)
const cdpFetchSuccess = ref(false)
const cdpFetchError = ref('')
const cdpLaunching = ref(false)
const cdpRestarting = ref(false)
const cdpLaunchSuccess = ref('')
const cdpLaunchError = ref('')
const shortcutCreating = ref(false)
const shortcutSuccess = ref(false)
const shortcutError = ref('')
const restartDialogOpen = ref(false)
const pendingAction = ref<'launch' | 'restart' | null>(null)

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

async function fetchCdpSuperProperties() {
  cdpFetching.value = true
  cdpFetchSuccess.value = false
  cdpFetchError.value = ''
  try {
    await fetchSuperPropertiesCdp(questsStore.cdpPort)
    cdpFetchSuccess.value = true
    setTimeout(() => { cdpFetchSuccess.value = false }, 5000)
    await checkCdp()
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

async function requestLaunchDiscordCdp() {
  resetLaunchMessage()
  try {
    const running = await isDiscordRunning('auto')
    if (running && !cdpStatus.value?.connected) {
      pendingAction.value = 'launch'
      restartDialogOpen.value = true
      return
    }
    await performLaunch(false)
  } catch (e) {
    cdpLaunchError.value = String(e)
    setTimeout(() => { cdpLaunchError.value = '' }, 6000)
  }
}

async function requestRestartDiscordCdp() {
  resetLaunchMessage()
  try {
    const running = await isDiscordRunning('auto')
    if (running) {
      pendingAction.value = 'restart'
      restartDialogOpen.value = true
      return
    }
    await performLaunch(false)
  } catch (e) {
    cdpLaunchError.value = String(e)
    setTimeout(() => { cdpLaunchError.value = '' }, 6000)
  }
}

async function confirmRestartDiscordCdp() {
  const action = pendingAction.value
  restartDialogOpen.value = false
  pendingAction.value = null
  if (!action) return
  await performLaunch(true)
}

async function performLaunch(restart: boolean) {
  if (restart) {
    cdpRestarting.value = true
  } else {
    cdpLaunching.value = true
  }
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
    cdpLaunching.value = false
    cdpRestarting.value = false
  }
}

onMounted(checkCdp)
</script>

<template>
  <AlertDialog :open="restartDialogOpen" @update:open="restartDialogOpen = $event">
    <AlertDialogContent class="max-w-[520px]">
      <AlertDialogHeader>
        <AlertDialogTitle>{{ t('settings.cdp_restart_confirm_title') }}</AlertDialogTitle>
        <AlertDialogDescription>
          {{ t('settings.cdp_restart_confirm_desc') }}
        </AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel @click="pendingAction = null">{{ t('dialog.cancel') }}</AlertDialogCancel>
        <AlertDialogAction @click="confirmRestartDiscordCdp">
          {{ t('settings.cdp_restart_confirm_action') }}
        </AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>

  <Card>
    <CardHeader>
      <CardTitle>{{ t('settings.cdp_title') }}</CardTitle>
      <CardDescription>{{ t('settings.cdp_desc') }}</CardDescription>
    </CardHeader>
    <CardContent class="space-y-5">
      <div
        class="flex items-center justify-between rounded-lg border p-3"
        :class="cdpStatus?.connected ? 'border-green-500/30 bg-green-500/10' : 'border-border bg-muted/50'"
      >
        <div class="flex items-center gap-2">
          <Wifi v-if="cdpStatus?.connected" class="h-4 w-4 text-green-500" />
          <WifiOff v-else class="h-4 w-4 text-muted-foreground" />
          <span class="text-sm">
            <template v-if="cdpChecking">{{ t('settings.cdp_checking') }}</template>
            <template v-else-if="cdpStatus?.connected">
              {{ t('settings.cdp_connected') }}
              <span v-if="cdpStatus.target_title" class="ml-1 text-muted-foreground">({{ cdpStatus.target_title }})</span>
            </template>
            <template v-else>{{ t('settings.cdp_disconnected') }}</template>
          </span>
        </div>
        <Button variant="ghost" size="sm" @click="checkCdp" :disabled="cdpChecking">
          <Loader2 v-if="cdpChecking" class="h-4 w-4 animate-spin" />
          <template v-else>{{ t('general.refresh') }}</template>
        </Button>
      </div>

      <div class="space-y-3 rounded-lg border border-border p-4">
        <p class="text-sm font-medium">{{ t('settings.integration_setup') }}</p>
        <p class="text-sm text-muted-foreground">{{ t('settings.cdp_launch_desc') }}</p>
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="secondary" @click="requestLaunchDiscordCdp" :disabled="cdpLaunching || cdpRestarting">
            <Loader2 v-if="cdpLaunching" class="mr-2 h-4 w-4 animate-spin" />
            <Play v-else class="mr-2 h-4 w-4" />
            {{ t('settings.cdp_launch') }}
          </Button>
          <Button variant="outline" @click="requestRestartDiscordCdp" :disabled="cdpLaunching || cdpRestarting">
            <Loader2 v-if="cdpRestarting" class="mr-2 h-4 w-4 animate-spin" />
            <RotateCcw v-else class="mr-2 h-4 w-4" />
            {{ t('settings.cdp_restart') }}
          </Button>
          <span v-if="cdpLaunchSuccess" class="flex items-center gap-1 text-sm text-green-500">
            <Check class="h-4 w-4" /> {{ cdpLaunchSuccess }}
          </span>
          <span v-if="cdpLaunchError" class="text-sm text-red-500">{{ cdpLaunchError }}</span>
        </div>
      </div>

      <div class="space-y-3 rounded-lg border border-border p-4">
        <p class="text-sm font-medium">{{ t('settings.cdp_shortcut_title') }}</p>
        <p class="text-sm text-muted-foreground">{{ t('settings.cdp_shortcut_desc') }}</p>
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="outline" @click="createShortcut" :disabled="shortcutCreating">
            <Loader2 v-if="shortcutCreating" class="mr-2 h-4 w-4 animate-spin" />
            {{ t('settings.cdp_create_shortcut') }}
          </Button>
          <span v-if="shortcutSuccess" class="flex items-center gap-1 text-sm text-green-500">
            <Check class="h-4 w-4" /> {{ t('settings.cdp_shortcut_success') }}
          </span>
          <span v-if="shortcutError" class="text-sm text-red-500">{{ shortcutError }}</span>
        </div>
      </div>

      <div v-if="cdpStatus?.connected" class="space-y-2">
        <div class="flex flex-wrap items-center gap-3">
          <Button variant="secondary" @click="fetchCdpSuperProperties" :disabled="cdpFetching">
            <Loader2 v-if="cdpFetching" class="mr-2 h-4 w-4 animate-spin" />
            <Link2 v-else class="mr-2 h-4 w-4" />
            {{ t('settings.cdp_fetch') }}
          </Button>
          <span v-if="cdpFetchSuccess" class="flex items-center gap-1 text-sm text-green-500">
            <Check class="h-4 w-4" /> {{ t('settings.cdp_fetch_success') }}
          </span>
          <span v-if="cdpFetchError" class="text-sm text-red-500">{{ cdpFetchError }}</span>
        </div>
      </div>

      <AdvancedDisclosure
        :title="t('settings.custom_port')"
        :description="t('settings.custom_port_desc')"
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
    </CardContent>
  </Card>
</template>
