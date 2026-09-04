import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { DiscordUser, ExtractedAccount, BillingSubscription, AuthProgress, AuthProgressHandler } from '@/api/tauri'
import { autoDetectToken, setToken, autoLoginViaCdp, autoFetchSuperProperties, getBillingSubscriptions } from '@/api/tauri'
import { useQuestsStore } from './quests'
import { useI18n } from 'vue-i18n'
import { useNow } from '@vueuse/core'

export const useAuthStore = defineStore('auth', () => {
  const { t } = useI18n()
  const user = ref<DiscordUser | null>(null)
  const token = ref<string | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const detectedAccounts = ref<ExtractedAccount[]>([])

  // Billing subscriptions (used to derive Nitro monthly Orbs grant anchor)
  const billingSubscriptions = ref<BillingSubscription[]>([])
  const billingLoading = ref(false)
  const billingError = ref<string | null>(null)
  const currentTime = useNow({ interval: 60_000 })
  let billingRequestRevision = 0

  function resetBillingState() {
    billingRequestRevision += 1
    billingSubscriptions.value = []
    billingLoading.value = false
    billingError.value = null
  }

  async function tryAutoDetect(
    onProgress?: AuthProgressHandler,
    commitMultipleAccounts?: (accounts: ExtractedAccount[]) => void | Promise<void>,
  ) {
    loading.value = true
    error.value = null
    detectedAccounts.value = []

    try {
      const accounts = await autoDetectToken(onProgress)

      if (accounts.length === 1) {
        // Only one account found, login automatically
        return await loginWithToken(accounts[0].token, onProgress)
      } else {
        // Multiple accounts, let UI handle selection
        if (commitMultipleAccounts) {
          await commitMultipleAccounts(accounts)
        } else {
          detectedAccounts.value = accounts
        }
      }
      return true
    } catch (e) {
      console.error('Auto detect failed:', e)
      error.value = e instanceof Error ? e.message : String(e)
      return false
    } finally {
      loading.value = false
    }
  }

  async function loginWithToken(tokenValue: string, onProgress?: AuthProgressHandler) {
    loading.value = true
    error.value = null
    resetBillingState()
    try {
      user.value = await setToken(tokenValue, (progress) => {
        // The store still performs one final SuperProperties synchronization
        // after the backend command. Keep the visible operation running until
        // that existing step has settled.
        if (progress.phase !== 'complete') onProgress?.(progress)
      })
      token.value = tokenValue

      // After successful login, wait for SuperProperties fetch to complete
      // This ensures all data is ready before ending the loading state
      try {
        const questsStore = useQuestsStore()
        await autoFetchSuperProperties(questsStore.cdpPort)

        bootstrapAfterLogin(questsStore, 'CDP init on login failed:')
      } catch (e) {
        // SuperProperties fetch failure should not block login
        console.warn('Failed to fetch SuperProperties:', e)
      }

      onProgress?.(completeAuthProgress())

      return true
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      return false
    } finally {
      loading.value = false
    }
  }

  /**
   * Log in by capturing the currently running Discord client's session over CDP
   * (the primary login path on Linux). The raw token is never exposed to the
   * frontend: the backend captures, validates, and stores it, returning only the
   * DiscordUser. Requires Discord to be running with CDP enabled.
   */
  async function loginViaCdp(onProgress?: AuthProgressHandler) {
    loading.value = true
    error.value = null
    resetBillingState()
    try {
      const questsStore = useQuestsStore()
      user.value = await autoLoginViaCdp(questsStore.cdpPort, onProgress)
      // Intentionally leave `token` null: CDP auto-login never surfaces the raw
      // token. Authenticated backend commands use the client in AppState.
      token.value = null

      // CDP is available by definition here (we just used it). Keep the login
      // method and quest execution method aligned so the first quest does not
      // fall back to a previously saved simulation preference.
      questsStore.cdpAvailable = true
      questsStore.gameQuestMode = 'cdp'

      // Refresh the connection state and the rest of the post-login data.
      bootstrapAfterLogin(questsStore, 'CDP init after CDP login failed:')

      return true
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      return false
    } finally {
      loading.value = false
    }
  }

  function completeAuthProgress(): AuthProgress {
    return {
      phase: 'complete',
      current: null,
      total: null,
      valid_accounts: null,
    }
  }

  // Keep post-login refresh work non-blocking for both authentication paths.
  function bootstrapAfterLogin(questsStore: ReturnType<typeof useQuestsStore>, cdpWarning: string) {
    questsStore.initCdpMode().catch(err => {
      console.warn(cdpWarning, err)
    })
    questsStore.getDetectableGames().catch(err => {
      console.warn('Background game list fetch failed:', err)
    })
    questsStore.fetchOrbsBalance().catch(err => {
      console.warn('Background Orbs balance fetch failed:', err)
    })
    fetchBillingSubscription().catch(err => {
      console.warn('Background billing subscriptions fetch failed:', err)
    })
  }

  async function logout() {
    // Invalidate account-scoped requests before awaiting quest shutdown.
    resetBillingState()

    // Stop any in-progress quest before clearing state
    const questsStore = useQuestsStore()
    try {
      await questsStore.stop()
    } catch (e) {
      console.warn('Failed to stop quest during logout:', e)
    }

    user.value = null
    token.value = null
    error.value = null
    detectedAccounts.value = []

    // Reset quests store to clear all cached data from previous account
    questsStore.resetForLogout()
  }

  async function fetchBillingSubscription(force = false) {
    if (billingLoading.value) return
    if (!force && billingSubscriptions.value.length > 0) return
    // CDP auto-login is authenticated on the backend but exposes no frontend
    // token, so gate on the logged-in user rather than the raw token.
    if (!user.value) return
    const requestToken = token.value
    const requestRevision = ++billingRequestRevision
    billingLoading.value = true
    billingError.value = null
    try {
      const subscriptions = await getBillingSubscriptions()
      if (requestRevision !== billingRequestRevision || token.value !== requestToken) return
      billingSubscriptions.value = subscriptions
    } catch (e) {
      if (requestRevision !== billingRequestRevision || token.value !== requestToken) return
      billingError.value = e as string
      console.warn('Failed to fetch billing subscriptions:', e)
    } finally {
      if (requestRevision === billingRequestRevision && token.value === requestToken) {
        billingLoading.value = false
      }
    }
  }

  // The Nitro subscription (monthly Orbs are a Nitro perk). Only positively
  // identified Nitro/Premium plans can provide the monthly grant anchor.
  const nitroSubscription = computed<BillingSubscription | null>(() => {
    const subs = billingSubscriptions.value
    if (!subs.length) return null
    const nitro = subs.find(s =>
      (s.payment_gateway_plan_id && /premium|nitro/i.test(s.payment_gateway_plan_id)) ||
      (s.items && s.items.some(it => /premium|nitro/i.test(it.plan_id)))
    )
    return nitro ?? null
  })

  // Days until the next monthly Orbs grant.
  // Discord grants monthly Nitro Orbs on the subscription anniversary day
  // (current_period_start day-of-month), so we compute the next occurrence of
  // that day relative to now.
  // Days / hours / minutes until the next monthly Orbs grant.
  // Discord grants monthly Nitro Orbs on the subscription anniversary day
  // (current_period_start day-of-month). Below 48h we switch to hours, and
  // below 12h we switch to minutes for a finer-grained countdown.
  const nextOrbsClaim = computed<
    { value: number; unit: 'days' | 'hours' | 'minutes' } | null
  >(() => {
    const start = nitroSubscription.value?.current_period_start
    if (!start) return null
    const anchor = new Date(start)
    if (isNaN(anchor.getTime())) return null
    const anchorDay = anchor.getUTCDate()

    const now = currentTime.value
    const nowYear = now.getUTCFullYear()
    const nowMonth = now.getUTCMonth()

    // Find the first occurrence of anchorDay in a future month, clamping to the
    // last day of the month when anchorDay exceeds that month's length (e.g. day
    // 31 in February would otherwise roll over and break the search).
    let candidate: Date | null = null
    for (let offset = 0; offset < 12; offset++) {
      const y = nowYear + Math.floor((nowMonth + offset) / 12)
      const m = (nowMonth + offset) % 12
      const lastDay = new Date(Date.UTC(y, m + 1, 0)).getUTCDate()
      const day = Math.min(anchorDay, lastDay)
      const c = new Date(Date.UTC(y, m, day))
      if (c.getTime() > now.getTime()) {
        candidate = c
        break
      }
    }
    if (!candidate) return null

    const ms = candidate.getTime() - now.getTime()
    const MINUTE = 1000 * 60
    const HOUR = MINUTE * 60
    const DAY = HOUR * 24

    if (ms < 12 * HOUR) {
      return { value: Math.max(1, Math.ceil(ms / MINUTE)), unit: 'minutes' }
    }
    if (ms < 48 * HOUR) {
      return { value: Math.max(1, Math.ceil(ms / HOUR)), unit: 'hours' }
    }
    return { value: Math.max(1, Math.ceil(ms / DAY)), unit: 'days' }
  })

  // Localized Nitro membership status label + color class (null for non-members).
  const nitroStatus = computed<{ label: string; class: string } | null>(() => {
    const pt = user.value?.premium_type
    if (!pt || pt === 0) return null
    if (pt === 1) return { label: t('user.nitro_classic'), class: 'text-sky-600 dark:text-sky-400' }
    if (pt === 2) return { label: t('user.nitro'), class: 'text-violet-600 dark:text-violet-400' }
    if (pt === 3) return { label: t('user.nitro_basic'), class: 'text-indigo-600 dark:text-indigo-400' }
    return null
  })

  return {
    user,
    token,
    loading,
    error,
    detectedAccounts,
    billingSubscriptions,
    billingLoading,
    billingError,
    nitroSubscription,
    nextOrbsClaim,
    nitroStatus,
    tryAutoDetect,
    loginWithToken,
    loginViaCdp,
    logout,
    fetchBillingSubscription
  }
})
