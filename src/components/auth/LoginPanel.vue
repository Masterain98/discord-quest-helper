<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  AlertCircle,
  AppWindow,
  Check,
  ChevronDown,
  ChevronRight,
  HardDriveDownload,
  KeyRound,
  Loader2,
  Monitor,
  RadioTower,
  RotateCcw,
} from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import {
  listRunningDesktopCdpSessions,
  launchDesktopClientCdp,
  type AuthProgress,
  type ClientSelection,
  type CdpStatus,
  type DesktopClientInventory,
  type DesktopClientState,
  type ExtractedAccount,
} from '@/api/tauri'
import { desktopClientArgForProvider, useDesktopClientState } from '@/composables/desktopClientState'
import {
  classifyCdpAvailability,
  canBeginLogin,
  installedCdpLaunchTargets,
  presentAuthProgress,
  shouldAskCdpLaunchTarget,
  shouldPollCdp,
  selectionForCdpLaunchTarget,
  selectionForCurrentCdpOwner,
  startCdpPolling,
  usesVesktopForCdpLogin,
  hasUnchanneledOfficialMacInstallation,
  type CdpLaunchTarget,
  type LoginMethod,
  type LoginProgressState,
} from './loginFlow'

const CDP_POLL_INTERVAL_MS = 5_000

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const clients = useDesktopClientState()

const manualExpanded = ref(false)
const manualTokenInput = ref('')
const selectedAccountId = ref<string | null>(null)
const activeMethod = ref<LoginMethod | null>(null)
const progress = ref<{
  method: LoginMethod
  state: LoginProgressState
  key: string
  params?: Record<string, number>
  detail?: string
} | null>(null)

const cdpStatus = ref<CdpStatus | null>(null)
const cdpChecking = ref(false)
const cdpProbeFailed = ref(false)
const cdpRestartDialogOpen = ref(false)
const cdpChooseDialogOpen = ref(false)
const cdpLaunchChoices = ref<CdpLaunchTarget[]>([])
const selectedCdpTarget = ref<CdpLaunchTarget | null>(null)
const rememberCdpChoice = ref(false)
const desktopClients = ref<DesktopClientInventory | null>(null)
const ownerConflict = ref(false)
let stopCdpPolling: (() => void) | null = null
let accountViewTransitionRevision = 0

const showAutoDetect = computed(() => {
  if (!questsStore.platformCapabilitiesReady) return false
  const level = questsStore.platformCapabilities?.tokenAutoDetection
  return level !== 'manual_only' && level !== 'unavailable'
})

const busy = computed(() => (
  activeMethod.value !== null
  || authStore.loading
  || cdpRestartDialogOpen.value
  || cdpChooseDialogOpen.value
))
const showingDetectedAccounts = computed(() => authStore.detectedAccounts.length > 0)
const cdpAvailability = computed(() => classifyCdpAvailability(
  cdpChecking.value,
  cdpStatus.value,
  cdpProbeFailed.value,
))
const cdpStatusKey = computed(() => ({
  checking: 'settings.cdp_checking',
  ready: 'settings.cdp_connected',
  starting: 'auth.cdp_status_starting',
  offline: 'settings.cdp_disconnected_short',
  error: 'auth.cdp_status_error',
})[cdpAvailability.value])
const usingVesktopCdp = computed(() => (
  usesVesktopForCdpLogin(desktopClients.value)
))
const restartUsesVesktop = computed(() => (
  selectedCdpTarget.value === 'vesktop'
  || usingVesktopCdp.value
))
const cdpLoginDetailKey = computed(() => (
  usingVesktopCdp.value ? 'auth.cdp_login_detail_vesktop' : 'auth.cdp_login_detail'
))
const cdpLoginActionKey = computed(() => (
  usingVesktopCdp.value ? 'auth.cdp_login_action_vesktop' : 'auth.cdp_login_action'
))
const cdpStatusClass = computed(() => ({
  checking: 'bg-muted text-muted-foreground',
  ready: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
  starting: 'bg-amber-500/10 text-amber-700 dark:text-amber-300',
  offline: 'bg-muted text-muted-foreground',
  error: 'bg-destructive/10 text-destructive',
})[cdpAvailability.value])
const progressText = computed(() => {
  if (!progress.value) return ''
  const message = t(progress.value.key, progress.value.params ?? {})
  return progress.value.detail ? `${message}: ${progress.value.detail}` : message
})

