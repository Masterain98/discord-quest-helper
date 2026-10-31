import type { QuestNotification } from '@/api/notifications'
import type { Quest } from '@/api/tauri'
import { getQuestRewardViews } from './questRewards'

export function notificationRewardImage(quest: Quest): string | null {
  const rewards = getQuestRewardViews(quest)
  const images = rewards.filter(reward => reward.asset && !/\.(mp4|webm)(\?|$)/i.test(reward.asset))
  const asset = (images.find(reward => reward.kind === 'collectible') ?? images.find(reward => reward.kind !== 'orbs') ?? images[0])?.asset
  if (asset) return /^https:\/\//i.test(asset) ? asset : `https://cdn.discordapp.com/${asset.replace(/^\/+/, '')}`
  return rewards.some(reward => reward.kind === 'orbs') ? '/icons/orbs.png' : null
}

export function estimatedCompletionAt(remainingSeconds: number, speed = 1, now = Date.now()): string | null {
  if (!Number.isFinite(remainingSeconds) || !Number.isFinite(speed) || speed <= 0) return null
  const timestamp = now + Math.ceil(Math.max(0, remainingSeconds) / speed) * 1000
  if (!Number.isFinite(timestamp) || Math.abs(timestamp) > 8.64e15) return null
  return new Date(timestamp).toISOString()
}

type QuestContext = Pick<QuestNotification, 'reward_image' | 'estimated_complete_at'>

/** Execution identity, separate from quest ID: retries and queue runs are distinct. */
export class QuestNotifications {
  private active: { id: string; quest: string; name: string; completed: boolean; context: QuestContext } | null = null
  private queue: { id: string; executed: number; succeeded: number; failed: number; skipped: number } | null = null
  constructor(private readonly send: (event: QuestNotification) => void) {}

  started(quest: string, name: string, context: QuestContext = {}) {
    this.active = { id: crypto.randomUUID(), quest, name, completed: false, context: { ...context } }
    if (this.queue) this.queue.executed++
    this.send({ ...context, id: `${this.active.id}:started`, kind: 'quest_started', quest_name: name })
  }
  completed(quest: string) {
    if (!this.active || this.active.quest !== quest || this.active.completed) return
    this.active.completed = true
    if (this.queue) this.queue.succeeded++
    this.send({ reward_image: this.active.context.reward_image, id: `${this.active.id}:completed`, kind: 'quest_completed', quest_name: this.active.name })
  }
  beginQueue() {
    if (!this.queue) this.queue = {
      id: crypto.randomUUID(), executed: this.active && !this.active.completed ? 1 : 0,
      succeeded: 0, failed: 0, skipped: 0,
    }
  }
  skipped() { if (this.queue) this.queue.skipped++ }
  failed() { if (this.queue) this.queue.failed++; this.active = null }
  drained() {
    const queue = this.queue
    this.queue = null
    if (!queue || queue.executed === 0) return
    this.send({ id: `${queue.id}:completed`, kind: 'queue_completed', succeeded: queue.succeeded, failed: queue.failed, skipped: queue.skipped })
  }
  abandonQueue() { this.queue = null }
  cancelActive() { this.active = null }
  reset() { this.active = null; this.queue = null }
}
