import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { DiscordUser, ExtractedAccount, ProgramReward, AuthProgress, AuthProgressHandler } from '@/api/tauri'
import { autoDetectToken, setToken, autoLoginViaCdp, autoFetchSuperProperties, getProgramRewards } from '@/api/tauri'
import { useQuestsStore } from './quests'
import { useI18n } from 'vue-i18n'
import { useNow } from '@vueuse/core'
import { getNitroOrbsClaim } from '@/utils/nitroOrbsCountdown'

export const useAuthStore = defineStore('auth', () => {
  const { t } = useI18n()
  const user = ref<DiscordUser | null>(null)
  const token = ref<string | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const detectedAccounts = ref<ExtractedAccount[]>([])

  // Discord's Program Rewards endpoint owns the monthly Orbs schedule.
  const nitroProgramReward = ref<ProgramReward | null>(null)
  const programRewardLoading = ref(false)
  const programRewardError = ref<string | null>(null)
  const programRewardLoaded = ref(false)
  const currentTime = useNow({ interval: 60_000 })
  let programRewardRequestRevision = 0

  function resetProgramRewardState() {
    programRewardRequestRevision += 1
    nitroProgramReward.value = null
    programRewardLoading.value = false
    programRewardError.value = null
    programRewardLoaded.value = false
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
    resetProgramRewardState()
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
    resetProgramRewardState()
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
    fetchNitroProgramReward().catch(err => {
      console.warn('Background Nitro program reward fetch failed:', err)
    })
  }

  async function logout() {
    // Invalidate account-scoped requests before awaiting quest shutdown.
    resetProgramRewardState()

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

  async function fetchNitroProgramReward(force = false) {
    if (programRewardLoading.value) return
    if (!force && programRewardLoaded.value) return
    // CDP auto-login is authenticated on the backend but exposes no frontend
    // token, so gate on the logged-in user rather than the raw token.
    if (!user.value) return
    const requestToken = token.value
    const requestRevision = ++programRewardRequestRevision
    programRewardLoading.value = true
    programRewardError.value = null
    try {
      const rewards = await getProgramRewards()
      if (requestRevision !== programRewardRequestRevision || token.value !== requestToken) return
      nitroProgramReward.value = rewards.find(reward => {
        const program = reward.reward_program
        // Discord's official ProgramReward enum is NITRO=0, XBOX=1.
        // Keep the string forms for keyed/legacy response normalization.
        return program === 0 || program === '0' || String(program).toUpperCase() === 'NITRO'
      }) ?? null
      programRewardLoaded.value = true
    } catch (e) {
      if (requestRevision !== programRewardRequestRevision || token.value !== requestToken) return
      programRewardError.value = e as string
      console.warn('Failed to fetch Nitro program reward:', e)
    } finally {
      if (requestRevision === programRewardRequestRevision && token.value === requestToken) {
        programRewardLoading.value = false
      }
    }
  }

  // Discord supplies the authoritative absolute timestamp, so no local or
  // UTC calendar arithmetic is needed here.
  const nextOrbsClaim = computed<
    { value: number; unit: 'days' | 'hours' | 'minutes' } | null
  >(() => {
    return getNitroOrbsClaim(nitroProgramReward.value?.next_reward_date, currentTime.value)
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
    nitroProgramReward,
    programRewardLoading,
    programRewardError,
    nextOrbsClaim,
    nitroStatus,
    tryAutoDetect,
    loginWithToken,
    loginViaCdp,
    logout,
    fetchNitroProgramReward
  }
})