async function transitionLoginCard(update: () => void) {
  if (
    authStore.user
    || !document.startViewTransition
    || window.matchMedia('(prefers-reduced-motion: reduce)').matches
  ) {
    update()
    return
  }

  const revision = ++accountViewTransitionRevision
  document.documentElement.classList.add('account-view-transition')
  const transition = document.startViewTransition(async () => {
    update()
    await nextTick()
  })

  try {
    await transition.finished
  } catch {
    // A concurrent view transition may skip this animation; the state update
    // has still completed inside the transition callback.
  } finally {
    if (revision === accountViewTransitionRevision) {
      document.documentElement.classList.remove('account-view-transition')
    }
  }
}

async function showDetectedAccounts(accounts: ExtractedAccount[]) {
  await transitionLoginCard(() => {
    authStore.detectedAccounts = accounts
  })
}

function setProgress(
  method: LoginMethod,
  state: LoginProgressState,
  key: string,
  params?: Record<string, number>,
  detail?: string,
) {
  progress.value = { method, state, key, params, detail }
}

function handleBackendProgress(method: LoginMethod, event: AuthProgress) {
  const presentation = presentAuthProgress(event)
  setProgress(method, presentation.state, presentation.key, presentation.params)
}

function errorDetail(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error)
}

function begin(method: LoginMethod): boolean {
  if (!canBeginLogin(activeMethod.value, authStore.loading)) return false
  activeMethod.value = method
  authStore.error = null
  return true
}

function finish() {
  activeMethod.value = null
}

async function refreshDesktopClients() {
  const snapshot = await clients.refresh(questsStore.cdpPort)
  if (snapshot) desktopClients.value = inventoryFromState(snapshot)
}

async function refreshCdpStatus(): Promise<CdpStatus | null> {
  if (cdpChecking.value) return cdpStatus.value
  cdpChecking.value = true
  cdpProbeFailed.value = false
  try {
    const snapshot = await clients.refresh(questsStore.cdpPort)
    if (!snapshot) throw new Error(clients.error.value ?? 'Desktop client state is unavailable')
    desktopClients.value = inventoryFromState(snapshot)
    const ready = snapshot.endpoint.status === 'discordReady'
    const status: CdpStatus = {
      available: ready,
      connected: ready,
      target_title: snapshot.endpoint.targetTitle,
      error: ready ? null : snapshot.endpoint.status,
    }
    cdpStatus.value = status
    questsStore.cdpAvailable = status.connected
    return status
  } catch (error) {
    cdpProbeFailed.value = true
    cdpStatus.value = null
    questsStore.cdpAvailable = false
    console.warn('Login page CDP probe failed:', error)
    return null
  } finally {
    cdpChecking.value = false
  }
}

function inventoryFromState(snapshot: DesktopClientState): DesktopClientInventory {
  const installed = (providerId: string, variantId?: string) => snapshot.installations.some(item => (
    item.providerId === providerId
    && item.validation === 'valid'
    && (!variantId || item.variantId === variantId)
  ))
  const running = (providerId: string, variantId?: string) => snapshot.processes.some(item => (
    item.providerId === providerId && (!variantId || item.variantId === variantId)
  ))
  const customOfficialMacInstalled = hasUnchanneledOfficialMacInstallation(snapshot.installations)
  return {
    officialInstalled: installed('discord.official'),
    vesktopInstalled: installed('vencord.vesktop'),
    officialRunning: running('discord.official'),
    vesktopRunning: running('vencord.vesktop'),
    cdpOwner: snapshot.endpoint.owner,
    stableInstalled: installed('discord.official', 'stable') || customOfficialMacInstalled,
    ptbInstalled: installed('discord.official', 'ptb'),
    canaryInstalled: installed('discord.official', 'canary'),
    stableRunning: running('discord.official', 'stable'),
    ptbRunning: running('discord.official', 'ptb'),
    canaryRunning: running('discord.official', 'canary'),
  }
}

function selectionForTarget(target: CdpLaunchTarget | null): ClientSelection {
  return selectionForCdpLaunchTarget(clients.state.value, target)
}

function selectionIsRunning(snapshot: DesktopClientState, selection: ClientSelection): boolean {
  if (selection.kind === 'installation') {
    return snapshot.processes.some(process => process.installationId === selection.installationId)
  }
  if (selection.kind === 'provider') {
    return snapshot.processes.some(process => (
      process.providerId === selection.providerId
      && (!selection.variantId || process.variantId === selection.variantId)
    ))
  }
  return snapshot.processes.length > 0
}

