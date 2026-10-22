import { describe, expect, it } from 'vitest'
import { badgeCount, badgeIcon } from './userBadges'

describe('Discord badge metrics', () => {
  it('uses the structured value in its stated unit, including zero and maxed-out hours', () => {
    expect(badgeCount({ badge_id: 20, progress: [{ current: 5263, unit: 'hours' }] }, 'hours')).toBe(5263)
    expect(badgeCount({ badge_id: 21, progress: [{ current: 0, unit: 'games' }] }, 'games')).toBe(0)
    expect(badgeCount({ badge_id: 20, progress: [{ current: 60, unit: 'minutes' }] }, 'hours')).toBeNull()
    expect(badgeCount({ badge_id: 21, progress: [{ current: -1, unit: 'games' }] }, 'games')).toBeNull()
    expect(badgeCount(undefined, 'games')).toBeNull()
  })

  it('parses only the observed Quests count format without treating unfamiliar labels as zero', () => {
    expect(badgeCount({ badge_id: 14, info_label: '499 Quests completed' }, null)).toBe(499)
    expect(badgeCount({ badge_id: 14, info_label: '1,234 Quests completed' }, null)).toBe(1234)
    expect(badgeCount({ badge_id: 14, info_label: '0 Quests completed' }, null)).toBe(0)
    expect(badgeCount({ badge_id: 14, info_label: '完成 499 个任务' }, null)).toBeNull()
    expect(badgeCount({ badge_id: 14, info_label: '1,23 Quests completed' }, null)).toBeNull()
  })

  it('uses returned Discord CDN icons and handles missing or malformed resources', () => {
    expect(badgeIcon({ badge_id: 20, complex_icon_static_url: 'https://cdn.discordapp.com/complex.svg', simple_icon_raster_url: 'https://cdn.discordapp.com/badge.png' }))
      .toBe('https://cdn.discordapp.com/complex.svg')
    expect(badgeIcon({ badge_id: 14, complex_icon_static_url: 'broken', simple_icon_raster_url: 'https://cdn.discordapp.com/badge.png' }))
      .toBe('https://cdn.discordapp.com/badge.png')
    expect(badgeIcon({ badge_id: 14, simple_icon_raster_url: 'https://cdn.discordapp.com/badge.png' }))
      .toBe('https://cdn.discordapp.com/badge.png')
    expect(badgeIcon({ badge_id: 14, simple_icon_raster_url: 'https://unrelated.example/badge.png' })).toBeNull()
    expect(badgeIcon(undefined)).toBeNull()
  })
})
