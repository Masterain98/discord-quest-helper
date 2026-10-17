export type GlobalTaskKind = 'watch' | 'play' | 'stream' | 'activity' | 'other'
export type GlobalRewardKind = 'orbs' | 'avatar' | 'ingame' | 'nitro' | 'other'
export type GlobalRegionKind = 'global' | 'restricted' | 'unknown'
export type GlobalAgeKind = 'adult' | 'unmarked' | 'unknown'

export interface GlobalQuestTask {
  type: string
  kind: GlobalTaskKind
  target: number | null
}

export interface GlobalQuestReward {
  kind: GlobalRewardKind
  name: string
  quantity: number | null
  premiumQuantity: number | null
  assetUrl: string | null
  expiresAt: string | null
  instructions: string[]
}

/** Public catalog entries carry no account status and cannot be executed. */
export interface GlobalQuest {
  id: string
  name: string
  gameName: string
  description: string
  startsAt: string | null
  expiresAt: string | null
  thumbnailUrl: string | null
  tasks: GlobalQuestTask[]
  rewards: GlobalQuestReward[]
}

export interface GlobalQuestRestriction {
  region: GlobalRegionKind
  include: string[]
  exclude: string[]
  age: GlobalAgeKind
}

export interface GlobalQuestFilters {
  query: string
  tasks: GlobalTaskKind[]
  rewards: GlobalRewardKind[]
  regions: GlobalRegionKind[]
  ages: GlobalAgeKind[]
  country: string
}
