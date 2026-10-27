<script setup lang="ts">
import { computed, onMounted, ref, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { open as openExternal } from '@tauri-apps/plugin-shell'
import { CheckCircle2, CircleAlert, Download, KeyRound, Loader2, MonitorUp } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  getDesktopClientState,
  getLiveTunnelStatus,
  launchDesktopClientCdp,
  startLiveTunnel,
  startProtonTunnel,
  stopLiveTunnel,
  type LiveTunnelStatus,
} from '@/api/tauri'
import { useQuestsStore } from '@/stores/quests'
import { commandErrorMessage } from '@/utils/commandError'
import SettingRow from './SettingRow.vue'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import type { SettingsTone } from './settingTones'

const { t } = useI18n()
const questsStore = useQuestsStore()
const CONFIG_STORAGE_KEY = 'questHelper_liveTunnelConfigPath'
const PROTON_USERNAME_KEY = 'questHelper_protonUsername'
const PROTON_COUNTRIES_KEY = 'questHelper_protonCountries'

const configPath = ref(localStorage.getItem(CONFIG_STORAGE_KEY) ?? '')
const status = ref<LiveTunnelStatus | null>(null)
const busy = ref(false)
const error = ref('')
const protonUsername = ref(localStorage.getItem(PROTON_USERNAME_KEY) ?? '')
const protonPassword = ref('')
const protonCountries = ref(localStorage.getItem(PROTON_COUNTRIES_KEY) ?? 'US,NL,JP')
const protonError = ref('')

const configName = computed(() => configPath.value.split(/[\\/]/).pop() ?? '')
const displayError = computed(() => error.value || status.value?.error || '')

type PanelState = {
  tone: SettingsTone
  icon: Component
  text: string
}

const panel = computed<PanelState>(() => {
  if (!status.value) {
    return { tone: 'neutral', icon: Loader2, text: t('general.loading') }
  }
  if (status.value.running) {
    return { tone: 'success', icon: CheckCircle2, text: t('settings.live_tunnel_running') }
  }
  if (status.value.externalWiresockRunning) {
    return { tone: 'warning', icon: CircleAlert, text: t('settings.live_tunnel_external_running') }
  }
  if (!status.value.wiresockInstalled) {
    return { tone: 'warning', icon: CircleAlert, text: t('settings.live_tunnel_wiresock_missing') }
  }
  if (!configPath.value) {
    return { tone: 'neutral', icon: MonitorUp, text: t('settings.live_tunnel_config_required') }
  }
  return { tone: 'neutral', icon: MonitorUp, text: t('settings.live_tunnel_stopped') }
})

const canStart = computed(() => (
  !!status.value
  && status.value.wiresockInstalled
  && !status.value.running
  && !status.value.externalWiresockRunning
  && !!configPath.value
))

async function refreshStatus() {
  try {
    status.value = await getLiveTunnelStatus()
  } catch (e) {
    error.value = commandErrorMessage(e)
  }
}

async function pickConfig() {
  error.value = ''
  try {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: t('settings.live_tunnel_config_label'), extensions: ['conf'] }],
    })
    if (typeof selected === 'string') {
      configPath.value = selected
      localStorage.setItem(CONFIG_STORAGE_KEY, selected)
    }
  } catch (e) {
    error.value = commandErrorMessage(e)
  }
}

async function start() {
  if (!configPath.value) {
    error.value = t('settings.live_tunnel_config_required')
    return
  }
  busy.value = true
  error.value = ''
  try {
    status.value = await startLiveTunnel(configPath.value)
  } catch (e) {
    error.value = commandErrorMessage(e)
    await refreshStatus()
  } finally {
    busy.value = false
  }
}

async function stop() {
  busy.value = true
  error.value = ''
  try {
    status.value = await stopLiveTunnel()
  } catch (e) {
    error.value = commandErrorMessage(e)
    await refreshStatus()
  } finally {
    busy.value = false
  }
}

async function restartDiscord(): Promise<boolean> {
  busy.value = true
  error.value = ''
  try {
    const state = await getDesktopClientState(questsStore.cdpPort)
    const result = await launchDesktopClientCdp(questsStore.cdpPort, state.selection, true)
    questsStore.cdpAvailable = result.cdp_connected
    return true
  } catch (e) {
    error.value = commandErrorMessage(e)
    return false
  } finally {
    busy.value = false
  }
}

async function startProton() {
  protonError.value = ''
  if (!protonUsername.value.trim()) {
    protonError.value = t('settings.proton_username_required')
    return
  }

  busy.value = true
  error.value = ''
  try {
    status.value = await startProtonTunnel(
      protonUsername.value.trim(),
      protonPassword.value,
      protonCountries.value,
    )
    protonPassword.value = ''
    localStorage.setItem(PROTON_USERNAME_KEY, protonUsername.value.trim())
    localStorage.setItem(PROTON_COUNTRIES_KEY, protonCountries.value)
    if (status.value.running) {
      await new Promise(resolve => setTimeout(resolve, 800))
      let restarted = await restartDiscord()
      if (!restarted) {
        busy.value = true
        await new Promise(resolve => setTimeout(resolve, 1200))
        restarted = await restartDiscord()
      }
      if (!restarted) {
        error.value = ''
        protonError.value = t('settings.live_tunnel_restart_hint')
      }
    }
  } catch (e) {
    const message = commandErrorMessage(e)
    const twoFactor = /2fa|controlling terminal/i.test(message)
    protonError.value = twoFactor
      ? `${message} ${t('settings.proton_2fa_hint')}`
      : message
    await refreshStatus()
  } finally {
    busy.value = false
  }
}

