<script setup lang="ts">
import { ChevronDown, Globe } from 'lucide-vue-next'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  getNetworkSettings, saveNetworkSettings, testNetworkConnection,
  type NetworkConnectionTest, type NetworkSettings, type NetworkStatus, type ProxyCredentialEdit, type ProxyMode,
} from '@/api/network'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingRow from './SettingRow.vue'

const { t } = useI18n()
const modes: ProxyMode[] = ['system', 'direct', 'manual']
const status = ref<NetworkStatus>()
const mode = ref<ProxyMode>('system')
const address = ref('')
const username = ref('')
const password = ref('')
const editingCredentials = ref(false)
const busy = ref<'loading' | 'saving' | 'testing' | null>('loading')
const idle = computed(() => busy.value === null)
const error = ref<string>()
const result = ref<NetworkConnectionTest>()
const saved = ref(false)
let refresh: ReturnType<typeof setInterval> | undefined

const draft = computed<NetworkSettings>(() => ({
  mode: mode.value,
  address: mode.value === 'manual' ? address.value.trim() : null,
}))
function proxyAddressKey(raw: string | null): string | null {
  if (!raw) return null
  try {
    const url = new URL(raw)
    if (url.protocol === 'socks5:' || url.protocol === 'socks5h:') {
      url.port ||= '1080'
      if (url.pathname === '/') url.pathname = ''
    }
    return url.href
  } catch {
    return raw
  }
}

const hasSavedCredentials = computed(() => mode.value === 'manual' && status.value?.has_credentials === true)
const credentialsForAddress = computed(() => hasSavedCredentials.value
  && proxyAddressKey(status.value?.settings.address ?? null) === proxyAddressKey(draft.value.address))
