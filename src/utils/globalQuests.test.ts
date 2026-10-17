import { describe, expect, it } from 'vitest'
import type { Quest } from '@/api/tauri'
import { completedGlobalQuestIds, emptyGlobalQuestFilters, filterGlobalQuests, isAvailableInCountry,
  parseGlobalQuests, parseGlobalRegions, questAssetUrl, unknownRestriction } from './globalQuests'

const now = Date.parse('2026-10-08T00:00:00Z')
function raw(id = '111', extra: Record<string, unknown> = {}) {
  return { id, config: { messages: { quest_name: 'Sample', game_title: 'Game' },
    starts_at: '2026-10-01T00:00:00Z', expires_at: '2026-10-10T00:00:00Z', ...extra } }
}

describe('public catalog normalization', () => {
  it('reads array and wrapped datasets without rounding IDs or adding account status', () => {
    const entry = raw('1556984508107063417')
    expect(parseGlobalQuests([entry])).toEqual(parseGlobalQuests({ quests: [entry] }))
    expect(parseGlobalQuests([entry])[0].id).toBe(entry.id)
    expect(parseGlobalQuests([entry])[0]).not.toHaveProperty('user_status')
    expect(() => parseGlobalQuests({ bad: [] })).toThrow()
  })
  it('skips malformed rows, deduplicates exact IDs and retains same-name variants', () => {
    expect(parseGlobalQuests([null, {}, raw(), raw(), raw('222')]).map(q => q.id)).toEqual(['111', '222'])
  })
  it('prefers v2 tasks, classifies multiple types and retains unknown tasks', () => {
    const quest = parseGlobalQuests([raw('111', { task_config: { tasks: { STREAM_ON_DESKTOP: {} } },
      task_config_v2: { tasks: { WATCH_VIDEO: { target: 120 }, PLAY_ON_XBOX: {}, ACHIEVEMENT_IN_ACTIVITY: {}, mystery: {} } } })])[0]
    expect(quest.tasks.map(t => t.kind)).toEqual(['watch', 'play', 'activity', 'other'])
    expect(quest.tasks[0].target).toBe(120)
  })
  it('does not classify Nitro with an image as an in-game reward', () => {
    const quest = parseGlobalQuests([raw('111', { rewards_config: { rewards: [
      { type: 5, asset: 'reward.png' }, { type: 4, orb_quantity: 200 }, { type: 3 }, { type: 1 }, { type: 2 }, { type: 999 },
    ] } })])[0]
    expect(quest.rewards.map(r => r.kind)).toEqual(['nitro', 'orbs', 'avatar', 'ingame', 'ingame', 'other'])
  })
  it('handles malformed optional metadata and unsafe assets', () => {
    const quest = parseGlobalQuests([raw('111', { starts_at: 'bad', expires_at: 'bad', task_config: null, rewards_config: { rewards: [null] } })])[0]
    expect(quest.startsAt).toBeNull(); expect(quest.expiresAt).toBeNull()
    expect(questAssetUrl('quests/111/image.jpg', '111')).toBe('https://cdn.discordapp.com/quests/111/image.jpg')
    expect(questAssetUrl('hero.png', '111')).toBe('https://cdn.discordapp.com/quests/111/hero.png')
    expect(questAssetUrl('https://example.com/image.png', '111')).toBeNull()
    expect(questAssetUrl('../image.png', '111')).toBeNull()
  })
})

describe('region and age rules', () => {
  it('supports both formats and normalizes UK to GB', () => {
    const rules = parseGlobalRegions({ quests: [
      { id: '111', is_global: false, show_age_gate: true, regions: ['UK'] },
      { id: '222', is_global: false, show_age_gate: false, regions: { include: ['GB'], exclude: [] } },
    ] })
    expect(rules['111']).toEqual({ region: 'restricted', include: ['GB'], exclude: [], age: 'adult' })
    expect(rules['222'].age).toBe('unmarked')
    expect(isAvailableInCountry(rules['111'], 'UK')).toBe(true)
    expect(isAvailableInCountry(rules['111'], 'US')).toBe(false)
  })
  it('includes global entries and honors exclusions over inclusions', () => {
    const rules = parseGlobalRegions([
      { id: '111', is_global: true, regions: [] },
      { id: '222', is_global: false, regions: { include: [], exclude: ['JP', 'KR'] } },
      { id: '333', is_global: true, regions: { include: ['US'], exclude: ['US'] } },
    ])
    expect(isAvailableInCountry(rules['111'], 'TW')).toBe(true)
    expect(isAvailableInCountry(rules['222'], 'TW')).toBe(true)
    expect(isAvailableInCountry(rules['222'], 'JP')).toBe(false)
    expect(isAvailableInCountry(rules['333'], 'US')).toBe(false)
  })
  it('keeps absent, empty non-global and malformed rules unknown', () => {
    const rules = parseGlobalRegions([
      { id: '111', is_global: false, regions: [] },
      { id: '222', is_global: true, regions: { include: ['not a country'], exclude: [] }, show_age_gate: false },
      { id: '333', is_global: true, regions: null },
      { id: '444', is_global: false, regions: ['XX'] },
    ])
    expect(rules['111']).toEqual(unknownRestriction())
    expect(rules['222'].region).toBe('unknown'); expect(rules['222'].age).toBe('unmarked')
    expect(rules['333'].region).toBe('unknown')
    expect(rules['444'].region).toBe('unknown')
    expect(isAvailableInCountry(unknownRestriction(), 'US')).toBe(false)
  })
})

