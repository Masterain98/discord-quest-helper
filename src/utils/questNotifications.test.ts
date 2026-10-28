import { describe, expect, it, vi } from 'vitest'
import { QuestNotifications, estimatedCompletionAt, notificationRewardImage } from './questNotifications'
import type { Quest } from '@/api/tauri'

describe('quest notification execution boundaries', () => {
  it('calculates completion from remaining work and the execution speed', () => {
    const now = Date.parse('2026-10-18T15:00:00Z')
    expect(estimatedCompletionAt(120, 2, now)).toBe('2026-10-18T15:01:00.000Z')
    expect(estimatedCompletionAt(120, 0.5, now)).toBe('2026-10-18T15:04:00.000Z')
    expect(estimatedCompletionAt(-5, 1, now)).toBe('2026-10-18T15:00:00.000Z')
    expect(estimatedCompletionAt(Infinity)).toBeNull()
    expect(estimatedCompletionAt(120, 0)).toBeNull()
    expect(estimatedCompletionAt(Number.MAX_VALUE)).toBeNull()
  })
  it('prefers static reward artwork and handles orbs without selecting videos', () => {
    const quest = { config: { rewards_config: { rewards: [
      { type: 2, asset: 'rewards/item.png', messages: { name: 'Item' } },
      { type: 3, asset: 'rewards/decoration.png', messages: { name: 'Decoration' } },
    ] } }, user_status: null } as Quest
    expect(notificationRewardImage(quest)).toBe('https://cdn.discordapp.com/rewards/decoration.png')
    quest.config.rewards_config!.rewards = [{ type: 4, orb_quantity: 100, sku_id: 'orbs', messages: { name: 'Orbs' } }]
    expect(notificationRewardImage(quest)).toBe('/icons/orbs.png')
    quest.config.rewards_config!.rewards = [{ type: 3, sku_id: 'video', asset: 'reward.mp4', messages: { name: 'Decoration' } }]
    expect(notificationRewardImage(quest)).toBeNull()
  })
  it('retains reward metadata for completion without carrying over a stale estimate', () => {
    const send = vi.fn()
    const tracker = new QuestNotifications(send)
    tracker.started('one', 'One', { reward_image: '/icons/orbs.png', estimated_complete_at: '2026-10-18T15:05:00Z' })
    tracker.completed('one')
    expect(send.mock.calls[0][0]).toMatchObject({ estimated_complete_at: '2026-10-18T15:05:00Z', reward_image: '/icons/orbs.png' })
    expect(send.mock.calls[1][0]).toMatchObject({ kind: 'quest_completed', reward_image: '/icons/orbs.png' })
    expect(send.mock.calls[1][0].estimated_complete_at).toBeUndefined()
  })
  it('deduplicates completion sources but allows a new execution of the same quest', () => {
    const send = vi.fn()
    const tracker = new QuestNotifications(send)
    tracker.started('one', 'Quest One')
    tracker.completed('other'); tracker.completed('one'); tracker.completed('one')
    tracker.started('one', 'Quest One'); tracker.completed('one')
    expect(send.mock.calls.map(([e]) => e.kind)).toEqual(['quest_started', 'quest_completed', 'quest_started', 'quest_completed'])
    expect(new Set(send.mock.calls.map(([e]) => e.id)).size).toBe(4)
  })
  it('reports each item and one summary with failed and skipped counts', () => {
    const send = vi.fn()
    const tracker = new QuestNotifications(send)
    tracker.beginQueue(); tracker.started('one', 'One')
    tracker.completed('one'); tracker.completed('one'); tracker.skipped(); tracker.failed()
    tracker.started('two', 'Two'); tracker.completed('two'); tracker.drained(); tracker.drained()
    expect(send.mock.calls.filter(([e]) => e.kind === 'quest_completed')).toHaveLength(2)
    expect(send.mock.calls[send.mock.calls.length - 1]?.[0]).toMatchObject({ kind: 'queue_completed', succeeded: 2, failed: 1, skipped: 1 })
  })
  it('does not report empty, skipped-only, or startup-failure-only queues', () => {
    const send = vi.fn()
    const tracker = new QuestNotifications(send)
    tracker.beginQueue(); tracker.skipped(); tracker.drained()
    tracker.beginQueue(); tracker.failed(); tracker.drained(); tracker.drained()
    expect(send).not.toHaveBeenCalled()
  })
  it('adopts a manually running task when items are appended', () => {
    const send = vi.fn()
    const tracker = new QuestNotifications(send)
    tracker.started('one', 'One'); tracker.beginQueue(); tracker.completed('one'); tracker.drained()
    expect(send.mock.calls[send.mock.calls.length - 1]?.[0]).toMatchObject({ kind: 'queue_completed', succeeded: 1 })
  })
  it('does not send queue completion after a manual clear or stop', () => {
    const send = vi.fn()
    const tracker = new QuestNotifications(send)
    tracker.beginQueue(); tracker.started('one', 'One')
    tracker.abandonQueue(); tracker.completed('one'); tracker.drained()
    expect(send.mock.calls.map(([e]) => e.kind)).toEqual(['quest_started', 'quest_completed'])
    tracker.cancelActive(); tracker.beginQueue(); tracker.skipped(); tracker.drained()
    expect(send).toHaveBeenCalledTimes(2)
  })
  it('discards execution identity on logout or account change', () => {
    const send = vi.fn()
    const tracker = new QuestNotifications(send)
    tracker.beginQueue(); tracker.started('one', 'One'); tracker.reset()
    tracker.completed('one'); tracker.drained()
    expect(send).toHaveBeenCalledTimes(1)
  })
})
