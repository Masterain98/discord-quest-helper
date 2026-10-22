import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { getCurrentUserBadges, type UserBadge } from '@/api/userBadges'
import { useAuthStore } from './auth'

const CACHE_TTL = 60_000

export const useUserBadgesStore = defineStore('userBadges', () => {
  const auth = useAuthStore()
  const badges = ref<UserBadge[]>([])
  const loading = ref(false)
  const failed = ref(false)
  let revision = 0
  let lastAttempt: number | null = null
  let pending: Promise<void> | null = null

  function refresh(): Promise<void> {
    if (!auth.user) return Promise.resolve()
    if (pending) return pending
    if (lastAttempt !== null && Date.now() - lastAttempt < CACHE_TTL) return Promise.resolve()
    const accountId = auth.user.id
    const requestRevision = revision
    lastAttempt = Date.now()
    loading.value = true
    failed.value = false
    const isCurrent = () => requestRevision === revision && accountId === auth.user?.id
    pending = (async () => {
      try {
        const result = await getCurrentUserBadges()
        if (isCurrent()) badges.value = result
      } catch {
        if (isCurrent()) failed.value = true
      } finally {
        if (isCurrent()) {
          loading.value = false
          pending = null
        }
      }
    })()
    return pending
  }

  // The shell creates this store regardless of whether the menu is open.
  // Start independently of login/page loading, and discard late account data.
  watch(() => auth.user?.id, () => {
    revision += 1
    badges.value = []
    loading.value = false
    failed.value = false
    lastAttempt = null
    pending = null
    void refresh()
  }, { immediate: true, flush: 'sync' })

  return { badges, loading, failed, refresh }
})
