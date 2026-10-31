import { invoke } from '@tauri-apps/api/core'

export interface UserBadge {
  badge_id: number
  owned?: boolean
  name?: string
  info_label?: string | null
  complex_icon_static_url?: string | null
  simple_icon_url?: string | null
  simple_icon_raster_url?: string | null
  progress?: Array<{ current?: number | null; unit?: string | null }> | null
}

export async function getCurrentUserBadges(): Promise<UserBadge[]> {
  const response = await invoke<{ badges?: UserBadge[] } | null>('get_current_user_badges')
  if (!Array.isArray(response?.badges)) throw new Error('Invalid user badges response')
  return response.badges.filter(badge => badge && typeof badge.badge_id === 'number')
}
