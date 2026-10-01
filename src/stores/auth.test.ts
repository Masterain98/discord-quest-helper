import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { DiscordUser } from '@/api/tauri'
import { useAuthStore } from './auth'

const mocks = vi.hoisted(() => ({
  autoDetectToken: vi.fn(),
  autoLoginViaCdp: vi.fn(),
  setToken: vi.fn(),
  autoFetchSuperProperties: vi.fn(),
  getProgramRewards: vi.fn(),
  getGameSimulationHistory: vi.fn(),
  stopAllGameSimulations: vi.fn(),
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
  gameIdleStore: {
    stopForAccountChange: vi.fn(),
  },
}))

vi.mock('@/api/tauri', () => ({
  autoDetectToken: mocks.autoDetectToken,
  autoLoginViaCdp: mocks.autoLoginViaCdp,
  setToken: mocks.setToken,
  autoFetchSuperProperties: mocks.autoFetchSuperProperties,
  getProgramRewards: mocks.getProgramRewards,
  getGameSimulationHistory: mocks.getGameSimulationHistory,
  stopAllGameSimulations: mocks.stopAllGameSimulations,
  getGameIdleStatus: vi.fn().mockResolvedValue(null),
  onGameIdleStatus: vi.fn(),
  onGameSimulationHistoryUpdated: vi.fn(),
  removeGameIdleQueueItem: vi.fn(),
  startGameIdle: vi.fn(),
  stopGameIdle: vi.fn(),
  // `gameIdle.stopForAccountChange` (invoked on login/logout) finishes the
  // active usage segment, so the mock must expose it.
  stopGameSimulationUsage: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('./quests', () => ({
  useQuestsStore: () => mocks.questsStore,
}))

// The real store reads `localStorage` during setup, which the node test
// environment does not provide. Replace the module the same way `./quests`
// is replaced; only `stopForAccountChange` is reached from the auth flows.
vi.mock('./gameIdle', () => ({
  useGameIdleStore: () => mocks.gameIdleStore,
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
    mocks.autoDetectToken.mockResolvedValue([])
    mocks.questsStore.cdpAvailable = false
    mocks.questsStore.gameQuestMode = 'simulate'
    mocks.autoLoginViaCdp.mockResolvedValue(user)
    mocks.setToken.mockResolvedValue(user)
    mocks.autoFetchSuperProperties.mockResolvedValue(undefined)
    mocks.getProgramRewards.mockResolvedValue([])
    mocks.getGameSimulationHistory.mockResolvedValue([])
    mocks.stopAllGameSimulations.mockResolvedValue(undefined)
    mocks.questsStore.initCdpMode.mockResolvedValue(undefined)
    mocks.questsStore.getDetectableGames.mockResolvedValue(undefined)
    mocks.questsStore.fetchOrbsBalance.mockResolvedValue(undefined)
    mocks.questsStore.stop.mockResolvedValue(undefined)
    mocks.gameIdleStore.stopForAccountChange.mockResolvedValue(undefined)
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

  it.each(['cdp', 'token', 'auto'] as const)(
    'displays structured command errors from %s login',
    async (method) => {
      const failure = {
        code: 'process_ambiguous',
        params: { port: 9223 },
        message: 'The current CDP owner could not be mapped to one exact installation.',
      }
      const authStore = useAuthStore()
      mocks.autoLoginViaCdp.mockRejectedValue(failure)
      mocks.setToken.mockRejectedValue(failure)
      mocks.autoDetectToken.mockRejectedValue(failure)

      const result = method === 'cdp' ? await authStore.loginViaCdp()
        : method === 'token' ? await authStore.loginWithToken('test-token')
          : await authStore.tryAutoDetect()

      expect(result).toBe(false)
      expect(authStore.error).toBe(failure.message)
      expect(authStore.loading).toBe(false)
      expect(authStore.user).toBeNull()
    },
  )

  it('cancels pending idle work before stopping simulations for an account switch', async () => {
    const authStore = useAuthStore()
    authStore.user = user

    await expect(authStore.loginWithToken('next-account-token')).resolves.toBe(true)
    expect(mocks.gameIdleStore.stopForAccountChange.mock.invocationCallOrder[0])
      .toBeLessThan(mocks.stopAllGameSimulations.mock.invocationCallOrder[0])
  })

  it('uses Discord program reward timestamps for the Orbs countdown', async () => {
    const authStore = useAuthStore()
    authStore.user = user
    mocks.getProgramRewards.mockResolvedValue([
      {
        // Discord's live enum is NITRO=0 (XBOX=1).
        reward_program: 0,
        next_reward_date: '2026-09-17T00:00:00.000Z',
      },
    ])

    await authStore.fetchNitroProgramReward()

    expect(authStore.nextOrbsClaim).toEqual({ value: 17, unit: 'days' })
  })
})
