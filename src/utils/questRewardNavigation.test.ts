import { describe, expect, it, vi } from 'vitest'
import { discordIntegrationAction, manualRewardClaimNotice, openRewardQuest } from './questRewardNavigation'

const mocks = vi.hoisted(() => ({ navigateToTab: vi.fn() }))
vi.mock('@/utils/navigate', () => mocks)

describe('reward navigation', () => {
  it('requires manual claiming without CDP and performs no navigation', async () => {
    const navigate = vi.fn()
    expect(await openRewardQuest('123', { cdpAvailable: false, cdpPort: 9333, navigate })).toBe('manual')
    expect(navigate).not.toHaveBeenCalled()
  })

  it('waits for navigation to the exact quest using the configured CDP port', async () => {
    let resolve!: () => void
    const navigate = vi.fn(() => new Promise<void>(done => { resolve = done }))
    const completed = vi.fn()
    const operation = openRewardQuest('123', { cdpAvailable: true, cdpPort: 9333, navigate }).then(completed)
    await Promise.resolve()
    expect(navigate).toHaveBeenCalledWith('/quest-home#123', 9333)
    expect(completed).not.toHaveBeenCalled()
    resolve()
    await operation
    expect(completed).toHaveBeenCalledWith('navigated')
  })

  it('encodes the quest ID as a single fragment', async () => {
    const navigate = vi.fn().mockResolvedValue(undefined)
    await openRewardQuest('a#b/c', { cdpAvailable: true, cdpPort: 9223, navigate })
    expect(navigate).toHaveBeenCalledWith('/quest-home#a%23b%2Fc', 9223)
  })

  it('propagates navigation errors without a fallback or success result', async () => {
    const error = new Error('Discord disconnected')
    const navigate = vi.fn().mockRejectedValue(error)
    await expect(openRewardQuest('123', { cdpAvailable: true, cdpPort: 9223, navigate })).rejects.toBe(error)
    expect(navigate).toHaveBeenCalledOnce()
  })

  it('explains manual claiming and opens the client integration settings on demand', () => {
    const notice = manualRewardClaimNotice(key => key)
    expect(notice.title).toBe('toast.manual_claim')
    expect(notice.description).toBe('toast.manual_claim_hint')
    expect(notice.actions).toHaveLength(1)
    expect(notice.actions![0].label).toBe('toast.open_discord_settings')
    expect(mocks.navigateToTab).not.toHaveBeenCalled()
    notice.actions![0].onClick()
    expect(mocks.navigateToTab).toHaveBeenCalledWith('settings', 'discord_integration')
    expect(discordIntegrationAction(key => key).label).toBe('toast.open_discord_settings')
  })
})
