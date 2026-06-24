import { computed, type Ref } from 'vue'
import type { Quest } from '@/api/tauri'
import { firstStartableTask, getQuestKind } from '@/utils/questTasks'

export type QuestViewPreset =
  | 'recommended'
  | 'to_accept'
  | 'ready_to_run'
  | 'ready_to_claim'
  | 'completed'
  | 'all'

export type AdvancedQuestFilters = {
  query: string
  types: {
    video: boolean
    play: boolean
    activity: boolean
  }
  rewards: {
    orbs: boolean
    avatarDecoration: boolean
    ingame: boolean
  }
  includeExpired: boolean
}

export type QuestBucket = {
  active: Quest | null
  queued: Quest[]
  blockedUntil: string | null
  toAccept: Quest[]
  readyToRun: Quest[]
  readyToClaim: Quest[]
  completed: Quest[]
  expired: Quest[]
  activityManual: Quest[]
  attentionNeeded: Quest[]
}

type HomeQuestStateOptions = {
  activeQuestId?: string | null
  questQueue?: Quest[]
  blockedUntil?: string | null
  cdpAvailable?: boolean
  now?: Date
}

type HomeQuestStateRefs = {
  activeQuestId?: Ref<string | null>
  questQueue?: Ref<Quest[]>
  blockedUntil?: Ref<string | null>
  cdpAvailable?: Ref<boolean>
}

export const emptyAdvancedQuestFilters = (): AdvancedQuestFilters => ({
  query: '',
  types: {
    video: false,
    play: false,
    activity: false,
  },
  rewards: {
    orbs: false,
    avatarDecoration: false,
    ingame: false,
  },
  includeExpired: false,
})

export function isQuestExpired(quest: Quest, now = new Date()): boolean {
  if (!quest.config.expires_at) return false
  return new Date(quest.config.expires_at).getTime() < now.getTime()
}

export function isEnrollmentBlocked(blockedUntil: string | null | undefined, now = new Date()): boolean {
  if (!blockedUntil) return false
  return new Date(blockedUntil).getTime() > now.getTime()
}

export function canQuestStart(quest: Quest, cdpAvailable = false): boolean {
  if (quest.user_status?.completed_at) return false
  if (!firstStartableTask(quest)) return false

  const kind = getQuestKind(quest)
  if (kind === 'activity') return cdpAvailable

  return true
}

function isEnrolled(quest: Quest): boolean {
  return !!quest.user_status?.enrolled_at
}

function isCompleted(quest: Quest): boolean {
  return !!quest.user_status?.completed_at
}

function isClaimed(quest: Quest): boolean {
  return !!quest.user_status?.claimed_at
}

export function deriveHomeQuestBuckets(quests: Quest[], options: HomeQuestStateOptions = {}): QuestBucket {
  const now = options.now ?? new Date()
  const cdpAvailable = options.cdpAvailable ?? false
  const blockedUntil = isEnrollmentBlocked(options.blockedUntil, now) ? options.blockedUntil ?? null : null

  const bucket: QuestBucket = {
    active: options.activeQuestId ? quests.find(quest => quest.id === options.activeQuestId) ?? null : null,
    queued: [...(options.questQueue ?? [])],
    blockedUntil,
    toAccept: [],
    readyToRun: [],
    readyToClaim: [],
    completed: [],
    expired: [],
    activityManual: [],
    attentionNeeded: [],
  }

  for (const quest of quests) {
    const expired = isQuestExpired(quest, now)
    const enrolled = isEnrolled(quest)
    const completed = isCompleted(quest)
    const claimed = isClaimed(quest)
    const kind = getQuestKind(quest)
    const startable = canQuestStart(quest, cdpAvailable)

    if (expired && !claimed) {
      bucket.expired.push(quest)
    }

    if (claimed) {
      bucket.completed.push(quest)
      continue
    }

    if (completed) {
      bucket.completed.push(quest)
      if (!expired) {
        bucket.readyToClaim.push(quest)
      }
      continue
    }

    if (!enrolled && !expired) {
      bucket.toAccept.push(quest)
      continue
    }

    if (enrolled && !completed && !expired) {
      if (kind === 'activity') {
        bucket.activityManual.push(quest)
      }

      if (startable) {
        bucket.readyToRun.push(quest)
      } else {
        bucket.attentionNeeded.push(quest)
      }
    }
  }

  return bucket
}

export function getRecommendedQuests(bucket: QuestBucket): Quest[] {
  const seen = new Set<string>()
  const ordered = [
    ...bucket.readyToClaim,
    ...bucket.readyToRun,
    ...bucket.toAccept,
    ...bucket.attentionNeeded,
  ]

  return ordered.filter(quest => {
    if (seen.has(quest.id)) return false
    seen.add(quest.id)
    return true
  })
}

export function useHomeQuestState(quests: Ref<Quest[]>, options: HomeQuestStateRefs = {}) {
  const buckets = computed(() => deriveHomeQuestBuckets(quests.value, {
    activeQuestId: options.activeQuestId?.value ?? null,
    questQueue: options.questQueue?.value ?? [],
    blockedUntil: options.blockedUntil?.value ?? null,
    cdpAvailable: options.cdpAvailable?.value ?? false,
  }))

  return {
    buckets,
    recommendedQuests: computed(() => getRecommendedQuests(buckets.value)),
  }
}
