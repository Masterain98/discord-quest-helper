import { describe, expect, it, vi } from 'vitest'
import { openGlobalQuest } from './globalQuestNavigation'

describe('global quest navigation', () => {
  it('opens the exact quest in the Discord SPA using the configured CDP port', async () => {
    const navigate = vi.fn().mockResolvedValue(undefined), openExternal = vi.fn()
    await openGlobalQuest('1556984508107063417', { mode: 'cdp', cdpPort: 9333, navigate, openExternal })
    expect(navigate).toHaveBeenCalledWith('/quest-home#1556984508107063417', 9333)
    expect(openExternal).not.toHaveBeenCalled()
  })
  it.each(['simulate', 'heartbeat'] as const)('uses the external quest URL in %s mode', async mode => {
    const navigate = vi.fn(), openExternal = vi.fn().mockResolvedValue(undefined)
    await openGlobalQuest('111', { mode, cdpPort: 9223, navigate, openExternal })
    expect(openExternal).toHaveBeenCalledWith('https://discord.com/quests/111')
    expect(navigate).not.toHaveBeenCalled()
  })
  it('reports CDP failures without opening a browser instead', async () => {
    const navigate = vi.fn().mockRejectedValue(new Error('CDP unavailable')), openExternal = vi.fn()
    await expect(openGlobalQuest('111', { mode: 'cdp', cdpPort: 9223, navigate, openExternal })).rejects.toThrow('CDP unavailable')
    expect(openExternal).not.toHaveBeenCalled()
  })
  it('rejects invalid IDs before performing any navigation', async () => {
    const navigate = vi.fn(), openExternal = vi.fn()
    await expect(openGlobalQuest('../channels/@me', { mode: 'cdp', cdpPort: 9223, navigate, openExternal })).rejects.toThrow('Invalid quest ID')
    expect(navigate).not.toHaveBeenCalled(); expect(openExternal).not.toHaveBeenCalled()
  })
})
