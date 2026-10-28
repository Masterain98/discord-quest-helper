import type { UserBadge } from '@/api/userBadges'

export const ACCOUNT_BADGES = [
  { id: 14, kind: 'quests', unit: null },
  { id: 21, kind: 'games', unit: 'games' },
  { id: 20, kind: 'hours', unit: 'hours' },
] as const

export function badgeCount(badge: UserBadge | undefined, unit: string | null): number | null {
  const progress = Array.isArray(badge?.progress)
    ? badge.progress.find(p => p?.unit === unit) : undefined
  const current = progress?.current
  if (typeof current === 'number' && Number.isSafeInteger(current) && current >= 0) return current
  // Quests currently has no structured progress. Only parse the observed
  // English format; preserve unfamiliar/localized labels for display instead.
  if (badge?.badge_id !== 14 || typeof badge.info_label !== 'string') return null
  const match = badge.info_label.match(/^\s*(\d+(?:,\d{3})*)\s+Quests?\s+completed\s*$/i)
  if (!match) return null
  const count = Number(match[1].replace(/,/g, ''))
  return Number.isSafeInteger(count) ? count : null
}

export function badgeIcon(badge: UserBadge | undefined): string | null {
  for (const value of [badge?.complex_icon_static_url, badge?.simple_icon_raster_url]) {
    if (typeof value !== 'string') continue
    try {
      const url = new URL(value)
      if (url.protocol === 'https:' && url.hostname === 'cdn.discordapp.com') return url.href
    } catch { /* Fall back to the local icon when a CDN URL is absent or malformed. */ }
  }
  return null
}
