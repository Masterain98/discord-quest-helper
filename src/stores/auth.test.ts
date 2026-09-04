import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { DiscordUser } from '@/api/tauri'
import { useAuthStore } from './auth'

const mocks = vi.hoisted(() => ({
  autoLoginViaCdp: vi.fn(),
  setToken: vi.fn(),
  autoFetchSuperProperties: vi.fn(),
  getBillingSubscriptions: vi.fn(),
  questsStore: {
    cdpPort: 9223,
    cdpAvailable: false,
    gameQuestMode: 'simulate',
    initCdpMode: vi.fn(),
    getDetectableGames: vi.fn(),
    fetchOrbsBalance: vi.fn(),
    stop: vi.fn(),
    resetForLogout: vi.fn(),
  },
}))

vi.mock('@/api/tauri', () => ({
  autoDetectToken: vi.fn(),
  autoLoginViaCdp: mocks.autoLoginViaCdp,
  setToken: mocks.setToken,
  autoFetchSuperProperties: mocks.autoFetchSuperProperties,
  getBillingSubscriptions: mocks.getBillingSubscriptions,
}))

vi.mock('./quests', () => ({
  useQuestsStore: () => mocks.questsStore,
}))

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

vi.mock('@vueuse/core', () => ({
  useNow: () => ({ value: new Date('2026-08-31T00:00:00.000Z') }),
}))

const user: DiscordUser = {
  id: '123',
  username: 'quest-user',
  discriminator: '0',
  avatar: null,
  global_name: 'Quest User',
}

describe('auth login quest mode selection', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    mocks.questsStore.cdpAvailable = false
    mocks.questsStore.gameQuestMode = 'simulate'
    mocks.autoLoginViaCdp.mockResolvedValue(user)
    mocks.setToken.mockResolvedValue(user)
    mocks.autoFetchSuperProperties.mockResolvedValue(undefined)
    mocks.getBillingSubscriptions.mockResolvedValue([])
    mocks.questsStore.initCdpMode.mockResolvedValue(undefined)
    mocks.questsStore.getDetectableGames.mockResolvedValue(undefined)
    mocks.questsStore.fetchOrbsBalance.mockResolvedValue(undefined)
  })

  it('selects CDP quest execution after a successful CDP login', async () => {
    const authStore = useAuthStore()

    await expect(authStore.loginViaCdp()).resolves.toBe(true)

    expect(mocks.questsStore.cdpAvailable).toBe(true)
    expect(mocks.questsStore.gameQuestMode).toBe('cdp')
    expect(mocks.questsStore.initCdpMode).toHaveBeenCalledOnce()
  })

  it('preserves the selected quest mode after a token login', async () => {
    const authStore = useAuthStore()

    await expect(authStore.loginWithToken('token-value')).resolves.toBe(true)

    expect(mocks.questsStore.gameQuestMode).toBe('simulate')
  })
})
