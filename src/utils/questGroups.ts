import type { Quest } from '@/api/tauri'
import { firstProgressValue, firstStartableTask, firstTargetTask, getQuestKind, getQuestTasks, isDesktopPlayTask, isVideoTask } from './questTasks'

export interface QuestDisplayGroup {
  key: string
  quests: Quest[]
}

/** Exact display identity, independent of region, target duration and account state. */
export function questGroupKey(quest: Quest): string | null {
  const { quest_name: name, game_title: game } = quest.config.messages
  const types = [...new Set(getQuestTasks(quest).map(task => task.type))].sort()
  if (!name?.trim() || !game?.trim() || !types.length || types.some(type => !type.trim())) return null
  return JSON.stringify([name, game, types])
}

/** Group only the supplied (already filtered and sorted) members. Never reorder input. */
export function groupQuests(quests: Quest[]): QuestDisplayGroup[] {
  const groups = new Map<string, QuestDisplayGroup>()
  const seen = new Set<string>()
  for (const quest of quests) {
    if (seen.has(quest.id)) continue
    seen.add(quest.id)
    const key = questGroupKey(quest) ?? `id:${quest.id}`
    const group = groups.get(key)
    if (group) group.quests.push(quest)
    else groups.set(key, { key, quests: [quest] })
  }
  return [...groups.values()]
}

export function questIsExpired(quest: Quest, now = Date.now()): boolean {
  return !!quest.config.expires_at && new Date(quest.config.expires_at).getTime() < now
}

export function canAcceptGroupMember(quest: Quest): boolean {
  return !quest.user_status?.enrolled_at && !quest.user_status?.completed_at && !quest.user_status?.claimed_at && !questIsExpired(quest)
}

export function canClaimGroupMember(quest: Quest): boolean {
  return !!quest.user_status?.completed_at && !quest.user_status.claimed_at && !questIsExpired(quest)
}

export function canQueueGroupMember(quest: Quest): boolean {
  if (!quest.user_status?.enrolled_at || quest.user_status.completed_at || quest.user_status.claimed_at || questIsExpired(quest)) return false
  if (getQuestKind(quest) === 'activity') return false
  const task = firstStartableTask(quest)
  return !!task && (isVideoTask(task) || isDesktopPlayTask(task))
}

export function groupStats(quests: Quest[]) {
  return {
    accepted: quests.filter(quest => quest.user_status?.enrolled_at).length,
    completed: quests.filter(quest => quest.user_status?.completed_at || quest.user_status?.claimed_at).length,
    claimable: quests.filter(canClaimGroupMember).length,
  }
}

export function groupProgress(quests: Quest[], active?: { questId: string | null, percentage: number }) {
  const completed = quests.filter(quest => quest.user_status?.completed_at || quest.user_status?.claimed_at).length
  const total = quests.length
  const sum = quests.reduce((sum, quest) => {
    if (quest.user_status?.completed_at || quest.user_status?.claimed_at) return sum + 100
    if (!quest.user_status?.enrolled_at) return sum
    const task = firstStartableTask(quest) ?? firstTargetTask(quest)
    const percent = active?.questId === quest.id
      ? active.percentage
      : task?.target ? firstProgressValue(quest, task.key) / task.target * 100 : 0
    return sum + (Number.isFinite(percent) ? Math.max(0, Math.min(100, percent)) : 0)
  }, 0)
  return {
    completed, total,
    percentage: total ? sum / total : 0,
    // A 100% counter alone is not Discord's confirmation of completion.
    isCompleted: total > 0 && completed === total,
  }
}

/** A read-only projection. It must never be used for execution or completion checks. */
export function hasLinkedQuestProgress(quest: Quest, active: Quest | null | undefined, now = Date.now()): boolean {
  if (!active || active.user_status?.completed_at || active.user_status?.claimed_at) return false
  if (!quest.user_status?.enrolled_at || quest.user_status.completed_at || quest.user_status.claimed_at || questIsExpired(quest, now)) return false
  const key = questGroupKey(quest)
  return key !== null && key === questGroupKey(active)
}
