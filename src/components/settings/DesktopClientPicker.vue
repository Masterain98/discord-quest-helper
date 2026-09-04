<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderOpen, Loader2, RefreshCw, Trash2 } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { useQuestsStore } from '@/stores/quests'
import { desktopClientArgForProvider, useDesktopClientState } from '@/composables/desktopClientState'
import type { ClientInstallation, ClientSelection, DesktopClientState, ProviderId } from '@/api/tauri'

const emit = defineEmits<{ changed: [] }>()
const { t } = useI18n()
const questsStore = useQuestsStore()
const clients = useDesktopClientState()
const snapshot = computed(() => clients.state.value)

function selected(selection: ClientSelection): boolean {
  const current = snapshot.value?.selection
  if (!current || current.kind !== selection.kind) return false
  if (selection.kind === 'auto') return true
  if (selection.kind === 'provider' && current.kind === 'provider') {
    return selection.providerId === current.providerId && selection.variantId === current.variantId
  }
  return selection.kind === 'installation'
    && current.kind === 'installation'
    && selection.installationId === current.installationId
}

function isRunning(installation: ClientInstallation): boolean {
  return snapshot.value?.processes.some(process => process.installationId === installation.id) ?? false
}

function pathFor(installation: ClientInstallation): string {
  if (installation.launchTarget.kind === 'executable') return installation.launchTarget.path
  if (installation.launchTarget.kind === 'macBundle') return installation.launchTarget.bundlePath
  return installation.launchTarget.appId
}

function statusFor(installation: ClientInstallation): string {
  if (installation.validation === 'missing') return t('desktop_clients.status_missing')
  if (installation.validation === 'invalid') return t('desktop_clients.status_invalid')
  if (isRunning(installation)) return t('desktop_clients.status_running')
  return t('desktop_clients.status_launchable')
}

async function choose(selection: ClientSelection) {
  try {
    const next = await clients.select(selection, questsStore.cdpPort)
    syncLegacyPreference(next)
    emit('changed')
  } catch {
    // The composable keeps the localized backend error in its reactive state.
  }
}

function providerForSelection(snapshot: DesktopClientState): ProviderId | null {
  const selection = snapshot.selection
  if (selection.kind === 'provider') return selection.providerId
  if (selection.kind === 'installation') {
    return snapshot.installations.find(item => item.id === selection.installationId)?.providerId ?? null
  }
  return null
}

function syncLegacyPreference(snapshot: DesktopClientState | null | undefined = clients.state.value) {
  if (!snapshot || snapshot.port !== questsStore.cdpPort) return
  const provider = providerForSelection(snapshot)
  questsStore.desktopClient = provider
    ? desktopClientArgForProvider(provider)
    : 'auto'
}

async function browse(providerId: ProviderId = 'vencord.vesktop') {
  try {
    const path = await open({
      multiple: false,
      directory: false,
      title: t('desktop_clients.browse_title'),
      ...(/linux/i.test(navigator.userAgent)
        ? {}
        : { filters: [{ name: 'Desktop client', extensions: ['exe', 'AppImage', 'app'] }] }),
    })
    if (typeof path !== 'string') return
    const next = await clients.addInstallation(providerId, path, questsStore.cdpPort)
    syncLegacyPreference(next)
    emit('changed')
  } catch {
    // The composable keeps the localized backend error in its reactive state.
  }
}

async function remove(installation: ClientInstallation) {
  try {
    const next = await clients.removeInstallation(installation.id, questsStore.cdpPort)
    syncLegacyPreference(next)
    emit('changed')
  } catch {
    // The composable keeps the localized backend error in its reactive state.
  }
}

function handleRadioKey(event: KeyboardEvent) {
  if (!['ArrowDown', 'ArrowRight', 'ArrowUp', 'ArrowLeft'].includes(event.key)) return
  const group = event.currentTarget as HTMLElement
  const radios = Array.from(group.querySelectorAll<HTMLElement>('[role="radio"]:not([aria-disabled="true"])'))
  const index = radios.indexOf(document.activeElement as HTMLElement)
  if (index < 0 || radios.length < 2) return
  event.preventDefault()
  const direction = event.key === 'ArrowDown' || event.key === 'ArrowRight' ? 1 : -1
  radios[(index + direction + radios.length) % radios.length]?.focus()
}