function selectionProvider(snapshot: DesktopClientState, selection: ClientSelection): string | null {
  if (selection.kind === 'provider') return selection.providerId
  if (selection.kind === 'installation') {
    return snapshot.installations.find(item => item.id === selection.installationId)?.providerId ?? null
  }
  return null
}

function syncLegacyDesktopClientPreference(selection: ClientSelection) {
  if (selection.kind !== 'provider') {
    questsStore.desktopClient = 'auto'
    return
  }
  questsStore.desktopClient = desktopClientArgForProvider(selection.providerId)
}

async function handleAutoDetect() {
  if (!begin('local')) return
  setProgress('local', 'running', 'auth.progress.extracting_tokens')
  try {
    if (showingDetectedAccounts.value) {
      await showDetectedAccounts([])
    }
    const succeeded = await authStore.tryAutoDetect(
      event => handleBackendProgress('local', event),
      showDetectedAccounts,
    )
    if (!succeeded) {
      setProgress('local', 'error', 'auth.progress.failed', undefined, authStore.error ?? undefined)
    }
  } finally {
    finish()
  }
}

async function selectAccount(account: ExtractedAccount) {
  if (!begin('local')) return
  selectedAccountId.value = account.user.id
  setProgress('local', 'running', 'auth.progress.validating_token')
  try {
    const succeeded = await authStore.loginWithToken(
      account.token,
      event => handleBackendProgress('local', event),
    )
    if (succeeded) {
      authStore.detectedAccounts = []
    } else {
      setProgress('local', 'error', 'auth.progress.failed', undefined, authStore.error ?? undefined)
    }
  } finally {
    selectedAccountId.value = null
    finish()
  }
}

async function resetDetectedAccounts() {
  if (busy.value) return
  await transitionLoginCard(() => {
    authStore.detectedAccounts = []
    progress.value = null
  })
}

async function handleManualLogin() {
  if (!manualTokenInput.value || !begin('manual')) return
  const token = manualTokenInput.value
  setProgress('manual', 'running', 'auth.progress.validating_token')
  try {
    const succeeded = await authStore.loginWithToken(
      token,
      event => handleBackendProgress('manual', event),
    )
    if (!succeeded) {
      setProgress('manual', 'error', 'auth.progress.failed', undefined, authStore.error ?? undefined)
    }
  } finally {
    manualTokenInput.value = ''
    finish()
  }
}

async function finishCdpLogin() {
  const succeeded = await authStore.loginViaCdp(event => handleBackendProgress('cdp', event))
  if (!succeeded) {
    setProgress('cdp', 'error', 'auth.progress.failed', undefined, authStore.error ?? undefined)
  }
  return succeeded
}

function requestCdpRestart(target: CdpLaunchTarget | null) {
  selectedCdpTarget.value = target
  setProgress(
    'cdp',
    'waiting',
    target === 'vesktop' ? 'auth.progress.restart_required_vesktop' : 'auth.progress.restart_required',
  )
  cdpRestartDialogOpen.value = true
}

async function launchOrRestartSelectedTarget(target: CdpLaunchTarget | null) {
  selectedCdpTarget.value = target
  const snapshot = await clients.refresh(questsStore.cdpPort)
  if (!snapshot) throw new Error(clients.error.value ?? 'Desktop client state is unavailable')
  const selection = selectionForTarget(target)
  if (selectionIsRunning(snapshot, selection)) {
    requestCdpRestart(target)
    return
  }

  setProgress(
    'cdp',
    'running',
    target === 'vesktop' ? 'auth.progress.launching_vesktop' : 'auth.progress.launching_discord',
  )
  try {
    await launchDesktopClientCdp(questsStore.cdpPort, selection, false)
    await refreshCdpStatus()
  } catch (launchError) {
    const latest = await clients.refresh(questsStore.cdpPort)
    if (!latest) throw launchError
    if (latest.endpoint.status !== 'discordReady') {
      if (selectionIsRunning(latest, selection)) {
        requestCdpRestart(target)
        return
      }
      throw launchError
    }
    const provider = selectionProvider(latest, selection)
    if (provider && latest.endpoint.ownerProviderId !== provider) {
      ownerConflict.value = true
      requestCdpRestart(target)
      return
    }
  }
  await finishCdpLogin()
}

