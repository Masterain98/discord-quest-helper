import { describe, expect, it } from 'vitest'
import type { Quest } from '@/api/tauri'
import { canQueueGroupMember, groupProgress, groupQuests, groupStats, hasLinkedQuestProgress, questGroupKey } from './questGroups'

function quest(id: string, name = 'Clean Crew', types = ['PLAY_ON_DESKTOP']): Quest {
  return {
    id,
    config: { messages: { quest_name: name, game_title: 'Roblox' }, task_config_v2: {
      tasks: Object.fromEntries(types.map(type => [type, { type, target: 900 }])),
    } },
    user_status: { enrolled_at: '2026-01-01' },
  }
}

describe('quest display groups', () => {
  it('preserves first occurrence, member order, input and unique IDs', () => {
    const input = [quest('a'), quest('b', 'Other'), quest('c'), quest('a')]
    const before = JSON.stringify(input)
    expect(groupQuests(input).map(group => group.quests.map(q => q.id))).toEqual([['a', 'c'], ['b']])
    expect(JSON.stringify(input)).toBe(before)
    expect(groupQuests(input.filter(q => q.id !== 'c'))[0].quests).toHaveLength(1)
  })

  it('matches exact names and specific type sets regardless of task order or schema version', () => {
    const a = quest('a', 'Name', ['WATCH_VIDEO', 'WATCH_VIDEO_ON_MOBILE'])
    const b = quest('b', 'Name', ['WATCH_VIDEO_ON_MOBILE', 'WATCH_VIDEO'])
    b.config.task_config = b.config.task_config_v2
    delete b.config.task_config_v2
    expect(questGroupKey(a)).toBe(questGroupKey(b))
    expect(questGroupKey(quest('c', 'name'))).not.toBe(questGroupKey(quest('d', 'Name')))
    expect(questGroupKey(quest('c', 'Name '))).not.toBe(questGroupKey(quest('d', 'Name')))
    expect(questGroupKey(quest('c', 'Name', ['PLAY_ON_XBOX']))).not.toBe(questGroupKey(quest('d', 'Name')))
    b.config.messages.game_title = 'Other'
    expect(questGroupKey(a)).not.toBe(questGroupKey(b))
  })

  it('keeps missing grouping fields independent', () => {
    const a = quest('a'); const b = quest('b')
    delete a.config.messages.game_title
    delete b.config.messages.game_title
    expect(groupQuests([a, b])).toHaveLength(2)
    expect(questGroupKey(quest('empty', ''))).toBeNull()
    expect(questGroupKey(quest('no-tasks', 'Name', []))).toBeNull()
  })

  it('links only eligible members without changing server progress or target', () => {
    const active = quest('active'); const member = quest('member')
    member.config.task_config_v2!.tasks!.PLAY_ON_DESKTOP.target = 1800
    member.user_status!.progress = { PLAY_ON_DESKTOP: { value: 20 } }
    const before = JSON.stringify(member)
    expect(hasLinkedQuestProgress(member, active)).toBe(true)
    expect(JSON.stringify(member)).toBe(before)
    expect(hasLinkedQuestProgress(member, null)).toBe(false)
    expect(hasLinkedQuestProgress(member, quest('other', 'Other'))).toBe(false)
    for (const status of [null, { enrolled_at: 'x', completed_at: 'x' }, { enrolled_at: 'x', claimed_at: 'x' }]) {
      member.user_status = status
      expect(hasLinkedQuestProgress(member, active)).toBe(false)
    }
    member.user_status = { enrolled_at: 'x' }
    member.config.expires_at = '2020-01-01'
    expect(hasLinkedQuestProgress(member, active)).toBe(false)
    expect(hasLinkedQuestProgress(active, { ...active, user_status: { completed_at: 'x' } })).toBe(false)
  })

  it('counts server states and only queues supported, accepted, unfinished tasks', () => {
    const ready = quest('ready')
    const done = quest('done'); done.user_status!.completed_at = 'x'
    const unaccepted = quest('unaccepted'); unaccepted.user_status = null
    expect(groupStats([ready, done, unaccepted])).toEqual({ accepted: 2, completed: 1, claimable: 1 })
    expect(canQueueGroupMember(ready)).toBe(true)
    for (const item of [done, unaccepted, quest('activity', 'Name', ['PLAY_ACTIVITY']), quest('xbox', 'Name', ['PLAY_ON_XBOX']), quest('stream', 'Name', ['STREAM_ON_DESKTOP'])]) {
      expect(canQueueGroupMember(item)).toBe(false)
    }
  })

  it('keeps 7/9 incomplete and averages the two remaining members independently', () => {
    const members = Array.from({ length: 9 }, (_, index) => quest(String(index)))
    members.slice(0, 7).forEach(q => { q.user_status!.completed_at = 'confirmed' })
    members[7].user_status!.progress = { PLAY_ON_DESKTOP: { value: 180 } }
    members[8].user_status!.progress = { PLAY_ON_DESKTOP: { value: 720 } }
    const progress = groupProgress(members)
    expect(progress.completed).toBe(7)
    expect(progress.total).toBe(9)
    expect(progress.isCompleted).toBe(false)
    expect(progress.percentage).toBeCloseTo(800 / 9)
    // Only the executing member receives its live confirmed percentage.
    expect(groupProgress(members, { questId: '7', percentage: 50 }).percentage).toBeCloseTo(830 / 9)
  })

  it('requires server confirmation for every ID even when all counters reach 100%', () => {
    const members = [quest('a'), quest('b')]
    members.forEach(q => { q.user_status!.progress = { PLAY_ON_DESKTOP: { value: 900 } } })
    expect(groupProgress(members)).toEqual({ completed: 0, total: 2, percentage: 100, isCompleted: false })
    members[0].user_status!.completed_at = 'confirmed'
    expect(groupProgress(members).isCompleted).toBe(false)
    members[1].user_status!.claimed_at = 'claimed'
    expect(groupProgress(members)).toEqual({ completed: 2, total: 2, percentage: 100, isCompleted: true })
  })

  it('averages percentages with individual targets and safely handles missing or invalid values', () => {
    const a = quest('a'); const b = quest('b')
    a.user_status!.progress = { PLAY_ON_DESKTOP: { value: 450 } }
    b.config.task_config_v2!.tasks!.PLAY_ON_DESKTOP.target = 1800
    b.user_status!.progress = { PLAY_ON_DESKTOP: { value: 900 } }
    expect(groupProgress([a, b]).percentage).toBe(50)
    expect(groupProgress([])).toEqual({ completed: 0, total: 0, percentage: 0, isCompleted: false })
    a.user_status!.progress.PLAY_ON_DESKTOP.value = Infinity
    b.user_status = null
    expect(groupProgress([a, b]).percentage).toBe(0)
  })
})
