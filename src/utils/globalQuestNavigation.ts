import type { GameQuestMode } from '@/api/tauri'

interface QuestNavigationOptions {
  mode: GameQuestMode
  cdpPort: number
  navigate: (path: string, port: number) => Promise<void>
  openExternal: (url: string) => Promise<void>
}

export async function openGlobalQuest(id: string, options: QuestNavigationOptions): Promise<void> {
  if (!/^\d+$/.test(id)) throw new Error('Invalid quest ID')
  if (options.mode === 'cdp') {
    // Reuse the client route used by the existing activity-quest details action.
    await options.navigate(`/quest-home#${id}`, options.cdpPort)
  } else {
    await options.openExternal(`https://discord.com/quests/${id}`)
  }
}
