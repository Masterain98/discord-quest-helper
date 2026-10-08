import type { Quest } from '@/api/tauri'
import { GLOBAL_COUNTRY_CODES } from './globalCountries'
import type {
  GlobalQuest, GlobalQuestFilters, GlobalQuestRestriction, GlobalRewardKind, GlobalTaskKind,
} from '@/types/globalQuests'

const countryCodes = new Set(GLOBAL_COUNTRY_CODES)

function object(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown> : {}
}
function text(value: unknown): string { return typeof value === 'string' ? value : '' }
function date(value: unknown): string | null {
  return typeof value === 'string' && Number.isFinite(Date.parse(value)) ? value : null
}
function number(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null
}
function collection(value: unknown): unknown[] {
  const items = Array.isArray(value) ? value : object(value).quests
  if (!Array.isArray(items)) throw new Error('Invalid quest dataset')
  return items
}

export function normalizeCountry(value: string): string {
  const country = value.trim().toUpperCase()
  return country === 'UK' ? 'GB' : country
}

export function questAssetUrl(value: unknown, questId: string): string | null {
  if (typeof value !== 'string' || !value.trim()) return null
  // Older entries use a filename; newer entries use a CDN-relative path.
  if (/^https:\/\//i.test(value)) {
    try { const url = new URL(value); return url.hostname === 'cdn.discordapp.com' ? url.href : null } catch { return null }
  }
  if (/[:?#\\]/.test(value) || value.split('/').includes('..')) return null
  const path = value.replace(/^\/+/, '')
  return `https://cdn.discordapp.com/${path.includes('/') ? path : `quests/${questId}/${path}`}`
}

export function globalTaskKind(type: string): GlobalTaskKind {
  if (type.includes('VIDEO')) return 'watch'
  if (type.includes('ACTIVITY') || type.includes('ACHIEVEMENT')) return 'activity'
  if (type.includes('STREAM')) return 'stream'
  if (type.includes('PLAY')) return 'play'
  return 'other'
}

function rewardKind(reward: Record<string, unknown>): GlobalRewardKind {
  if (reward.type === 4 || number(reward.orb_quantity) !== null) return 'orbs'
  if (reward.type === 3) return 'avatar'
  if (reward.type === 1 || reward.type === 2) return 'ingame'
  if (reward.type === 5) return 'nitro'
  return 'other'
}

export function parseGlobalQuests(value: unknown): GlobalQuest[] {
  const quests = new Map<string, GlobalQuest>()
  for (const item of collection(value)) {
    const raw = object(item), config = object(raw.config), messages = object(config.messages)
    const id = text(raw.id)
    if (!/^\d+$/.test(id) || !text(messages.quest_name)) continue
    const tasks = object(object(config.task_config_v2).tasks ?? object(config.task_config).tasks)
    const rewards = object(config.rewards_config).rewards
    quests.set(id, {
      id, name: text(messages.quest_name),
      gameName: text(messages.game_title) || text(object(config.application).name),
      description: text(messages.task_description),
      startsAt: date(config.starts_at), expiresAt: date(config.expires_at),
      thumbnailUrl: questAssetUrl(object(config.assets).hero, id),
      tasks: Object.entries(tasks).map(([key, task]) => {
        const entry = object(task), type = text(entry.type) || text(entry.event_name) || key
        return { type, kind: globalTaskKind(type), target: number(entry.target) }
      }),
      rewards: (Array.isArray(rewards) ? rewards : []).map(item => {
        const reward = object(item), kind = rewardKind(reward)
        return {
          kind, name: text(object(reward.messages).name),
          quantity: number(kind === 'orbs' ? reward.orb_quantity : reward.quantity),
          premiumQuantity: number(reward.premium_orb_quantity),
          assetUrl: questAssetUrl(reward.asset, id), expiresAt: date(reward.expires_at),
          instructions: [...new Set(Object.values(object(object(reward.messages).redemption_instructions_by_platform))
            .filter((instruction): instruction is string => typeof instruction === 'string' && instruction !== 'PLACEHOLDER'))],
        }
      }),
    })
  }
  return [...quests.values()]
}

export function unknownRestriction(): GlobalQuestRestriction {
  return { region: 'unknown', include: [], exclude: [], age: 'unknown' }
}

export function parseGlobalRegions(value: unknown): Record<string, GlobalQuestRestriction> {
  const result: Record<string, GlobalQuestRestriction> = {}
  for (const item of collection(value)) {
    const raw = object(item), id = text(raw.id)
    if (!/^\d+$/.test(id)) continue
    const regions = raw.regions, rules = object(regions)
    const include = Array.isArray(regions) ? regions : rules.include
    const exclude = Array.isArray(regions) ? [] : rules.exclude
    const valid = (codes: unknown): codes is string[] => Array.isArray(codes)
      && codes.every(code => typeof code === 'string' && countryCodes.has(normalizeCountry(code)))
    const restriction = unknownRestriction()
    restriction.age = raw.show_age_gate === true ? 'adult' : raw.show_age_gate === false ? 'unmarked' : 'unknown'
    if (valid(include) && valid(exclude)) {
      restriction.include = [...new Set(include.map(normalizeCountry))]
      restriction.exclude = [...new Set(exclude.map(normalizeCountry))]
      if (include.length || exclude.length) restriction.region = 'restricted'
      else if (raw.is_global === true) restriction.region = 'global'
    }
    result[id] = restriction
  }
  return result
}

export function emptyGlobalQuestFilters(): GlobalQuestFilters {
  return { query: '', tasks: [], rewards: [], regions: [], ages: [], country: '' }
}

export function isAvailableInCountry(restriction: GlobalQuestRestriction, country: string): boolean {
  if (restriction.region === 'unknown') return false
  const code = normalizeCountry(country)
  return !restriction.exclude.includes(code)
    && (restriction.include.length === 0 || restriction.include.includes(code))
}

export function filterGlobalQuests(
  quests: GlobalQuest[], restrictions: Record<string, GlobalQuestRestriction>, filters: GlobalQuestFilters, now: number,
  completedIds: ReadonlySet<string> = new Set(),
  acceptedIds: ReadonlySet<string> = new Set(),
): GlobalQuest[] {
  const query = filters.query.trim().toLocaleLowerCase()
  return quests.filter(quest => {
    if (!quest.expiresAt || Date.parse(quest.expiresAt) <= now) return false
    const restriction = restrictions[quest.id] ?? unknownRestriction()
    return (!query || `${quest.name} ${quest.gameName}`.toLocaleLowerCase().includes(query))
      && (!filters.tasks.length || filters.tasks.some(kind => (quest.tasks.length ? quest.tasks.some(task => task.kind === kind) : kind === 'other')))
      && (!filters.rewards.length || filters.rewards.some(kind => (quest.rewards.length ? quest.rewards.some(reward => reward.kind === kind) : kind === 'other')))
      && (!filters.regions.length || filters.regions.includes(restriction.region))
      && (!filters.ages.length || filters.ages.includes(restriction.age))
      && (!filters.country || isAvailableInCountry(restriction, filters.country))
  }).sort((a, b) => {
    const statusOrder = (id: string) => completedIds.has(id) ? 2 : acceptedIds.has(id) ? 1 : 0
    return statusOrder(a.id) - statusOrder(b.id)
      || (Date.parse(b.startsAt ?? '') || 0) - (Date.parse(a.startsAt ?? '') || 0) || b.id.localeCompare(a.id)
  })
}

export function completedGlobalQuestIds(quests: Quest[], accountId: string | null, ownerId: string | null): Set<string> {
  if (!accountId || accountId !== ownerId) return new Set()
  return new Set(quests.filter(quest => (!quest.user_status?.user_id || quest.user_status.user_id === accountId)
    && (quest.user_status?.completed_at || quest.user_status?.claimed_at)).map(quest => quest.id))
}

export function acceptedGlobalQuestIds(quests: Quest[], accountId: string | null, ownerId: string | null): Set<string> {
  if (!accountId || accountId !== ownerId) return new Set()
  return new Set(quests.filter(quest => (!quest.user_status?.user_id || quest.user_status.user_id === accountId)
    && Boolean(quest.user_status?.enrolled_at)
    && !quest.user_status?.completed_at && !quest.user_status?.claimed_at).map(quest => quest.id))
}
