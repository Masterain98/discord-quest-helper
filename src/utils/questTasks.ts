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
    ?? null
}