async function handleCdpLogin() {
  if (!begin('cdp')) return
  setProgress('cdp', 'running', 'auth.progress.checking_cdp')
  try {
    const status = await refreshCdpStatus()
    const snapshot = clients.state.value
    if (status?.connected && snapshot) {
      const provider = selectionProvider(snapshot, snapshot.selection)
      if (provider && snapshot.endpoint.ownerProviderId !== provider) {
        ownerConflict.value = true
        selectedCdpTarget.value = null
        setProgress('cdp', 'waiting', 'auth.progress.choose_client')
        cdpRestartDialogOpen.value = true
        return
      }
      await finishCdpLogin()
      return
    }
    if (snapshot?.selection.kind !== 'auto') {
      await launchOrRestartSelectedTarget(null)
      return
    }
    const targets = installedCdpLaunchTargets(desktopClients.value)
    if (shouldAskCdpLaunchTarget(false, targets)) {
      cdpLaunchChoices.value = targets
      selectedCdpTarget.value = null
      rememberCdpChoice.value = false
      setProgress('cdp', 'waiting', 'auth.progress.choose_client')
      cdpChooseDialogOpen.value = true
      return
    }
    await launchOrRestartSelectedTarget(targets[0] ?? null)
  } catch (error) {
    authStore.error = errorDetail(error)
    setProgress('cdp', 'error', 'auth.progress.failed', undefined, authStore.error)
  } finally {
    finish()
    if (!authStore.user && !cdpRestartDialogOpen.value && !cdpChooseDialogOpen.value) {
      void refreshCdpStatus()
    }
  }
}

async function selectCdpLaunchTarget(target: CdpLaunchTarget) {
  const shouldRemember = rememberCdpChoice.value
  setProgress(
    'cdp',
    'running',
    target === 'vesktop' ? 'auth.progress.launching_vesktop' : 'auth.progress.launching_discord',
  )
  cdpChooseDialogOpen.value = false
  if (!begin('cdp')) return
  try {
    const selected = selectionForTarget(target)
    const persisted = shouldRemember ? selected : { kind: 'auto' as const }
    await clients.select(persisted, questsStore.cdpPort)
    syncLegacyDesktopClientPreference(persisted)
    await launchOrRestartSelectedTarget(target)
  } catch (error) {
    authStore.error = errorDetail(error)
    setProgress('cdp', 'error', 'auth.progress.failed', undefined, authStore.error)
  } finally {
    finish()
    if (!authStore.user && !cdpRestartDialogOpen.value) void refreshCdpStatus()
  }
}

async function confirmCdpRestart() {
  cdpRestartDialogOpen.value = false
  if (!begin('cdp')) return
  setProgress(
    'cdp',
    'running',
    selectedCdpTarget.value === 'vesktop'
      ? 'auth.progress.restarting_vesktop'
      : 'auth.progress.restarting_discord',
  )
  try {
    const snapshot = await clients.refresh(questsStore.cdpPort)
    if (!snapshot) throw new Error(clients.error.value ?? 'Desktop client state is unavailable')
    await launchDesktopClientCdp(
      questsStore.cdpPort,
      selectionForTarget(selectedCdpTarget.value),
      true,
    )
    ownerConflict.value = false
    await refreshCdpStatus()
    await finishCdpLogin()
  } catch (error) {
    authStore.error = errorDetail(error)
    setProgress('cdp', 'error', 'auth.progress.failed', undefined, authStore.error)
  } finally {
    finish()
    if (!authStore.user) void refreshCdpStatus()
  }
}

async function useCurrentCdpOwner() {
  try {
    const snapshot = await clients.refresh(questsStore.cdpPort)
    const providerId = snapshot?.endpoint.ownerProviderId
    if (!snapshot || !providerId) return
    const ownerSession = (await listRunningDesktopCdpSessions()).find(session => (
      session.port === questsStore.cdpPort && session.providerId === providerId
    ))
    if (!ownerSession) throw new Error('The current CDP owner could not be mapped to one exact installation.')
    const selection = selectionForCurrentCdpOwner(snapshot, ownerSession)
    await clients.select(selection, questsStore.cdpPort)
    syncLegacyDesktopClientPreference(selection)
    ownerConflict.value = false
    cdpRestartDialogOpen.value = false
    if (!begin('cdp')) return
    await finishCdpLogin()
  } catch (error) {
    authStore.error = errorDetail(error)
    setProgress('cdp', 'error', 'auth.progress.failed', undefined, authStore.error)
  } finally {
    finish()
  }
}

function handleRestartDialogOpenChange(open: boolean) {
  cdpRestartDialogOpen.value = open
  if (!open && progress.value?.state === 'waiting') {
    setProgress('cdp', 'neutral', 'auth.progress.restart_cancelled')
    void refreshCdpStatus()
  }
}

