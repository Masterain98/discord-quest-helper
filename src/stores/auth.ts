import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { DiscordUser, ExtractedAccount } from '@/api/tauri'
import { autoDetectToken, setToken, autoFetchSuperProperties } from '@/api/tauri'
import { useQuestsStore } from './quests'

export const useAuthStore = defineStore('auth', () => {
  const user = ref<DiscordUser | null>(null)
  const token = ref<string | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const detectedAccounts = ref<ExtractedAccount[]>([])

  async function tryAutoDetect() {
    loading.value = true
    error.value = null
    detectedAccounts.value = []

    try {
      console.log('Calling autoDetectToken...')
      const accounts = await autoDetectToken()
      console.log('Received accounts:', accounts)

      if (accounts.length === 1) {
        console.log('Single account found, logging in...')
        // Only one account found, login automatically
        await loginWithToken(accounts[0].token)
      } else {
        console.log('Multiple accounts found, updating detectedAccounts state...')
        // Multiple accounts, let UI handle selection
        detectedAccounts.value = accounts
      }
      return true
    } catch (e) {
      console.error('Auto detect failed:', e)
      error.value = e as string
      return false
    } finally {
      loading.value = false
    }
  }

  async function loginWithToken(tokenValue: string) {
    loading.value = true
    error.value = null
    try {
      user.value = await setToken(tokenValue)
      token.value = tokenValue

      // After successful login, wait for SuperProperties fetch to complete
      // This ensures all data is ready before ending the loading state
      try {
        const questsStore = useQuestsStore()
        await autoFetchSuperProperties(questsStore.cdpPort)
      } catch (e) {
        // SuperProperties fetch failure should not block login
        console.warn('Failed to fetch SuperProperties:', e)
      }

      return true
    } catch (e) {
      error.value = e as string
      return false
    } finally {
      loading.value = false
    }
  }

  function logout() {
    user.value = null
    token.value = null
    error.value = null
  }

  return {
    user,
    token,
    loading,
    error,
    detectedAccounts,
    tryAutoDetect,
    loginWithToken,
    logout
  }
})
