import { parseGlobalQuests, parseGlobalRegions } from '@/utils/globalQuests'

const BASE_URL = 'https://api.discordquest.com'
export const GLOBAL_QUEST_TIMEOUT = 20_000

async function getDataset(path: '/api/quests' | '/api/regions'): Promise<unknown> {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), GLOBAL_QUEST_TIMEOUT)
  try {
    const response = await fetch(`${BASE_URL}${path}`, {
      credentials: 'omit', cache: 'no-store', signal: controller.signal, headers: { Accept: 'application/json' },
    })
    if (!response.ok) throw new Error(`HTTP ${response.status}`)
    return await response.json()
  } finally {
    clearTimeout(timeout)
  }
}

export function fetchGlobalQuestCatalog() { return getDataset('/api/quests').then(parseGlobalQuests) }
export function fetchGlobalQuestRegions() { return getDataset('/api/regions').then(parseGlobalRegions) }