function handleChooseDialogOpenChange(open: boolean) {
  cdpChooseDialogOpen.value = open
  if (open) {
    rememberCdpChoice.value = false
    void refreshDesktopClients()
  }
  if (!open && progress.value?.key === 'auth.progress.choose_client') {
    setProgress('cdp', 'neutral', 'auth.progress.launch_cancelled')
    void refreshCdpStatus()
  }
}

function pollCdpIfNeeded() {
  if (shouldPollCdp({
    busy: busy.value,
    authenticated: Boolean(authStore.user),
    visible: document.visibilityState === 'visible',
  })) {
    void refreshCdpStatus()
  }
}

function handleVisibilityChange() {
  if (document.visibilityState === 'visible') pollCdpIfNeeded()
}

onMounted(() => {
  pollCdpIfNeeded()
  void refreshDesktopClients()
  void clients.migrateLegacySelection(questsStore.cdpPort, questsStore.desktopClient)
  stopCdpPolling = startCdpPolling(pollCdpIfNeeded, CDP_POLL_INTERVAL_MS)
  document.addEventListener('visibilitychange', handleVisibilityChange)
})

onUnmounted(() => {
  accountViewTransitionRevision += 1
  document.documentElement.classList.remove('account-view-transition')
  stopCdpPolling?.()
  document.removeEventListener('visibilitychange', handleVisibilityChange)
})

watch(() => questsStore.cdpPort, () => {
  void refreshCdpStatus()
})
</script>

