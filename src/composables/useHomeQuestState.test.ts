import { describe, expect, it } from 'vitest'
import type { Quest } from '@/api/tauri'
import { deriveHomeQuestBuckets } from './useHomeQuestState'

function quest(overrides: Partial<Quest> & { id: string }): Quest {
  return {
    id: overrides.id,
    config: {
      messages: {
        quest_name: overrides.config?.messages?.quest_name ?? overrides.id,
        game_title: overrides.config?.messages?.game_title ?? 'Game',
      },
      expires_at: overrides.config?.expires_at ?? '2026-06-22T00:00:00.000Z',
      task_config_v2: overrides.config?.task_config_v2 ?? {
        tasks: {
          task: { type: 'WATCH_VIDEO', target: 60 },
        },
      },
      ...overrides.config,
    },
    user_status: overrides.user_status ?? null,
  }
}

describe('deriveHomeQuestBuckets', () => {
  const now = new Date('2026-06-21T00:00:00.000Z')

  it('groups quests by next user-facing action', () => {
    const buckets = deriveHomeQuestBuckets([
      quest({ id: 'accept' }),
      quest({ id: 'run', user_status: { enrolled_at: '2026-06-20T00:00:00.000Z' } }),
      quest({ id: 'claim', user_status: { enrolled_at: '2026-06-20T00:00:00.000Z', completed_at: '2026-06-20T01:00:00.000Z' } }),
      quest({ id: 'done', user_status: { enrolled_at: '2026-06-20T00:00:00.000Z', completed_at: '2026-06-20T01:00:00.000Z', claimed_at: '2026-06-20T02:00:00.000Z' } }),
      quest({ id: 'expired', config: { messages: { quest_name: 'Expired' }, expires_at: '2026-06-20T00:00:00.000Z' } }),
    ], { now })

    expect(buckets.toAccept.map(item => item.id)).toEqual(['accept'])
    expect(buckets.readyToRun.map(item => item.id)).toEqual(['run'])
    expect(buckets.readyToClaim.map(item => item.id)).toEqual(['claim'])
    expect(buckets.completed.map(item => item.id)).toEqual(['claim', 'done'])
    expect(buckets.expired.map(item => item.id)).toEqual(['expired'])
  })

  it('marks activity quests as needing attention when CDP is unavailable', () => {
    const activityQuest = quest({
      id: 'activity',
      user_status: { enrolled_at: '2026-06-20T00:00:00.000Z' },
      config: {
        messages: { quest_name: 'Activity' },
        task_config_v2: {
          tasks: {
            task: { type: 'ACHIEVEMENT_IN_ACTIVITY', target: 3 },
          },
        },
      },
    })

    const unavailable = deriveHomeQuestBuckets([activityQuest], { now, cdpAvailable: false })
    const available = deriveHomeQuestBuckets([activityQuest], { now, cdpAvailable: true })

    expect(unavailable.attentionNeeded.map(item => item.id)).toEqual(['activity'])
    expect(unavailable.readyToRun).toHaveLength(0)
    expect(available.readyToRun.map(item => item.id)).toEqual(['activity'])
  })
})