describe('catalog filters', () => {
  it('places completed quests last while preserving newest-first order within each group', () => {
    const quests = parseGlobalQuests([
      raw('111', { starts_at: '2026-10-07T00:00:00Z' }),
      raw('222', { starts_at: '2026-10-05T00:00:00Z' }),
      raw('333', { starts_at: '2026-10-04T00:00:00Z' }),
      raw('444', { starts_at: '2026-10-06T00:00:00Z' }),
    ])
    const completed = new Set(['111', '333'])
    expect(filterGlobalQuests(quests, {}, emptyGlobalQuestFilters(), now, completed).map(q => q.id)).toEqual(['444', '222', '111', '333'])
    expect(filterGlobalQuests(quests, {}, emptyGlobalQuestFilters(), now).map(q => q.id)).toEqual(['111', '444', '222', '333'])
    expect(quests.map(q => q.id)).toEqual(['111', '222', '333', '444'])
  })

  it('excludes expired, exact-deadline and unknown-expiry entries, retaining upcoming tasks', () => {
    const quests = parseGlobalQuests([raw(), raw('222', { expires_at: '2026-10-08T00:00:00Z' }),
      raw('333', { starts_at: '2026-10-09T00:00:00Z' }), raw('444', { expires_at: null })])
    expect(filterGlobalQuests(quests, {}, emptyGlobalQuestFilters(), now).map(q => q.id)).toEqual(['333', '111'])
  })
  it('uses OR within groups and AND between groups with every task and reward', () => {
    const quests = parseGlobalQuests([raw('111', { task_config_v2: { tasks: { WATCH_VIDEO: {}, PLAY_ON_XBOX: {} } },
      rewards_config: { rewards: [{ type: 4 }, { type: 5 }] } }), raw('222')])
    const filters = { ...emptyGlobalQuestFilters(), tasks: ['stream', 'play'] as const, rewards: ['nitro'] as const }
    expect(filterGlobalQuests(quests, {}, { ...filters, tasks: [...filters.tasks], rewards: [...filters.rewards] }, now).map(q => q.id)).toEqual(['111'])
    expect(filterGlobalQuests(quests, {}, { ...emptyGlobalQuestFilters(), tasks: ['play'], rewards: ['avatar'] }, now)).toEqual([])
  })
  it('combines country, status, age and case-insensitive search, excluding unknown availability', () => {
    const quests = parseGlobalQuests([raw('111'), raw('222'), raw('333')])
    const rules = parseGlobalRegions([
      { id: '111', is_global: true, show_age_gate: true, regions: [] },
      { id: '222', is_global: false, show_age_gate: false, regions: ['US'] },
    ])
    const filters = { ...emptyGlobalQuestFilters(), country: 'US', query: ' gAME ' }
    expect(filterGlobalQuests(quests, rules, filters, now).map(q => q.id)).toEqual(['222', '111'])
    expect(filterGlobalQuests(quests, rules, { ...filters, ages: ['adult'], regions: ['global'] }, now).map(q => q.id)).toEqual(['111'])
  })
})

describe('account completion', () => {
  const quests = [{ ...raw('111'), user_status: { completed_at: 'done', user_id: 'account' } },
    { ...raw('222'), user_status: { claimed_at: 'claimed' } },
    { ...raw('333'), user_status: { completed_at: 'done', user_id: 'old-account' } }] as Quest[]
  it('marks exact quest IDs for completed and claimed rewards, never same-name variants', () => {
    expect([...completedGlobalQuestIds(quests, 'account', 'account')]).toEqual(['111', '222'])
  })
  it('clears completion for logged-out and changed accounts', () => {
    expect(completedGlobalQuestIds(quests, null, 'account').size).toBe(0)
    expect(completedGlobalQuestIds(quests, 'new-account', 'account').size).toBe(0)
  })
})