<template>
  <section
    class="login-panel-stage mx-auto my-auto flex w-full max-w-2xl flex-col gap-6 py-4 sm:py-8"
    aria-labelledby="login-heading"
  >
    <div class="flex justify-center">
      <slot name="toolbar" />
    </div>

    <div class="login-brand-stage flex justify-center py-2 sm:py-4">
      <div class="flex items-center gap-3 sm:gap-4">
        <img src="/icons/logo.png" :alt="t('general.title')" class="h-12 w-12 select-none sm:h-14 sm:w-14" />
        <div>
          <h2 id="login-heading" class="text-2xl font-semibold tracking-tight sm:text-3xl">
            {{ t('general.welcome') }}
          </h2>
          <p class="mt-1 max-w-xl text-sm leading-6 text-muted-foreground sm:text-base">
            {{ t('general.login_prompt') }}
          </p>
        </div>
      </div>
    </div>

    <div class="login-card-shell overflow-hidden rounded-xl border bg-card/60 shadow-[0_16px_50px_-32px_hsl(var(--primary)/0.45)]">
      <div class="login-card-content">
      <template v-if="showingDetectedAccounts">
        <div class="flex items-start justify-between gap-4 border-b px-5 py-5 sm:px-6">
          <div>
            <h3 class="font-semibold">{{ t('account.select_desc') }}</h3>
            <p class="mt-1 text-sm text-muted-foreground">
              {{ t('auth.progress.accounts_found', { count: authStore.detectedAccounts.length }) }}
            </p>
          </div>
          <Button variant="ghost" size="sm" :disabled="busy" @click="resetDetectedAccounts">
            {{ t('auth.back_to_methods') }}
          </Button>
        </div>

        <div class="max-h-72 space-y-2 overflow-y-auto p-4 sm:p-5">
          <Button
            v-for="account in authStore.detectedAccounts"
            :key="account.user.id"
            variant="outline"
            class="h-auto w-full justify-start rounded-lg px-4 py-3 text-left transition-transform active:translate-y-px"
            :disabled="busy"
            @click="selectAccount(account)"
          >
            <Loader2 v-if="selectedAccountId === account.user.id" class="mr-3 h-5 w-5 shrink-0 animate-spin" />
            <img
              v-else-if="account.user.avatar"
              :src="`https://cdn.discordapp.com/avatars/${account.user.id}/${account.user.avatar}.png`"
              :alt="account.user.username"
              class="mr-3 h-9 w-9 shrink-0 rounded-lg"
            />
            <div v-else class="mr-3 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-sm font-semibold text-primary">
              {{ (account.user.global_name || account.user.username).slice(0, 1).toUpperCase() }}
            </div>
            <span class="min-w-0">
              <span class="block truncate font-semibold">{{ account.user.global_name || account.user.username }}</span>
              <span class="block truncate text-xs text-muted-foreground">
                @{{ account.user.username }}
              </span>
            </span>
          </Button>
        </div>

        <div class="border-t px-5 py-4 sm:px-6">
          <Button variant="ghost" size="sm" class="gap-2" :disabled="busy" @click="handleAutoDetect">
            <RotateCcw class="h-4 w-4" />
            {{ t('auth.rescan') }}
          </Button>
        </div>
      </template>

      <template v-else>
        <div class="login-methods">
          <article v-if="showAutoDetect" class="login-method bg-primary/[0.045] px-5 py-5 sm:px-6 sm:py-6">
            <div class="login-method-copy flex min-w-0 items-start gap-4">
              <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-primary/12 text-primary">
                <HardDriveDownload class="h-5 w-5" />
              </div>
              <div class="min-w-0">
                <h3 class="font-semibold">{{ t('auth.auto_detect') }}</h3>
                <p class="mt-1 max-w-md text-sm leading-5 text-muted-foreground">{{ t('auth.local_login_desc') }}</p>
              </div>
            </div>
            <div class="login-method-action">
              <Button size="lg" class="login-method-button gap-2" :disabled="busy" @click="handleAutoDetect">
                <Loader2 v-if="activeMethod === 'local'" class="h-4 w-4 shrink-0 animate-spin" />
                {{ t('auth.extract_token') }}
              </Button>
            </div>
          </article>

          <article :class="['login-method px-5 py-5 sm:px-6', showAutoDetect && 'border-t']">
            <div class="login-method-copy flex min-w-0 items-start gap-4">
              <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                <RadioTower class="h-5 w-5" />
              </div>
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <h3 class="font-semibold">{{ t('auth.cdp_login') }}</h3>
                  <span :class="['inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium', cdpStatusClass]">
                    <Loader2 v-if="cdpAvailability === 'checking'" class="h-3 w-3 animate-spin" />
                    <span v-else class="h-1.5 w-1.5 rounded-full bg-current opacity-80" />
                    {{ t(cdpStatusKey) }}
                  </span>
                </div>
                <p class="mt-1 max-w-md text-sm leading-5 text-muted-foreground">{{ t(cdpLoginDetailKey) }}</p>
              </div>
            </div>
            <div class="login-method-action">
              <Button
                size="lg"
                :variant="showAutoDetect ? 'outline' : 'default'"
                class="login-method-button gap-2"
                :disabled="busy"
                @click="handleCdpLogin"
              >
                <Loader2 v-if="activeMethod === 'cdp'" class="h-4 w-4 shrink-0 animate-spin" />
                {{ t(cdpLoginActionKey) }}
              </Button>
            </div>
          </article>
        </div>

        <article :class="['border-t px-5 py-5 transition-colors duration-300 sm:px-6', manualExpanded && 'bg-muted/20']">
          <button
            type="button"
            class="flex w-full items-center justify-between gap-4 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
            :aria-expanded="manualExpanded"
            aria-controls="manual-token-form"
            :disabled="busy"
            @click="manualExpanded = !manualExpanded"
          >
            <span class="flex min-w-0 items-start gap-4">
              <span class="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                <KeyRound class="h-5 w-5" />
              </span>
              <span class="min-w-0">
                <span class="block font-semibold">{{ t('settings.advanced_login_method') }}</span>
                <span class="mt-1 block max-w-md text-sm leading-5 text-muted-foreground">{{ t('settings.advanced_login_desc') }}</span>
              </span>
            </span>
            <ChevronDown
              :class="['h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-300 ease-out', manualExpanded && 'rotate-180']"
            />
          </button>

          <Transition name="manual-disclosure">
            <div v-if="manualExpanded" id="manual-token-form" class="manual-disclosure-grid">
              <div class="min-h-0">
                <form class="space-y-3 pt-4 pl-0 sm:pl-[3.75rem]" @submit.prevent="handleManualLogin">
                  <div class="flex flex-col gap-2 sm:flex-row">
                    <Input
                      v-model="manualTokenInput"
                      type="password"
                      autocomplete="off"
                      :placeholder="t('auth.enter_token')"
                      class="flex-1"
                      :disabled="busy"
                    />
                    <Button type="submit" class="gap-2 sm:min-w-24" :disabled="!manualTokenInput || busy">
                      <Loader2 v-if="activeMethod === 'manual'" class="h-4 w-4 animate-spin" />
                      {{ t('auth.login') }}
                    </Button>
                  </div>
                  <p class="text-xs leading-5 text-muted-foreground">{{ t('settings.token_storage_note') }}</p>
                </form>
              </div>
            </div>
          </Transition>
        </article>
      </template>
      </div>

      <Transition name="progress-status">
        <div v-if="progress" class="progress-status-grid">
          <div
            :role="progress.state === 'error' ? 'alert' : 'status'"
            aria-live="polite"
            aria-atomic="true"
            :class="[
              'progress-status-row flex items-start gap-2.5 border-t px-5 py-3 text-sm sm:px-6',
              progress.state === 'error' && 'bg-destructive/5 text-destructive',
              progress.state === 'success' && 'bg-emerald-500/5 text-emerald-700 dark:text-emerald-300',
              (progress.state === 'running' || progress.state === 'waiting' || progress.state === 'neutral') && 'text-muted-foreground',
            ]"
          >
            <Loader2 v-if="progress.state === 'running'" class="mt-0.5 h-4 w-4 shrink-0 animate-spin text-primary" />
            <Check v-else-if="progress.state === 'success'" class="mt-0.5 h-4 w-4 shrink-0" />
            <AlertCircle v-else-if="progress.state === 'error'" class="mt-0.5 h-4 w-4 shrink-0" />
            <RadioTower v-else class="mt-0.5 h-4 w-4 shrink-0" />
            <span class="min-w-0 break-words">{{ progressText }}</span>
          </div>
        </div>
      </Transition>
    </div>

    <AlertDialog :open="cdpChooseDialogOpen" @update:open="handleChooseDialogOpenChange">
      <AlertDialogContent class="client-picker-dialog max-w-[560px] gap-0 overflow-hidden border-border/70 bg-background/95 p-0 shadow-[0_24px_80px_-32px_hsl(var(--primary)/0.45)] backdrop-blur-xl">
        <div class="border-b border-border/60 bg-primary/[0.045]">
          <AlertDialogHeader class="px-6 pb-6 pt-6 sm:px-8 sm:pb-7 sm:pt-8">
            <div class="flex items-start justify-between gap-4">
              <div class="flex min-w-0 items-start gap-3.5">
                <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-primary text-primary-foreground shadow-[0_10px_24px_-12px_hsl(var(--primary))]">
                  <RadioTower class="h-5 w-5" :stroke-width="1.8" />
                </div>
                <div class="min-w-0">
                  <AlertDialogTitle class="text-xl font-semibold tracking-[-0.02em] sm:text-[1.35rem]">
                    {{ t('auth.cdp_choose_title') }}
                  </AlertDialogTitle>
                  <AlertDialogDescription class="mt-2 max-w-[38rem] text-sm leading-6 text-muted-foreground sm:text-[0.95rem]">
                    {{ t('auth.cdp_choose_desc') }}
                  </AlertDialogDescription>
                </div>
              </div>
              <span class="hidden shrink-0 items-center gap-2 rounded-full border border-border/70 bg-background/70 px-2.5 py-1.5 text-[11px] font-semibold tracking-[0.04em] text-muted-foreground sm:inline-flex">
                <span class="h-1.5 w-1.5 rounded-full bg-muted-foreground/60" aria-hidden="true" />
                {{ t('settings.cdp_disconnected_short') }}
              </span>
            </div>
          </AlertDialogHeader>
        </div>

        <div class="space-y-4 px-6 py-5 sm:px-8 sm:py-6">
          <label class="group flex cursor-pointer items-center gap-3 rounded-xl border border-border/70 bg-card/60 px-3.5 py-3 transition-colors duration-200 hover:border-primary/35 hover:bg-primary/[0.035]">
            <input
              v-model="rememberCdpChoice"
              type="checkbox"
              class="peer sr-only"
            />
            <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded-[6px] border border-muted-foreground/35 bg-background text-transparent transition-all duration-200 peer-checked:border-primary peer-checked:bg-primary peer-checked:text-primary-foreground peer-focus-visible:ring-2 peer-focus-visible:ring-primary/40 peer-focus-visible:ring-offset-2">
              <Check class="h-3.5 w-3.5" :stroke-width="3" aria-hidden="true" />
            </span>
            <span class="text-sm font-medium text-foreground">{{ t('auth.cdp_remember_choice') }}</span>
          </label>

          <div class="grid gap-2.5">
            <Button
              v-for="target in cdpLaunchChoices"
              :key="target"
              type="button"
              variant="outline"
              class="group flex h-auto min-h-[72px] w-full items-center justify-between rounded-xl border-border/70 bg-card/70 px-4 py-3.5 text-left shadow-sm transition-all duration-200 hover:-translate-y-0.5 hover:border-primary/45 hover:bg-primary/[0.045] hover:shadow-[0_12px_28px_-20px_hsl(var(--primary)/0.7)] active:translate-y-0 focus-visible:ring-2 focus-visible:ring-primary/40 focus-visible:ring-offset-2"
              @click="selectCdpLaunchTarget(target)"
            >
              <span class="flex min-w-0 items-center gap-3.5">
                <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary transition-colors duration-200 group-hover:bg-primary group-hover:text-primary-foreground">
                  <component
                    :is="target === 'vesktop' ? AppWindow : Monitor"
                    class="h-[18px] w-[18px]"
                    :stroke-width="1.9"
                    aria-hidden="true"
                  />
                </span>
                <span class="min-w-0 truncate text-[15px] font-semibold tracking-[-0.01em] text-foreground">
                  {{ t(`auth.cdp_client_${target}`) }}
                </span>
              </span>
              <ChevronRight class="h-4 w-4 shrink-0 text-muted-foreground/60 transition-transform duration-200 group-hover:translate-x-0.5 group-hover:text-primary" aria-hidden="true" />
            </Button>
          </div>
        </div>

        <AlertDialogFooter class="border-t border-border/60 bg-muted/20 px-6 py-4 sm:px-8">
          <AlertDialogCancel class="mt-0 rounded-lg border-transparent bg-transparent px-3 text-muted-foreground hover:bg-background hover:text-foreground sm:mt-0">
            {{ t('dialog.cancel') }}
          </AlertDialogCancel>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <AlertDialog :open="cdpRestartDialogOpen" @update:open="handleRestartDialogOpenChange">
      <AlertDialogContent class="max-w-[520px]">
        <AlertDialogHeader>
          <AlertDialogTitle>{{
            ownerConflict
              ? t('desktop_clients.owner_conflict_title')
              : restartUsesVesktop
              ? t('settings.cdp_dialog_title_disconnected_vesktop')
              : t('settings.cdp_dialog_title_disconnected')
          }}</AlertDialogTitle>
          <AlertDialogDescription>{{
            ownerConflict
              ? t('desktop_clients.owner_conflict_desc')
              : restartUsesVesktop
              ? t('settings.cdp_dialog_desc_disconnected_vesktop')
              : t('settings.cdp_dialog_desc_disconnected')
          }}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <Button v-if="ownerConflict" variant="outline" @click="useCurrentCdpOwner">
            {{ t('desktop_clients.use_current') }}
          </Button>
          <AlertDialogAction @click="confirmCdpRestart">
            {{ ownerConflict ? t('desktop_clients.switch_selected') : t('settings.cdp_dialog_confirm') }}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </section>
