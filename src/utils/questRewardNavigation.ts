import type { ToastOptions } from '@/stores/toast'
import { navigateToTab } from '@/utils/navigate'

interface RewardNavigationOptions {
  cdpAvailable: boolean
  cdpPort: number
  navigate: (path: string, port: number) => Promise<void>
}

/** Open the client's reward page; rewards are always claimed manually in Discord. */
export async function openRewardQuest(
  id: string,
  options: RewardNavigationOptions,
): Promise<'manual' | 'navigated'> {
  if (!options.cdpAvailable) return 'manual'
  await options.navigate(`/quest-home#${encodeURIComponent(id)}`, options.cdpPort)
  return 'navigated'
}

export function discordIntegrationAction(t: (key: string) => string) {
  return {
    label: t('toast.open_discord_settings'),
    onClick: () => navigateToTab('settings', 'discord_integration'),
  }
}

export function manualRewardClaimNotice(t: (key: string) => string): ToastOptions {
  return {
    title: t('toast.manual_claim'),
    description: t('toast.manual_claim_hint'),
    actions: [discordIntegrationAction(t)],
  }
}
