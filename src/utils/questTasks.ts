import type { Quest, QuestTaskConfigEntry } from '@/api/tauri'

export interface QuestTaskView {
  key: string
  type: string
  target?: number
  targetText: string
  label: string
  applications?: Array<{ id: string }>
  externalIds?: string[]
  assets?: unknown
  messages?: Record<string, string>
  eventName?: string
}

export type QuestKind = 'video' | 'stream' | 'activity'

const TASK_LABELS: Record<string, string> = {
  WATCH_VIDEO: 'Desktop Video',
  WATCH_VIDEO_ON_MOBILE: 'Mobile Video',
  PLAY_ON_DESKTOP: 'Desktop Play',
  PLAY_ON_XBOX: 'Xbox',
  PLAY_ON_PLAYSTATION: 'PlayStation',
  STREAM_ON_DESKTOP: 'Stream',
  ACHIEVEMENT_IN_ACTIVITY: 'Activity Achievement',
  PLAY_ACTIVITY: 'Activity - Cloud Game',
}

/** Keep task platform colors consistent across account and public quest lists. */
export function questTaskBadgeClass(type: string): string {
  if (type === 'PLAY_ON_XBOX') {
    return 'border-emerald-500/55 bg-emerald-500/10 text-emerald-700 shadow-[0_1px_5px_rgb(16_185_129_/_0.35)] dark:text-emerald-300'
  }
  if (type === 'PLAY_ON_PLAYSTATION') {
    return 'border-blue-500/55 bg-blue-500/10 text-blue-700 shadow-[0_1px_5px_rgb(59_130_246_/_0.35)] dark:text-blue-300'
  }
  if (type === 'PLAY_ON_DESKTOP') {
    return 'border-indigo-500/55 bg-indigo-500/10 text-indigo-700 shadow-[0_1px_5px_rgb(99_102_241_/_0.35)] dark:text-indigo-300'
  }
  if (type === 'WATCH_VIDEO_ON_MOBILE') {
    return 'border-fuchsia-500/55 bg-fuchsia-500/10 text-fuchsia-700 shadow-[0_1px_5px_rgb(217_70_239_/_0.35)] dark:text-fuchsia-300'
  }
  if (type === 'WATCH_VIDEO') {
    return 'border-sky-500/55 bg-sky-500/10 text-sky-700 shadow-[0_1px_5px_rgb(14_165_233_/_0.35)] dark:text-sky-300'
  }
  if (type === 'ACHIEVEMENT_IN_ACTIVITY' || type === 'ACHIEVEMENT_IN_GAME') {
    return 'border-amber-500/55 bg-amber-500/10 text-amber-700 shadow-[0_1px_5px_rgb(245_158_11_/_0.35)] dark:text-amber-300'
  }
  if (type === 'PLAY_ACTIVITY') {
    return 'border-violet-500/55 bg-violet-500/10 text-violet-700 shadow-[0_1px_5px_rgb(139_92_246_/_0.35)] dark:text-violet-300'
  }
  return 'border-border bg-background text-foreground shadow-[0_1px_4px_rgb(0_0_0_/_0.18)]'
}

export function formatDuration(seconds: number): string {
  const totalSeconds = Math.max(0, Math.round(seconds))
  const minutes = Math.floor(totalSeconds / 60)
  const secs = totalSeconds % 60

  if (minutes >= 60) {
    const hours = Math.floor(minutes / 60)
    const mins = minutes % 60
    return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`
  }

  if (minutes === 0) return `${secs}s`
  return secs > 0 ? `${minutes}m ${secs}s` : `${minutes}m`
}

function taskLabel(type: string): string {
  return TASK_LABELS[type] ?? type.replace(/_/g, ' ').toLowerCase().replace(/\b\w/g, char => char.toUpperCase())
}

function targetText(type: string, target?: number): string {
  if (target == null) return ''
  if (type === 'ACHIEVEMENT_IN_ACTIVITY') {
    return `${target} task${target === 1 ? '' : 's'}`
  }
  return formatDuration(target)
}

function toTaskView(key: string, task: QuestTaskConfigEntry): QuestTaskView {
  const type = task.type || key
  return {
    key,
    type,
    target: task.target,
    targetText: targetText(type, task.target),
    label: taskLabel(type),
    applications: task.applications,
    externalIds: task.external_ids,
    assets: task.assets,
    messages: task.messages,
    eventName: task.event_name,
  }
}

export function getQuestTasks(quest: Quest): QuestTaskView[] {
  const tasks = quest.config.task_config_v2?.tasks ?? quest.config.task_config?.tasks
  if (!tasks) return []
  return Object.entries(tasks).map(([key, task]) => toTaskView(key, task))
}

export function getQuestKind(quest: Quest): QuestKind {
  const tasks = getQuestTasks(quest)
  if (tasks.some(task => task.type.includes('ACTIVITY') || task.type.includes('ACHIEVEMENT'))) {
    return 'activity'
  }
  if (tasks.some(task => task.type.includes('STREAM') || task.type.includes('PLAY'))) {
    return 'stream'
  }
  return 'video'
}

export function isVideoTask(task: QuestTaskView): boolean {
  return task.type === 'WATCH_VIDEO' || task.type === 'WATCH_VIDEO_ON_MOBILE' || task.type.includes('VIDEO')
}

export function isDesktopPlayTask(task: QuestTaskView): boolean {
  return task.type === 'PLAY_ON_DESKTOP'
}

export function isStreamTask(task: QuestTaskView): boolean {
  return task.type.includes('STREAM')
}

export function isActivityTask(task: QuestTaskView): boolean {
  return task.type.includes('ACTIVITY') || task.type.includes('ACHIEVEMENT')
}

export function isPlayActivityTask(task: QuestTaskView): boolean {
  return task.type === 'PLAY_ACTIVITY'
}

export function playActivityProgressPercentage(
  progressSeconds: number,
  targetSeconds: number,
  completed = false
): number {
  if (completed) return 100
  if (targetSeconds <= 0) return 0
  return Math.min(99, Math.max(0, progressSeconds / targetSeconds * 100))
}

export function firstProgressValue(quest: Quest, taskKey?: string): number {
  const progress = quest.user_status?.progress
  if (!progress || typeof progress !== 'object') return 0

  if (taskKey && progress[taskKey]?.value != null) {
    return progress[taskKey].value ?? 0
  }

  const first = Object.values(progress)[0]
  return first?.value ?? 0
}

export function firstTargetTask(quest: Quest): QuestTaskView | null {
  return getQuestTasks(quest).find(task => task.target != null && task.target > 0) ?? null
}

export function firstStartableTask(quest: Quest): QuestTaskView | null {
  const tasks = getQuestTasks(quest)
  return tasks.find(task => isVideoTask(task) && task.target != null && task.target > 0)
    ?? tasks.find(task => isDesktopPlayTask(task) && task.target != null && task.target > 0)
    ?? tasks.find(task => isStreamTask(task) && task.target != null && task.target > 0)
    ?? tasks.find(task => isActivityTask(task) && task.target != null && task.target > 0)
    ?? null
}