const usingSavedCredentials = computed(() => credentialsForAddress.value && !editingCredentials.value)
const credentials = computed<ProxyCredentialEdit>(() => {
  if (mode.value === 'manual') {
    if (!usingSavedCredentials.value && (username.value || password.value)) {
      return { action: 'replace', username: username.value, password: password.value }
    }
    if (hasSavedCredentials.value && !editingCredentials.value) return { action: 'keep' }
  }
  return { action: 'remove' }
})
const usesUnencryptedCredentials = computed(() => mode.value === 'manual'
  && (credentials.value.action === 'replace' || usingSavedCredentials.value)
  && /^(http|socks5h?):\/\//i.test(draft.value.address ?? ''))
const dirty = computed(() => !status.value || status.value.storage_invalid
  || status.value.credential_cleanup_pending
  || status.value.settings.mode !== draft.value.mode
  || status.value.settings.address !== draft.value.address
  || credentials.value.action === 'replace'
  || (status.value.has_credentials && credentials.value.action === 'remove'))

watch([mode, address, username, password, editingCredentials], () => {
  error.value = undefined
  result.value = undefined
  saved.value = false
}, { flush: 'sync' })

function apply(next: NetworkStatus) {
  mode.value = next.settings.mode
  address.value = next.settings.address ?? ''
  status.value = next
  username.value = ''
  password.value = ''
  editingCredentials.value = false
}

function toggleCredentialEditing() {
  username.value = ''
  password.value = ''
  editingCredentials.value = !editingCredentials.value
}

async function refreshStatus() {
  if (!idle.value || status.value?.settings.mode !== 'system') return
  const previous = status.value
  try {
    const next = await getNetworkSettings()
    if (idle.value && status.value === previous) status.value = next
  } catch {
    // Keep the last readable status until the next refresh.
  }
}

onMounted(async () => {
  try {
    apply(await getNetworkSettings())
  } catch {
    error.value = 'storage_error'
  } finally {
    busy.value = null
  }
})
onMounted(() => { refresh = setInterval(refreshStatus, 5_000) })
onUnmounted(() => clearInterval(refresh))

async function submit(action: 'saving' | 'testing') {
  if (!idle.value || (action === 'saving' && !dirty.value)) return
  busy.value = action
  error.value = undefined
  result.value = undefined
  saved.value = false
  try {
    if (action === 'saving') {
      apply(await saveNetworkSettings(draft.value, credentials.value))
      saved.value = true
    } else {
      result.value = await testNetworkConnection(draft.value, credentials.value)
    }
  } catch (reason) {
    const codes = [
      'invalid_address', 'storage_error', 'timeout', 'connection_error', 'http_error', 'unexpected_response',
      'invalid_http_credentials', 'invalid_socks_credentials', 'credential_store_error', 'credential_endpoint_changed',
    ]
    error.value = typeof reason === 'string' && codes.includes(reason) ? reason : 'client_error'
  } finally {
    busy.value = null
  }
}
</script>

<template>
  <SettingsSectionCard :title="t('settings.nav_network')" :description="t('network.description')" :icon="Globe">
    <SettingRow id="settings-network-proxy" :label="t('network.mode')" :description="t('network.scope')">
      <DropdownMenu>
        <DropdownMenuTrigger as-child :disabled="!idle">
          <Button type="button" variant="outline" class="w-full justify-between font-normal sm:w-56" :aria-label="t('network.mode')">
            <span class="truncate">{{ t(`network.${mode}`) }}</span>
            <ChevronDown class="ml-2 h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" class="w-[var(--radix-dropdown-menu-trigger-width)]">
          <DropdownMenuRadioGroup v-model="mode">
            <DropdownMenuRadioItem v-for="option in modes" :key="option" :value="option">
              {{ t(`network.${option}`) }}
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingRow>

    <SettingRow v-if="mode === 'manual'" id="settings-network-address" :label="t('network.address')" :description="t('network.address_help')">
      <Input
        v-model="address"
        :aria-label="t('network.address')"
        :disabled="!idle"
        :aria-invalid="error === 'invalid_address' || undefined"
        placeholder="http://127.0.0.1:7890"
        autocomplete="off"
        autocapitalize="off"
        :spellcheck="false"
        class="w-full sm:w-56"
        @keydown.enter.prevent="submit('saving')"
      />
    </SettingRow>

    <SettingRow
      v-if="hasSavedCredentials"
      id="settings-network-credentials"
      :label="t('network.credentials_saved')"
      :description="editingCredentials ? t('network.credentials_remove_help') : undefined"
    >
      <Button type="button" variant="outline" :disabled="!idle" @click="toggleCredentialEditing">
        {{ t(editingCredentials ? 'network.credentials_keep' : 'network.credentials_edit') }}
      </Button>
    </SettingRow>

    <template v-if="mode === 'manual' && !usingSavedCredentials">
      <SettingRow id="settings-network-username" :label="t('network.username')" :description="t('network.credentials_optional')">
        <Input
          v-model="username"
          :aria-label="t('network.username')"
          :disabled="!idle"
          autocomplete="off"
          autocapitalize="off"
          :spellcheck="false"
          class="w-full sm:w-56"
        />
      </SettingRow>
      <SettingRow id="settings-network-password" :label="t('network.password')">
        <Input
          v-model="password"
          type="password"
          :aria-label="t('network.password')"
          :disabled="!idle"
          autocomplete="new-password"
          class="w-full sm:w-56"
          @keydown.enter.prevent="submit('saving')"
        />
      </SettingRow>
    </template>

    <p v-if="usesUnencryptedCredentials" class="text-sm text-muted-foreground">
      {{ t('network.unencrypted_credentials') }}
    </p>

    <SettingRow
      id="settings-network-status"
      :label="t('network.active_route')"
      :description="status?.pac_configured ? t('network.pac_help') : undefined"
    >
      <div v-if="status?.discord" class="text-sm sm:text-right">
        <p>{{ t(`network.${status.settings.mode}`) }}</p>
        <p v-if="status.settings.mode === 'system'" class="text-xs text-muted-foreground">{{ t(status.discord.source === 'unavailable' ? 'network.error_client_error' : `network.source_${status.discord.source}`) }}</p>
        <p v-if="status.discord.endpoint" class="break-all font-mono text-xs text-muted-foreground">{{ status.discord.endpoint }}</p>
      </div>
      <p v-else class="text-sm text-muted-foreground">{{ idle ? '—' : t('general.loading') }}</p>
    </SettingRow>

    <div class="flex flex-wrap gap-2">
      <Button type="button" variant="outline" :disabled="!idle" @click="submit('testing')">
        {{ busy === 'testing' ? t('general.loading') : t('network.test') }}
      </Button>
      <Button type="button" :disabled="!idle || !dirty" @click="submit('saving')">
        {{ busy === 'saving' ? t('general.loading') : t('network.save') }}
      </Button>
    </div>
    <p v-if="status?.credential_cleanup_pending" role="alert" class="text-sm text-muted-foreground">
      {{ t('network.credential_cleanup_pending') }}
    </p>
    <p v-if="error" role="alert" class="text-sm text-destructive">{{ t(`network.error_${error}`) }}</p>
    <p v-else-if="status?.storage_invalid" role="alert" class="text-sm text-destructive">{{ t('network.error_storage_error') }}</p>
    <p v-if="result" role="status" class="text-sm text-muted-foreground">
      {{ t('network.test_result', { ip: result.ip, ms: result.elapsed_ms }) }}
    </p>
    <p v-else-if="saved" role="status" class="text-sm text-muted-foreground">{{ t('network.saved') }}</p>
  </SettingsSectionCard>
</template>
