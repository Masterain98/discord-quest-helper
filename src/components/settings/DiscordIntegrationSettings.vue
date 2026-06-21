<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Check, Link2, Loader2, Wifi, WifiOff } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import { checkCdpStatus, createDiscordDebugShortcut, fetchSuperPropertiesCdp, type CdpStatus } from '@/api/tauri'
import AdvancedDisclosure from './AdvancedDisclosure.vue'

const { t } = useI18n()
const questsStore = useQuestsStore()

const cdpStatus = ref<CdpStatus | null>(null)
const cdpChecking = ref(false)
const cdpFetching = ref(false)
const cdpFetchSuccess = ref(false)
const cdpFetchError = ref('')
const shortcutCreating = ref(false)
const shortcutSuccess = ref(false)
const shortcutError = ref('')

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

onMounted(checkCdp)
</script>

<template>
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
