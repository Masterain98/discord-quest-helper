<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  AlertCircle,
  Check,
  ChevronDown,
  HardDriveDownload,
  KeyRound,
  Loader2,
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
  checkCdpStatus,
  isDiscordRunning,
  launchDiscordCdp,
  restartDiscordCdp,
  type AuthProgress,
  type CdpStatus,
  type ExtractedAccount,
} from '@/api/tauri'
import {
  classifyCdpAvailability,
  canBeginLogin,
  presentAuthProgress,
  shouldPollCdp,
  startCdpPolling,
  type LoginMethod,
  type LoginProgressState,
} from './loginFlow'

const CDP_POLL_INTERVAL_MS = 5_000

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()

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
let stopCdpPolling: (() => void) | null = null
let accountViewTransitionRevision = 0

const showAutoDetect = computed(() => {
  if (!questsStore.platformCapabilitiesReady) return false
  const level = questsStore.platformCapabilities?.tokenAutoDetection
  return level !== 'manual_only' && level !== 'unavailable'
})

const busy = computed(() => activeMethod.value !== null || authStore.loading)
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

async function refreshCdpStatus(): Promise<CdpStatus | null> {
  if (cdpChecking.value) return cdpStatus.value
  cdpChecking.value = true
  cdpProbeFailed.value = false
  try {
    const status = await checkCdpStatus(questsStore.cdpPort)
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

async function handleCdpLogin() {
  if (!begin('cdp')) return
  setProgress('cdp', 'running', 'auth.progress.checking_cdp')
  try {
    const status = await refreshCdpStatus()
    if (!status?.available) {
      try {
        setProgress('cdp', 'running', 'auth.progress.launching_discord')
        await launchDiscordCdp(questsStore.cdpPort, 'auto')
        await refreshCdpStatus()
      } catch (launchError) {
        const retryStatus = await refreshCdpStatus()
        if (!retryStatus?.available) {
          const running = await isDiscordRunning('auto')
          if (running) {
            setProgress('cdp', 'waiting', 'auth.progress.restart_required')
            cdpRestartDialogOpen.value = true
            return
          }
          throw launchError
        }
      }
    }
    await finishCdpLogin()
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
  setProgress('cdp', 'running', 'auth.progress.restarting_discord')
  try {
    await restartDiscordCdp(questsStore.cdpPort, 'auto')
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

function handleRestartDialogOpenChange(open: boolean) {
  cdpRestartDialogOpen.value = open
  if (!open && progress.value?.state === 'waiting') {
    setProgress('cdp', 'neutral', 'auth.progress.restart_cancelled')
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
  stopCdpPolling = startCdpPolling(pollCdpIfNeeded, CDP_POLL_INTERVAL_MS)
  document.addEventListener('visibilitychange', handleVisibilityChange)
})

onUnmounted(() => {
  accountViewTransitionRevision += 1
  document.documentElement.classList.remove('account-view-transition')
  stopCdpPolling?.()
  document.removeEventListener('visibilitychange', handleVisibilityChange)
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
              <span class="block truncate text-xs text-muted-foreground">@{{ account.user.username }}</span>
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
                <p class="mt-1 max-w-md text-sm leading-5 text-muted-foreground">{{ t('auth.cdp_login_detail') }}</p>
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
                {{ t('auth.cdp_login_action') }}
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

    <AlertDialog :open="cdpRestartDialogOpen" @update:open="handleRestartDialogOpenChange">
      <AlertDialogContent class="max-w-[520px]">
        <AlertDialogHeader>
          <AlertDialogTitle>{{ t('settings.cdp_dialog_title_disconnected') }}</AlertDialogTitle>
          <AlertDialogDescription>{{ t('settings.cdp_dialog_desc_disconnected') }}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <AlertDialogAction @click="confirmCdpRestart">
            {{ t('settings.cdp_dialog_confirm') }}
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
