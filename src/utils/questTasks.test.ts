import { describe, expect, it } from 'vitest'
import type { Quest } from '@/api/tauri'
import en from '@/locales/en.json'
import zh from '@/locales/zh.json'
import {
  firstProgressValue,
  firstStartableTask,
  getQuestKind,
  getQuestTasks,
  isPlayActivityTask,
} from './questTasks'

function playActivityQuest(): Quest {
  return {
    id: 'cloud-activity',
    config: {
      messages: { quest_name: 'Cloud Activity' },
      task_config_v2: {
        tasks: {
          PLAY_ACTIVITY: { type: 'PLAY_ACTIVITY', target: 900 },
        },
      },
    },
    user_status: {
      enrolled_at: '2026-08-04T00:00:00.000Z',
      progress: {
        PLAY_ACTIVITY: { value: 48 },
      },
    },
  }
}

describe('PLAY_ACTIVITY task helpers', () => {
  it('keeps cloud games in the Activity kind with a distinct task predicate', () => {
    const quest = playActivityQuest()
    const task = firstStartableTask(quest)

    expect(getQuestKind(quest)).toBe('activity')
    expect(task?.type).toBe('PLAY_ACTIVITY')
    expect(task && isPlayActivityTask(task)).toBe(true)
    expect(getQuestTasks(quest)[0].label).toBe('Activity - Cloud Game')
  })

  it('restores PLAY_ACTIVITY progress as elapsed seconds', () => {
    const quest = playActivityQuest()

    expect(firstProgressValue(quest, 'PLAY_ACTIVITY')).toBe(48)
  })

  it('provides localized cloud-game badge labels', () => {
    expect(en.filter.activity_cloud_game).toBe('Activity - Cloud Game')
    expect(zh.filter.activity_cloud_game).toBe('活动 - 云游戏')
  })
})