onMounted(async () => {
  const refreshed = await clients.refresh(questsStore.cdpPort)
  await clients.migrateLegacySelection(questsStore.cdpPort, questsStore.desktopClient)
  syncLegacyPreference(refreshed?.port === questsStore.cdpPort ? clients.state.value : undefined)
})
</script>

<template>
  <div class="space-y-3">
    <div class="flex items-start justify-between gap-3">
      <div>
        <p class="text-sm font-medium">{{ t('settings.desktop_client') }}</p>
        <p class="mt-1 text-sm text-muted-foreground">{{ t('settings.desktop_client_desc') }}</p>
      </div>
      <Button variant="ghost" size="sm" :disabled="clients.loading.value" @click="clients.refresh(questsStore.cdpPort)">
        <Loader2 v-if="clients.loading.value" class="h-4 w-4 animate-spin" />
        <RefreshCw v-else class="h-4 w-4" />
        <span class="sr-only">{{ t('general.refresh') }}</span>
      </Button>
    </div>

    <div
      role="radiogroup"
      :aria-label="t('settings.desktop_client')"
      class="grid gap-2"
      @keydown="handleRadioKey"
    >
      <button
        type="button"
        role="radio"
        :aria-checked="selected({ kind: 'auto' })"
        :class="[
          'rounded-lg border px-4 py-3 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring',
          selected({ kind: 'auto' }) ? 'border-primary bg-primary/8' : 'hover:bg-muted/50',
        ]"
        @click="choose({ kind: 'auto' })"
      >
         <span class="block text-sm font-semibold">{{ t('desktop_clients.ask_each_time') }}</span>
         <span class="mt-1 block text-xs text-muted-foreground">{{ t('desktop_clients.ask_each_time_desc') }}</span>
      </button>

      <div
        v-for="installation in snapshot?.installations ?? []"
        :key="installation.id"
        :class="[
          'group flex min-w-0 items-start gap-2 rounded-lg border p-1 transition-colors',
          selected({ kind: 'installation', installationId: installation.id }) ? 'border-primary bg-primary/8' : 'hover:bg-muted/40',
          installation.validation !== 'valid' && 'border-amber-500/40',
        ]"
      >
        <button
          type="button"
          role="radio"
          class="min-w-0 flex-1 rounded-md px-3 py-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
          :aria-checked="selected({ kind: 'installation', installationId: installation.id })"
          @click="choose({ kind: 'installation', installationId: installation.id })"
        >
          <span class="flex flex-wrap items-center gap-2">
            <span class="text-sm font-semibold">{{ installation.displayName }}</span>
            <span class="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">{{ statusFor(installation) }}</span>
            <span v-if="snapshot?.endpoint.ownerProviderId === installation.providerId" class="rounded bg-emerald-500/10 px-1.5 py-0.5 text-[11px] text-emerald-700 dark:text-emerald-300">{{ t('desktop_clients.cdp_owner') }}</span>
          </span>
          <span class="mt-1 block truncate text-xs text-muted-foreground" :title="pathFor(installation)">{{ pathFor(installation) }}</span>
        </button>
        <Button
          v-if="installation.validation !== 'valid'"
          variant="ghost"
          size="sm"
          class="mt-1 shrink-0"
          @click="browse(installation.providerId)"
        >
          <FolderOpen class="h-4 w-4" />
          <span class="sr-only">{{ t('desktop_clients.relocate') }}</span>
        </Button>
        <Button
          v-if="installation.source === 'user'"
          variant="ghost"
          size="sm"
          class="mt-1 shrink-0 text-muted-foreground hover:text-destructive"
          @click="remove(installation)"
        >
          <Trash2 class="h-4 w-4" />
          <span class="sr-only">{{ t('desktop_clients.remove') }}</span>
        </Button>
      </div>
    </div>

    <div class="flex flex-wrap gap-2">
      <Button variant="outline" size="sm" class="gap-2" @click="browse('vencord.vesktop')">
        <FolderOpen class="h-4 w-4" />
        {{ t('desktop_clients.add_vesktop') }}
      </Button>
    </div>
    <p v-if="clients.error.value" role="alert" class="text-sm text-destructive">{{ clients.error.value }}</p>
    <p v-for="issue in snapshot?.discoveryIssues ?? []" :key="`${issue.code}:${issue.message}`" class="text-xs text-amber-700 dark:text-amber-300">
      {{ issue.message }}
    </p>
  </div>
</template>
