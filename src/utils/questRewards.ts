import type { Quest, QuestReward, QuestUserStatus } from '@/api/tauri'

export type QuestRewardKind = 'orbs' | 'collectible' | 'ingame' | 'discord'

export interface QuestRewardView {
  kind: QuestRewardKind
  name: string
  asset: string | null
  skuId: string
  type: number
  amountText: string
  badgeText: string | null
  claimed: boolean
  icon: 'orbs' | 'asset' | 'gift'
}

export function isOrbReward(reward: QuestReward): boolean {
  return reward.type === 4 || reward.orb_quantity != null
}

export function isCollectibleReward(reward: QuestReward): boolean {
  const name = (reward.messages?.name || '').toLowerCase()
  return reward.type === 3
    || name.includes('decoration')
    || name.includes('avatar')
    || name.includes('profile')
}

export function isInGameReward(reward: QuestReward): boolean {
  return reward.type === 2 || (!!reward.asset && !isOrbReward(reward) && !isCollectibleReward(reward))
}

export function getPremiumMultiplier(reward: QuestReward): number | null {
  if (!reward.orb_quantity || !reward.premium_orb_quantity) return null
  if (reward.premium_orb_quantity <= reward.orb_quantity) return null
  return reward.premium_orb_quantity / reward.orb_quantity
}

function formatMultiplier(multiplier: number): string {
  return `${Number(multiplier.toFixed(2)).toString()}x`
}

export function formatQuestReward(reward: QuestReward, userStatus?: QuestUserStatus | null, userPremiumType?: number | null): QuestRewardView {
  const name = reward.messages?.name || 'Reward'
  const hasNitro = !!userPremiumType && userPremiumType > 0
  const multiplier = hasNitro ? getPremiumMultiplier(reward) : null
  const claimedOrbs = userStatus?.orb_quantity_claimed
  const claimed = !!userStatus?.claimed_at

  if (isOrbReward(reward)) {
    const base = reward.orb_quantity
    const premium = reward.premium_orb_quantity
    const amountText = claimed && claimedOrbs != null
      ? `Claimed ${claimedOrbs.toLocaleString()} Orbs`
      : hasNitro && base != null && premium != null && premium > base
        ? `${base.toLocaleString()} -> ${premium.toLocaleString()} Orbs`
        : base != null
          ? `${base.toLocaleString()} Orbs`
          : name

    return {
      kind: 'orbs',
      name,
      asset: reward.asset || null,
      skuId: reward.sku_id,
      type: reward.type,
      amountText,
      badgeText: multiplier ? `Nitro ${formatMultiplier(multiplier)}` : null,
      claimed,
      icon: reward.asset ? 'asset' : 'orbs',
    }
  }

  const kind: QuestRewardKind = isInGameReward(reward)
    ? 'ingame'
    : isCollectibleReward(reward)
      ? 'collectible'
      : 'discord'

  return {
    kind,
    name,
    asset: reward.asset || null,
    skuId: reward.sku_id,
    type: reward.type,
    amountText: reward.quantity != null ? `${name} x${reward.quantity}` : name,
    badgeText: null,
    claimed,
    icon: reward.asset ? 'asset' : 'gift',
  }
}

export function getQuestRewardViews(quest: Quest, userPremiumType?: number | null): QuestRewardView[] {
  return (quest.config.rewards_config?.rewards || []).map(reward => formatQuestReward(reward, quest.user_status, userPremiumType))
}

export function getQuestRewardCategory(quest: Quest): 'orbs' | 'avatar' | 'ingame' {
  const rewards = quest.config.rewards_config?.rewards || []
  if (rewards.some(isOrbReward)) return 'orbs'
  if (rewards.some(isCollectibleReward)) return 'avatar'
  return 'ingame'
}
