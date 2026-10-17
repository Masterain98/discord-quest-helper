import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useQuestsStore } from './quests'

const mocks = vi.hoisted(() => ({
  getVirtualCurrencyBalance: vi.fn(),
  getVirtualCurrencyTotalRedeemed: vi.fn(),
}))
vi.mock('@/api/tauri', () => ({
  ...mocks,
  getPlatformCapabilities: vi.fn().mockResolvedValue({
    os: 'windows', defaultGameQuestMode: 'simulate', executableOsPriority: ['win32'],
  }),
  onQuestProgress: vi.fn().mockResolvedValue(() => undefined),
  onQuestComplete: vi.fn().mockResolvedValue(() => undefined),
  onQuestError: vi.fn().mockResolvedValue(() => undefined),
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

beforeEach(() => {
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() })
  vi.stubGlobal('cancelAnimationFrame', vi.fn())
  vi.clearAllMocks()
  mocks.getVirtualCurrencyBalance.mockResolvedValue(87040)
  mocks.getVirtualCurrencyTotalRedeemed.mockResolvedValue(133520)
  setActivePinia(createPinia())
})
afterEach(() => vi.unstubAllGlobals())

describe('account Orbs summary', () => {
  it('publishes each metric as it arrives and shares startup requests', async () => {
    const redeemed = deferred<number>()
    mocks.getVirtualCurrencyTotalRedeemed.mockReturnValue(redeemed.promise)
    const store = useQuestsStore()
    const first = store.fetchOrbsSummary()
    let secondFinished = false
    const second = store.fetchOrbsSummary(true).then(() => { secondFinished = true })
    await Promise.resolve()
    expect(store.orbsBalance).toBe(87040)
    expect(store.orbsBalanceLoading).toBe(false)
    expect(store.orbsTotalRedeemedLoading).toBe(true)
    expect(store.orbsSummaryLoading).toBe(true)
    expect(secondFinished).toBe(false)
    redeemed.resolve(133520)
    await Promise.all([first, second])
    expect(store.orbsTotalRedeemed).toBe(133520)
    expect(store.orbsSummaryLoading).toBe(false)
    await store.fetchOrbsSummary()
    expect(mocks.getVirtualCurrencyBalance).toHaveBeenCalledTimes(1)
    expect(mocks.getVirtualCurrencyTotalRedeemed).toHaveBeenCalledTimes(1)
  })

  it.each(['balance', 'redeemed'] as const)('keeps the other metric when %s fails and retries only manually', async failed => {
    const fail = failed === 'balance' ? mocks.getVirtualCurrencyBalance : mocks.getVirtualCurrencyTotalRedeemed
    fail.mockRejectedValueOnce(new Error('Offline'))
    const store = useQuestsStore()
    await expect(store.fetchOrbsSummary()).rejects.toThrow('Offline')
    expect(store.orbsBalance).toBe(failed === 'balance' ? null : 87040)
    expect(store.orbsTotalRedeemed).toBe(failed === 'redeemed' ? null : 133520)
    expect(store.orbsBalanceError).toBe(failed === 'balance' ? 'Offline' : null)
    expect(store.orbsTotalRedeemedError).toBe(failed === 'redeemed' ? 'Offline' : null)
    await store.fetchOrbsSummary()
    expect(fail).toHaveBeenCalledTimes(1)
    await store.fetchOrbsSummary(true)
    expect(store.orbsBalance).toBe(87040)
    expect(store.orbsTotalRedeemed).toBe(133520)
    expect(store.orbsBalanceError).toBeNull()
    expect(store.orbsTotalRedeemedError).toBeNull()
  })

  it('retains previously loaded values and timestamps after a failed refresh', async () => {
    const store = useQuestsStore()
    await store.fetchOrbsSummary()
    const timestamp = store.orbsTotalRedeemedFetchedAt
    mocks.getVirtualCurrencyBalance.mockResolvedValueOnce(0)
    mocks.getVirtualCurrencyTotalRedeemed.mockRejectedValueOnce(new Error('Offline'))
    await expect(store.fetchOrbsSummary(true)).rejects.toThrow('Offline')
    expect(store.orbsBalance).toBe(0)
    expect(store.orbsTotalRedeemed).toBe(133520)
    expect(store.orbsTotalRedeemedFetchedAt).toBe(timestamp)
    expect(store.orbsSummaryLoading).toBe(false)
  })

  it('drops old-account responses without finishing the new account request', async () => {
    const oldBalance = deferred<number>()
    const oldRedeemed = deferred<number>()
    const newBalance = deferred<number>()
    const newRedeemed = deferred<number>()
    mocks.getVirtualCurrencyBalance.mockReturnValueOnce(oldBalance.promise).mockReturnValueOnce(newBalance.promise)
    mocks.getVirtualCurrencyTotalRedeemed.mockReturnValueOnce(oldRedeemed.promise).mockReturnValueOnce(newRedeemed.promise)
    const store = useQuestsStore()
    store.setQuestAccount('old')
    const old = store.fetchOrbsSummary()
    store.setQuestAccount('new')
    const current = store.fetchOrbsSummary()
    oldBalance.resolve(87040)
    oldRedeemed.reject(new Error('Old account error'))
    await old
    expect(store.orbsBalance).toBeNull()
    expect(store.orbsTotalRedeemed).toBeNull()
    expect(store.orbsTotalRedeemedError).toBeNull()
    expect(store.orbsSummaryLoading).toBe(true)
    newBalance.resolve(0)
    newRedeemed.resolve(1234)
    await current
    expect(store.orbsBalance).toBe(0)
    expect(store.orbsTotalRedeemed).toBe(1234)
    store.resetForLogout()
    expect(store.orbsBalance).toBeNull()
    expect(store.orbsTotalRedeemed).toBeNull()
    expect(store.orbsTotalRedeemedFetchedAt).toBeNull()
  })

  it('respects the balance visibility preference', async () => {
    const store = useQuestsStore()
    store.showOrbsBalance = false
    await store.fetchOrbsSummary()
    expect(mocks.getVirtualCurrencyBalance).not.toHaveBeenCalled()
    expect(mocks.getVirtualCurrencyTotalRedeemed).not.toHaveBeenCalled()
  })
})