async function getWiresock() {
  error.value = ''
  try {
    await openExternal('https://wiresock.net/wiresock-sdk')
  } catch (e) {
    error.value = commandErrorMessage(e)
  }
}

onMounted(refreshStatus)
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.proton_title')"
    :description="t('settings.proton_desc')"
    :icon="KeyRound"
    tone="violet"
  >
    <SettingRow :label="t('settings.proton_username_label')">
      <Input
        v-model="protonUsername"
        class="max-w-72"
        autocomplete="username"
        :aria-label="t('settings.proton_username_label')"
      />
    </SettingRow>

    <SettingRow
      :label="t('settings.proton_password_label')"
      :description="t('settings.proton_password_desc')"
    >
      <Input
        v-model="protonPassword"
        type="password"
        class="max-w-72"
        autocomplete="off"
        :placeholder="t('settings.proton_password_placeholder')"
        :aria-label="t('settings.proton_password_label')"
      />
    </SettingRow>

    <SettingRow
      :label="t('settings.proton_countries_label')"
      :description="t('settings.proton_countries_desc')"
    >
      <Input
        v-model="protonCountries"
        class="max-w-48"
        autocomplete="off"
        :aria-label="t('settings.proton_countries_label')"
      />
    </SettingRow>

    <div class="flex flex-wrap items-center gap-3">
      <Button
        :disabled="busy || status?.running"
        @click="startProton"
      >
        <Loader2 v-if="busy" class="mr-2 h-4 w-4 animate-spin" />
        {{ t('settings.proton_generate_start') }}
      </Button>
      <span class="text-xs leading-relaxed text-muted-foreground">
        {{ t('settings.proton_session_hint') }}
      </span>
    </div>

    <p class="text-xs leading-relaxed text-muted-foreground">
      {{ t('settings.proton_wiresock_auto') }}
    </p>
    <p class="text-xs leading-relaxed text-muted-foreground">
      {{ t('settings.proton_2fa_hint') }}
    </p>
    <p class="text-xs leading-relaxed text-muted-foreground">
      {{ t('settings.proton_license_note') }}
    </p>
    <p v-if="protonError" role="alert" class="text-xs leading-relaxed text-destructive">
      {{ protonError }}
    </p>
  </SettingsSectionCard>

  <SettingsSectionCard
    class="mt-7 sm:mt-9"
    :title="t('settings.live_tunnel_title')"
    :description="t('settings.live_tunnel_desc')"
    :icon="MonitorUp"
    tone="info"
  >
    <SettingRow
      :label="t('settings.live_tunnel_config_label')"
      :description="t('settings.live_tunnel_config_desc')"
    >
      <div class="flex items-center gap-2">
        <span v-if="configName" class="max-w-48 truncate text-xs text-muted-foreground">{{ configName }}</span>
        <Button variant="outline" size="sm" :disabled="busy" @click="pickConfig">
          {{ t('settings.live_tunnel_choose') }}
        </Button>
      </div>
    </SettingRow>

    <SettingsStatusPanel :tone="panel.tone" :icon="panel.icon">
      <span role="status">{{ panel.text }}</span>
      <template #actions>
        <div v-if="status?.running" class="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="busy"
            @click="restartDiscord"
          >
            <Loader2 v-if="busy" class="mr-2 h-4 w-4 animate-spin" />
            {{ t('settings.cdp_restart') }}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            :disabled="busy"
            @click="stop"
          >
            {{ t('settings.live_tunnel_stop') }}
          </Button>
        </div>
        <Button
          v-else-if="status && !status.wiresockInstalled"
          variant="outline"
          size="sm"
          :disabled="busy"
          @click="getWiresock"
        >
          <Download class="mr-2 h-4 w-4" />
          {{ t('settings.live_tunnel_get_wiresock') }}
        </Button>
        <Button
          v-else-if="canStart"
          size="sm"
          :disabled="busy"
          @click="start"
        >
          <Loader2 v-if="busy" class="mr-2 h-4 w-4 animate-spin" />
          {{ t('settings.live_tunnel_start') }}
        </Button>
      </template>
    </SettingsStatusPanel>

    <p class="text-xs leading-relaxed text-muted-foreground">
      {{ t('settings.live_tunnel_restart_hint') }}
    </p>
    <p class="text-xs leading-relaxed text-muted-foreground">
      {{ t('settings.live_tunnel_warning') }}
    </p>
    <p class="text-xs leading-relaxed text-muted-foreground">
      {{ t('settings.live_tunnel_exit_note') }}
    </p>
    <p v-if="displayError" role="alert" class="text-xs leading-relaxed text-destructive">
      {{ displayError }}
    </p>
  </SettingsSectionCard>
</template>