</template>

<style scoped>
.login-brand-stage {
  view-transition-name: app-brand;
}

.login-methods {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
}

.login-method {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 1rem;
}

.login-method-action,
.login-method-button {
  width: 100%;
}

.login-method-button {
  min-height: 2.75rem;
  height: auto;
  white-space: normal;
  line-height: 1.25;
  text-align: center;
}

.manual-disclosure-grid {
  display: grid;
  grid-template-rows: 1fr;
}

.progress-status-grid {
  display: grid;
  grid-template-rows: 1fr;
}

.progress-status-row {
  min-height: 3rem;
}

.progress-status-enter-active,
.progress-status-leave-active {
  overflow: hidden;
  transform-origin: top;
  transition:
    grid-template-rows 320ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity 220ms ease,
    transform 320ms cubic-bezier(0.22, 1, 0.36, 1);
}

.progress-status-enter-from,
.progress-status-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
  transform: translateY(-0.375rem) scaleY(0.97);
}

.progress-status-enter-from .progress-status-row,
.progress-status-leave-to .progress-status-row {
  min-height: 0;
}

.manual-disclosure-enter-active,
.manual-disclosure-leave-active {
  overflow: hidden;
  transform-origin: top;
  transition:
    grid-template-rows 360ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity 240ms ease,
    transform 360ms cubic-bezier(0.22, 1, 0.36, 1);
}

.manual-disclosure-enter-from,
.manual-disclosure-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
  transform: translateY(-0.375rem) scaleY(0.98);
}

@media (min-width: 640px) {
  .login-methods {
    grid-template-columns: minmax(0, 1fr) max-content;
  }

  .login-method {
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    align-items: center;
  }

  .login-method-copy {
    grid-column: 1;
  }

  .login-method-action {
    grid-column: 2;
    max-width: 18rem;
  }

  .login-method-button {
    min-width: 9rem;
  }
}

@media (prefers-reduced-motion: reduce) {
  .manual-disclosure-enter-active,
  .manual-disclosure-leave-active,
  .progress-status-enter-active,
  .progress-status-leave-active {
    transition-duration: 1ms;
  }
}
</style>
